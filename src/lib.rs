use std::{
    fs::{self, OpenOptions},
    io::{BufReader, BufWriter},
    path::PathBuf,
    time::{Duration, SystemTime},
};

use serde::{de::DeserializeOwned, Serialize};

pub struct FileBackedValue<T>
    where T: Serialize + DeserializeOwned
{
    path: PathBuf,
    value: Option<T>,
    dirty_time: Option<Duration>,
}

// TODO: create error type to replace the Options
//pub enum FileBackedValueError {}

//pub type FileBackedValueResult<T> = Result<FileBackedValue<T>, FileBackedValueError>;

// TODO: increase safety by getting rid of some unwraps.
impl<T> FileBackedValue<T>
    where T: Serialize + DeserializeOwned
{
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            value: None,
            dirty_time: None,
        }
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
    pub fn get(&mut self) -> Option<&T> {
        if self.value.is_none() || self.file_is_dirty() {
            // The backing file has not been read before or has become dirty.
            self.value = self.read_file();
        }
        self.value.as_ref()
    }

    pub fn get_or_insert(&mut self, default: T) -> &T {
        if self.file_is_dirty() {
            // If the file is dirty, recompute even if we already have a value.
            self.insert(default)
        } else if self.value.is_none() {
            // The file has not been read before; read it now and store the value.
            // The file must exists because otherwise it will have been marked as dirty.
            let value = self.read_file().unwrap();
            self.insert(value)
        } else {
            // The file is not dirty, return the current value if it exists.
            self.value.as_ref().unwrap()
        }
    }

    pub fn get_or_insert_with<F>(&mut self, default: F) -> &T
        where F: FnOnce() -> T
    {
        if self.file_is_dirty() {
            // If the file is dirty, recompute even if we already have a value.
            self.insert((default)())
        } else if self.value.is_none() {
            // The file has not been read before; read it now and store the value.
            // The file must exists because otherwise it will have been marked as dirty.
            let value = self.read_file().unwrap();
            self.insert(value)
        } else {
            // The file is not dirty, return the current value if it exists.
            self.value.as_ref().unwrap()
        }
    }

    /// Inserts `value` into the option and writes it to the backing file.
    /// Returns a mutable reference to the value.
    pub fn insert(&mut self, value: T) -> &T {
        self.write_file(&value).unwrap();
        self.value.insert(value)
    }

    /// Read a value of type `T` from the backing file as a JSON string.
    fn read_file(&self) -> Option<T> {
        if let Ok(file) = OpenOptions::new().read(true).open(&self.path) {
            let rdr = BufReader::new(file);
            serde_json::from_reader(rdr).ok()
        } else {
            None
        }
    }

    /// Write `value` to the backing file as a JSON string.
    fn write_file(&self, value: &T) -> Option<()> {
        // Create parent directories if necessary.
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir).unwrap();
        }

        let file = OpenOptions::new().create_new(true).write(true).open(&self.path).ok()?;
        let wtr = BufWriter::new(file);
        serde_json::to_writer(wtr, value).ok()
    }

    /// Check whether the backing file was last modified longer than `dirty_time` ago.
    /// If the file does not exist or the modification time could otherwise not be retrieved, true is returned.
    fn file_is_dirty(&self) -> bool {
        self.dirty_time.is_some_and(|dirty_time|
            file_needs_recomputation(&self.path, dirty_time))
    }
}

/// Check whether the file at `path` was last modified longer than `dirty_time` ago.
/// If the file does not exist or the modification time could otherwise not be retrieved, true is returned.
fn file_needs_recomputation(path: &PathBuf, dirty_time: Duration) -> bool {
    time_since_last_modified(path).is_none_or(|last_modified|
        last_modified >= dirty_time)
}

/// Get the duration since the file at `path` was last modified.
fn time_since_last_modified(path: &PathBuf) -> Option<Duration> {
    if let Ok(time) = fs::metadata(path) {
        let now = SystemTime::now();
        let last_modified = time.modified().ok()?;
        now.duration_since(last_modified).ok()
    } else {
        None
    }
}
