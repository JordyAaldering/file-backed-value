use std::{
    fs::{self, OpenOptions},
    io::{self, BufReader, BufWriter},
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use serde::{de::DeserializeOwned, Serialize};

pub struct FileBackedValue<T>
    where T: Serialize + DeserializeOwned
{
    dir: PathBuf,
    filename: String,
    value: Option<T>,
    dirty_time: Option<Duration>,
}

#[derive(Debug)]
pub enum FileBackedValueError {
    FileError(io::Error),
    JsonError(serde_json::Error),
}

pub type FileBackedValueResult<T> = Result<T, FileBackedValueError>;

impl<T> FileBackedValue<T>
    where T: Serialize + DeserializeOwned
{
    pub fn new(filename: &str) -> Self {
        Self {
            dir: PathBuf::from(directories::BaseDirs::new().expect("No valid home directory found").data_dir()),
            filename: sanitize_filename::sanitize(filename),
            value: None,
            dirty_time: None,
        }
    }

    pub fn new_at(filename: &str, dir: &Path) -> Self {
        Self {
            dir: PathBuf::from(dir),
            filename: sanitize_filename::sanitize(filename),
            value: None,
            dirty_time: None,
        }
    }

    pub fn path(&self) -> PathBuf {
        self.dir.join(&self.filename)
    }

    /// If the time since the file was last edited is longer ago than `dirty_time`,
    /// require a recomputation of the value and a writeback to the file.
    /// If this value is not set, the file is only ever read once.
    pub fn set_dirty_time(&mut self, dirty_time: Duration) {
        self.dirty_time = Some(dirty_time);
    }

    /// Make this file dirty, requiring a recomputation the next time a value is get.
    /// Returns the currently stored value, if any.
    pub fn set_dirty(&mut self) -> Option<T> {
        self.value.take()
    }

    /// Get the current value, which might be None if the backing file does not yet exist.
    pub fn get(&mut self) -> FileBackedValueResult<Option<&T>> {
        if self.value.is_none() || self.file_is_dirty() {
            // The backing file has not been read before or has become dirty.
            self.value = self.read_file()?;
        }
        Ok(self.value.as_ref())
    }

    pub fn get_or_insert(&mut self, default: T) -> FileBackedValueResult<&T> {
        if self.file_is_dirty() {
            // If the file is dirty, recompute even if we already have a value.
            Ok(self.insert(default))
        } else if self.value.is_none() {
            // The file has not been read before; read it now and store the value.
            // The file must exists because otherwise it will have been marked as dirty.
            let value = self.read_file()?.unwrap();
            Ok(self.value.insert(value))
        } else {
            // The file is not dirty, return the current value if it exists.
            Ok(self.value.as_ref().unwrap())
        }
    }

    pub fn get_or_insert_with<F>(&mut self, default: F) -> FileBackedValueResult<&T>
        where F: FnOnce() -> T
    {
        if self.file_is_dirty() {
            // If the file is dirty, recompute even if we already have a value.
            Ok(self.insert((default)()))
        } else if self.value.is_none() {
            // The file has not been read before; read it now and store the value.
            // The file must exists because otherwise it will have been marked as dirty.
            let value = self.read_file()?.unwrap();
            Ok(self.value.insert(value))
        } else {
            // The file is not dirty, return the current value if it exists.
            Ok(self.value.as_ref().unwrap())
        }
    }

    /// Inserts `value` into the option and writes it to the backing file.
    /// Returns a mutable reference to the value.
    pub fn insert(&mut self, value: T) -> &T {
        self.write_file(&value).unwrap();
        self.value.insert(value)
    }

    /// Read a value of type `T` from the backing file as a JSON string.
    fn read_file(&self) -> FileBackedValueResult<Option<T>> {
        match OpenOptions::new().read(true).open(self.path()) {
            Ok(f) => {
                let rdr = BufReader::new(f);
                serde_json::from_reader(rdr)
                    .map_err(|e| FileBackedValueError::JsonError(e))
                    .map(|json| Some(json))
            },
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(FileBackedValueError::FileError(e))
        }
    }

    /// Write `value` to the backing file as a JSON string.
    fn write_file(&self, value: &T) -> FileBackedValueResult<()> {
        // Create parent directories if necessary.
        fs::create_dir_all(&self.dir)
            .map_err(|e| FileBackedValueError::FileError(e))?;

        let path = self.path();
        let file = OpenOptions::new().create_new(true).write(true).open(path)
            .map_err(|e| FileBackedValueError::FileError(e))?;
        let wtr = BufWriter::new(file);
        serde_json::to_writer(wtr, value)
            .map_err(|e| FileBackedValueError::JsonError(e))
    }

    /// Check whether the backing file was last modified longer than `dirty_time` ago.
    /// If the file does not exist or the modification time could otherwise not be retrieved, true is returned.
    fn file_is_dirty(&self) -> bool {
        self.dirty_time.is_some_and(|dirty_time|
            file_needs_recomputation(&self.path(), dirty_time))
    }
}

/// Check whether the file at `path` was last modified longer than `dirty_time` ago.
/// If the file does not exist or the modification time could otherwise not be retrieved, true is returned.
fn file_needs_recomputation(path: &Path, dirty_time: Duration) -> bool {
    time_since_last_modified(path).is_none_or(|last_modified|
        last_modified >= dirty_time)
}

/// Get the duration since the file at `path` was last modified.
fn time_since_last_modified(path: &Path) -> Option<Duration> {
    if let Ok(time) = fs::metadata(path) {
        let now = SystemTime::now();
        let last_modified = time.modified().ok()?;
        now.duration_since(last_modified).ok()
    } else {
        None
    }
}
