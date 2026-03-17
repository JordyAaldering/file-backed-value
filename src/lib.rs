use std::{fs, io::{self, BufReader, BufWriter}, path::PathBuf, time::{Duration, SystemTime}};

use serde::{de::DeserializeOwned, Serialize};

pub struct FileBackedValue
{
    path: PathBuf,
    dirty_time: Option<Duration>,
}

#[derive(Debug)]
pub enum FileBackedValueError {
    FileError(io::Error),
    JsonError(serde_json::Error),
}

pub type FileBackedValueResult<T> = Result<T, FileBackedValueError>;

impl FileBackedValue
{
    /// Create a file-backed value in the user's data directory.
    ///
    /// ### Example
    /// - Linux: `/home/alice/.local/share`
    /// - MacOS: `/Users/Alice/Library/Application Support`
    /// - Windows: `C:\Users\Alice\AppData\Roaming`
    pub fn new(filename: &str) -> Self {
        let parent = PathBuf::from(directories::BaseDirs::new().expect("No valid home directory found").data_dir());
        let filename = sanitize_filename::sanitize(filename);
        Self {
            path: parent.join(filename),
            dirty_time: None,
        }
    }

    /// Create a file-backed value in the specified directory.
    pub fn new_at(filename: &str, parent: &str) -> Self {
        let parent = PathBuf::from(parent);
        let filename = sanitize_filename::sanitize(filename);
        Self {
            path: parent.join(filename),
            dirty_time: None,
        }
    }

    /// Set the duration after which the file is considered dirty and needs to be recomputed.
    pub fn set_dirty_time(&mut self, dirty_time: Duration) {
        self.dirty_time = Some(dirty_time);
    }

    /// Path to the backing file.
    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    /// Clear the currently stored value and remove the backing file.
    pub fn clear(&mut self) -> io::Result<()> {
        fs::remove_file(self.path())
    }

    /// Get the current value, which might be None if the backing file does not yet exist.
    pub fn get<T>(&mut self) -> FileBackedValueResult<Option<T>>
    where
        T: DeserializeOwned
    {
        if self.file_is_dirty() {
            Ok(None)
        } else {
            read_file(&self.path)
        }
    }

    /// Get the current value, or insert `default` if the backing file does not exist or is dirty.
    pub fn get_or_insert<T>(&mut self, default: T) -> FileBackedValueResult<T>
    where
        T: DeserializeOwned + Serialize
    {
        if self.file_is_dirty() {
            self.insert(&default);
            Ok(default)
        } else {
            // The file has not been read before; read it now and store the value.
            // The file must exists because otherwise it will have been marked as dirty.
            let res: Option<T> = read_file(&self.path)?;
            Ok(res.unwrap())
        }
    }

    /// Get the current value, or insert `default()` if the backing file does not exist or is dirty.
    pub fn get_or_insert_with<F, T>(&mut self, default: F) -> FileBackedValueResult<T>
    where
        F: FnOnce() -> T,
        T: DeserializeOwned + Serialize,
    {
        if self.file_is_dirty() {
            let res = default();
            self.insert(&res);
            Ok(res)
        } else {
            // The file has not been read before; read it now and store the value.
            // The file must exists because otherwise it will have been marked as dirty.
            let res: Option<T> = read_file(&self.path)?;
            Ok(res.unwrap())
        }
    }

    /// Inserts `value` into the option and writes it to the backing file.
    pub fn insert<T>(&mut self, value: &T)
    where
        T: Serialize
    {
        write_file(&self.path, value).unwrap();
    }

    /// Check whether the backing file was last modified longer than `dirty_time` ago.
    /// If the file does not exist or the modification time could otherwise not be retrieved, true is returned.
    fn file_is_dirty(&self) -> bool {
        self.dirty_time.is_some_and(|dirty_time|
            file_needs_recomputation(&self.path(), dirty_time))
    }
}

/// Read a value of type `T` from the backing file as a JSON string.
fn read_file<T>(path: &PathBuf) -> FileBackedValueResult<Option<T>>
where
    T: DeserializeOwned
{
    match fs::OpenOptions::new().read(true).open(path) {
        Ok(f) => {
            let rdr = BufReader::new(f);
            serde_json::from_reader(rdr)
                .map_err(Into::into)
                .map(|json| Some(json))
        },
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(FileBackedValueError::FileError(e))
    }
}

/// Write `value` to the backing file as a JSON string.
fn write_file<T>(path: &PathBuf, value: &T) -> FileBackedValueResult<()>
where
    T: Serialize
{
    // Create parent directories if necessary.
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(Into::into)?;
    }

    let file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .truncate(true)
        .open(path)
        .map_err(Into::into)?;

    let wtr = BufWriter::new(file);
    serde_json::to_writer(wtr, value).map_err(Into::into)
}

/// Check whether the file at `path` was last modified longer than `dirty_time` ago.
/// If the file does not exist or the modification time could otherwise not be retrieved, true is returned.
fn file_needs_recomputation(path: &PathBuf, dirty_time: Duration) -> bool {
    time_since_modified(path).is_none_or(|last_modified|
        last_modified >= dirty_time)
}

/// Get the duration since the file at `path` was last modified.
fn time_since_modified(path: &PathBuf) -> Option<Duration> {
    if let Ok(time) = fs::metadata(path) {
        let now = SystemTime::now();
        let last_modified = time.modified().or_else(|_| time.created()).ok()?;
        now.duration_since(last_modified).ok()
    } else {
        None
    }
}

impl Into<FileBackedValueError> for io::Error {
    fn into(self) -> FileBackedValueError {
        FileBackedValueError::FileError(self)
    }
}

impl Into<FileBackedValueError> for serde_json::Error {
    fn into(self) -> FileBackedValueError {
        FileBackedValueError::JsonError(self)
    }
}
