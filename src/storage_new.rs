use clap::error::Result;
use regex::regex;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::sync::Mutex;
use std::{fs::File, os::unix::fs::FileExt, time::SystemTime};
use thiserror::Error;

use crate::config::Config;
use crate::storage_new::StorageError::{
    Corupted, FileCapacityExceeded, InvalidDBPath, InvalidIndex, StorageCapacityExceeded,
};

#[derive(Error, Debug)]
pub enum StorageError {
    #[error("IO error: {0}")]
    IOError(#[from] std::io::Error),
    #[error("Checksum for file {0} at offset {1} with length {2} is not valid.")]
    Corupted(String, u64, u64),
    #[error("File {0} with capacity {1}MB would be overfilled by write of {2} bytes.")]
    FileCapacityExceeded(String, u64, usize),
    #[error("Database capacity of {0} would be exceeded by write of size {1}.")]
    StorageCapacityExceeded(u64, u64),
    #[error("The item at the path {0} is not a directory.")]
    InvalidDBPath(String),
    #[error("The index {0} does not correspond to any database file.")]
    InvalidIndex(u32),
}

pub struct StorageInstance {
    file: File,

    filename: String,

    capacity: u64,

    // Only used for writes.
    next_offset: Mutex<u64>,
}

pub struct ReadResponse {
    pub timestamp: u64,
    pub bytes: Vec<u8>,
}

pub struct AppendResponse {
    pub file_index: u32,
    pub offset: u64,
    pub timestamp: u64,
}

impl StorageInstance {
    const HEADER_SIZE: u64 = 16;

    fn timestamp() -> u64 {
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64
    }

    fn checksum(bytes: &[u8]) -> u32 {
        crc32fast::hash(bytes)
    }

    fn new(file: File, filename: String, config: &Config) -> Result<Self, StorageError> {
        let next_offset = Mutex::new(file.metadata()?.len());
        Ok(StorageInstance {
            file,
            filename,
            capacity: config.file_size,
            next_offset,
        })
    }

    pub fn append_len(bytes: &[u8]) -> u64 {
        bytes.len() as u64 + Self::HEADER_SIZE
    }

    fn would_exceed_capacity(&self, bytes: &[u8]) -> bool {
        Self::append_len(bytes) + self.file.metadata().unwrap().len() > self.capacity
    }

    fn append(&self, bytes: &[u8]) -> Result<(u64, u64), StorageError> {
        if self.would_exceed_capacity(bytes) {
            return Err(FileCapacityExceeded(
                self.filename.clone(),
                self.capacity,
                bytes.len(),
            ));
        }

        let mut next_offset = self.next_offset.lock().unwrap();
        let cur_offset = *next_offset;

        self.file
            .write_all_at(&StorageInstance::checksum(bytes).to_be_bytes(), cur_offset)?;
        let timestamp = StorageInstance::timestamp();
        self.file
            .write_all_at(&timestamp.to_be_bytes(), cur_offset + 4)?;
        self.file
            .write_all_at(&(bytes.len() as u32).to_be_bytes(), cur_offset + 12)?;
        self.file
            .write_all_at(bytes, *next_offset + Self::HEADER_SIZE)?;
        *next_offset += Self::HEADER_SIZE + bytes.len() as u64;

        Ok((cur_offset, timestamp))
    }

    fn set_read_only(&self) -> Result<(), StorageError> {
        self.file.metadata()?.permissions().set_readonly(true);
        Ok(())
    }

    fn parse_u32(bytes: &[u8], offset: u64) -> u32 {
        let offset = offset as usize;
        u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap())
    }

    fn parse_u64(bytes: &[u8], offset: u64) -> u64 {
        let offset = offset as usize;
        u64::from_be_bytes(bytes[offset..offset + 8].try_into().unwrap())
    }

    fn read(&self, offset: u64, size: u64) -> Result<ReadResponse, StorageError> {
        let mut buffer: Vec<u8> = vec![0; (size + Self::HEADER_SIZE) as usize];

        self.file.read_exact_at(&mut buffer, offset)?;

        let checksum = Self::parse_u32(&buffer, 0);
        if checksum != crc32fast::hash(&buffer[Self::HEADER_SIZE as usize..]) {
            return Err(Corupted(self.filename.clone(), offset, size));
        }

        let timestamp = Self::parse_u64(&buffer, 4);

        Ok(ReadResponse {
            bytes: buffer[Self::HEADER_SIZE as usize..].into(),
            timestamp,
        })
    }

    // Uses the encoded size rather than reading the size from the keydir.
    pub fn read_at(&self, offset: u64) -> Result<ReadResponse, StorageError> {
        let mut header: Vec<u8> = vec![0; Self::HEADER_SIZE as usize];
        self.file.read_exact_at(&mut header, offset)?;

        let size = Self::parse_u32(&header, 12);
        let bytes: Vec<u8> = vec![0; size as usize];

        let checksum = Self::parse_u32(&header, 0);
        if checksum != crc32fast::hash(&bytes) {
            return Err(Corupted(self.filename.clone(), offset, size as u64));
        }

        let timestamp = Self::parse_u64(&header, 4);

        Ok(ReadResponse {
            bytes: bytes.into(),
            timestamp,
        })
    }

    fn flush(&self) -> Result<(), StorageError> {
        self.file.sync_all().map_err(Into::into)
    }

    pub fn len(&self) -> Result<u64, StorageError> {
        Ok(self.file.metadata()?.len())
    }
}

pub struct Storage {
    pub files: boxcar::Vec<StorageInstance>,
    config: Config,
    size: Mutex<u64>,
    max_bytes: u64,
}

impl Storage {
    const ROOT_FILE_NAME: &str = "db_file_";
    const INITIAL_FILE_NAME: &str = "db_file_00000";

    // Assumes file names of the form: db_file_00000, db_file_00001, etc.
    // Returns None if the filename does not match that pattern.
    fn extract_file_number(filename: OsString) -> Option<(String, u32)> {
        let Ok(filename) = filename.into_string() else {
            return None;
        };

        let caps = regex!(r"^db_file_(?<number>\d{5})$").captures(&filename)?;

        Some((
            caps.get_match().as_str().into(),
            caps.name("number").unwrap().as_str().parse().unwrap(),
        ))
    }

    fn open_file(path: &str, create: bool) -> Result<File, StorageError> {
        OpenOptions::new()
            .read(true)
            .write(true)
            .create(create)
            .truncate(false)
            .open(path)
            .map_err(Into::into)
    }

    fn head_file(&self) -> Option<&StorageInstance> {
        self.files.get(self.files.count() - 1)
    }

    pub fn initialize(config: Config) -> Result<Self, StorageError> {
        let path = &config.database_path;
        if !fs::exists(path)? {
            fs::create_dir_all(path)?;
        }

        if !fs::metadata(path)?.is_dir() {
            return Err(InvalidDBPath(path.into()));
        }

        // Read existing files.
        let mut size: u64 = 0;
        let mut num_to_file: BTreeMap<u32, StorageInstance> = BTreeMap::new();
        for file in fs::read_dir(path)? {
            let file = file?;

            if !file.metadata()?.is_file() {
                continue;
            }

            if let Some((filename, num)) = Self::extract_file_number(file.file_name()) {
                let file = Self::open_file(file.path().to_str().unwrap(), false)?;

                size += file.metadata().unwrap().len();
                num_to_file.insert(num, StorageInstance::new(file, filename, &config)?);
            }
        }

        // If no database files exist, create the initial file.
        if num_to_file.is_empty() {
            let file = Self::open_file(Self::INITIAL_FILE_NAME, true)?;
            num_to_file.insert(
                0,
                StorageInstance::new(file, Self::INITIAL_FILE_NAME.into(), &config)?,
            );
        }

        let files = boxcar::Vec::with_capacity(1000);
        for file in num_to_file.into_values() {
            files.push(file);
        }

        let max_bytes = config.max_db_size;

        Ok(Storage {
            config,
            files,
            max_bytes,
            size: Mutex::new(size),
        })
    }

    pub fn append(&self, bytes: &[u8]) -> Result<AppendResponse, StorageError> {
        // Chec if max db size would be exceeded.
        let mut size = self.size.lock().unwrap();
        if StorageInstance::append_len(bytes) + *size > self.max_bytes {
            return Err(StorageCapacityExceeded(self.max_bytes, bytes.len() as u64));
        }

        // Create a new file if current head file would overflow.
        if self.head_file().unwrap().would_exceed_capacity(bytes) {
            let head = self.head_file().unwrap();
            head.flush()?;
            head.set_read_only()?;

            let next_file_name = format!("{}{:05}", Self::ROOT_FILE_NAME, self.files.count());
            let next_file_path = format!("{}{}", self.config.database_path, next_file_name);
            let next_file = Self::open_file(&next_file_path, true)?;
            self.files.push(StorageInstance::new(
                next_file,
                next_file_name,
                &self.config,
            )?);
        }

        // Write the bytes.
        let (offset, timestamp) = self.head_file().unwrap().append(bytes)?;
        *size += StorageInstance::append_len(bytes);
        Ok(AppendResponse {
            file_index: self.files.count() as u32 - 1,
            offset,
            timestamp,
        })
    }

    pub fn read(
        &self,
        file_index: u32,
        offset: u64,
        size: u64,
    ) -> Result<ReadResponse, StorageError> {
        if file_index > self.files.count() as u32 {
            return Err(InvalidIndex(file_index));
        }

        self.files
            .get(file_index as usize)
            .unwrap()
            .read(offset, size)
    }

    pub fn flush(&self) -> Result<(), StorageError> {
        self.head_file().unwrap().flush().map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Seek, SeekFrom, Write};
    use std::path::{Path, PathBuf};
    use std::sync::{
        Arc, Barrier, Mutex,
        atomic::{AtomicU64, Ordering},
    };
    use std::thread;

    static NEXT_TEMP_DIR: AtomicU64 = AtomicU64::new(0);
    static CURRENT_DIR_LOCK: Mutex<()> = Mutex::new(());
    static ONE_MB: u64 = 2u64.pow(20);

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let id = NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("keyval-storage-new-{}-{id}", std::process::id()));
            fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    struct CurrentDirGuard(PathBuf);

    impl CurrentDirGuard {
        fn change_to(path: &Path) -> Self {
            let previous = std::env::current_dir().unwrap();
            std::env::set_current_dir(path).unwrap();
            Self(previous)
        }
    }

    impl Drop for CurrentDirGuard {
        fn drop(&mut self) {
            std::env::set_current_dir(&self.0).unwrap();
        }
    }

    fn config(database_path: &Path, file_size: u64, max_db_size: u64) -> Config {
        Config {
            database_path: format!("{}/", database_path.display()),
            command_mode: crate::config::CommandMode::Repl,
            socket_address: "127.0.0.1:0".parse().unwrap(),
            file_size,
            max_db_size,
            max_key_size: 1024,
            max_value_size: 1024,
        }
    }

    #[test]
    fn initializes_empty_database_directory() {
        let _lock = CURRENT_DIR_LOCK.lock().unwrap();
        let temp_dir = TempDir::new();
        let _current_dir = CurrentDirGuard::change_to(temp_dir.path());
        let config = Config {
            database_path: ".".into(),
            command_mode: crate::config::CommandMode::Repl,
            socket_address: "127.0.0.1:0".parse().unwrap(),
            file_size: ONE_MB,
            max_db_size: 2 * ONE_MB,
            max_key_size: 1024,
            max_value_size: 1024,
        };

        let storage = Storage::initialize(config).unwrap();

        assert_eq!(storage.files.count(), 1);
        assert!(Path::new(Storage::INITIAL_FILE_NAME).is_file());
    }

    #[test]
    fn initializes_from_numbered_files_with_gaps_and_ignores_other_files() {
        let temp_dir = TempDir::new();
        for filename in ["db_file_00000", "db_file_00002", "notes.txt"] {
            File::create(temp_dir.path().join(filename)).unwrap();
        }

        let storage = Storage::initialize(config(temp_dir.path(), 1, 4)).unwrap();

        assert_eq!(storage.files.count(), 2);
    }

    #[test]
    fn reads_and_writes_valid_data() {
        let temp_dir = TempDir::new();
        File::create(temp_dir.path().join(Storage::INITIAL_FILE_NAME)).unwrap();
        let storage = Storage::initialize(config(temp_dir.path(), 1024, 2048)).unwrap();
        let payload = b"append-only value";

        let append1 = storage.append(payload).unwrap();
        let read1 = storage.read(0, 0, payload.len() as u64).unwrap();
        let AppendResponse {
            file_index,
            offset,
            timestamp,
        } = storage.append(payload).unwrap();
        let read2 = storage
            .read(file_index, offset, payload.len() as u64)
            .unwrap();

        assert_eq!(read1.bytes, payload);
        assert_eq!(read1.timestamp, append1.timestamp);
        assert_eq!(append1.file_index, 0);
        assert_eq!(append1.offset, 0);
        assert_eq!(read2.bytes, payload);
        assert_eq!(read2.timestamp, timestamp);
    }

    #[test]
    fn rejects_a_record_with_an_invalid_checksum() {
        let temp_dir = TempDir::new();
        let file_path = temp_dir.path().join(Storage::INITIAL_FILE_NAME);
        File::create(&file_path).unwrap();
        let storage = Storage::initialize(config(temp_dir.path(), ONE_MB, 2 * ONE_MB)).unwrap();
        let payload = b"checksum-protected";
        storage.append(payload).unwrap();

        let mut file = OpenOptions::new().write(true).open(file_path).unwrap();
        file.seek(SeekFrom::Start(StorageInstance::HEADER_SIZE))
            .unwrap();
        file.write_all(&[payload[0] ^ 0xff]).unwrap();
        file.flush().unwrap();

        assert!(matches!(
            storage.read(0, 0, payload.len() as u64),
            Err(StorageError::Corupted(_, 0, size)) if size == payload.len() as u64
        ));
    }

    #[test]
    fn rolls_over_when_the_head_file_cannot_fit_a_record() {
        let temp_dir = TempDir::new();
        File::create(temp_dir.path().join(Storage::INITIAL_FILE_NAME)).unwrap();
        let storage = Storage::initialize(config(temp_dir.path(), 30, 200)).unwrap();
        storage.append(b"first").unwrap();

        storage.append(b"more").unwrap();

        assert_eq!(storage.files.count(), 2);
        assert!(temp_dir.path().join("db_file_00001").is_file());
    }

    #[test]
    fn concurrent_appends_and_reads_do_not_corrupt_records() {
        let temp_dir = TempDir::new();
        File::create(temp_dir.path().join(Storage::INITIAL_FILE_NAME)).unwrap();
        let storage =
            Arc::new(Storage::initialize(config(temp_dir.path(), 2 << 20, 2 << 20)).unwrap());
        let initial = b"stable";
        storage.append(initial).unwrap();

        const WORKERS: usize = 10;
        const WRITES_PER_WORKER: usize = 100;
        const PAYLOAD_SIZE: usize = 32;
        let barrier = Arc::new(Barrier::new(WORKERS));
        let workers: Vec<_> = (0..WORKERS)
            .map(|worker| {
                let storage = Arc::clone(&storage);
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    let mut payloads = Vec::new();
                    for idx in 0..WRITES_PER_WORKER {
                        let mut payload = [0; PAYLOAD_SIZE];
                        payload[..8].copy_from_slice(&(worker as u64).to_be_bytes());
                        payload[8..16].copy_from_slice(&(idx as u64).to_be_bytes());
                        payload[16..].fill(worker as u8);
                        payloads.push(payload);
                    }

                    barrier.wait();

                    for payload in &payloads {
                        storage.append(payload).unwrap();
                        let read = storage.read(0, 0, initial.len() as u64).unwrap();
                        assert_eq!(read.bytes, initial);
                    }
                    payloads
                })
            })
            .collect();

        let mut expected: Vec<_> = workers
            .into_iter()
            .flat_map(|worker| worker.join().unwrap().to_vec())
            .collect();
        expected.sort();

        let mut actual = Vec::with_capacity(WORKERS);
        for index in 0..(WORKERS * WRITES_PER_WORKER) {
            let offset = StorageInstance::HEADER_SIZE
                + initial.len() as u64
                + index as u64 * (StorageInstance::HEADER_SIZE + PAYLOAD_SIZE as u64);
            actual.push(storage.read(0, offset, PAYLOAD_SIZE as u64).unwrap().bytes);
        }
        actual.sort();

        assert_eq!(actual, expected);
    }

    #[test]
    fn concurrent_appends_and_reads_add_new_files_correctly() {
        let temp_dir = TempDir::new();
        File::create(temp_dir.path().join(Storage::INITIAL_FILE_NAME)).unwrap();
        let storage = Arc::new(Storage::initialize(config(temp_dir.path(), 128, 2 << 20)).unwrap());
        let initial = b"stable";
        storage.append(initial).unwrap();

        const WORKERS: usize = 1;
        const WRITES_PER_WORKER: usize = 100;
        const PAYLOAD_SIZE: usize = 32;
        let barrier = Arc::new(Barrier::new(WORKERS));
        let workers: Vec<_> = (0..WORKERS)
            .map(|worker| {
                let storage = Arc::clone(&storage);
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    let mut payloads = Vec::new();
                    for idx in 0..WRITES_PER_WORKER {
                        let mut payload = [0; PAYLOAD_SIZE];
                        payload[..8].copy_from_slice(&(worker as u64).to_be_bytes());
                        payload[8..16].copy_from_slice(&(idx as u64).to_be_bytes());
                        payload[16..].fill(worker as u8);
                        payloads.push(payload);
                    }

                    barrier.wait();

                    for payload in &payloads {
                        let AppendResponse {
                            file_index, offset, ..
                        } = storage.append(payload).unwrap();
                        let read = storage
                            .read(file_index, offset, PAYLOAD_SIZE as u64)
                            .unwrap();
                        assert_eq!(read.bytes, payload);
                    }
                })
            })
            .collect();

        for worker in workers {
            worker.join().unwrap();
        }

        println!("Files: {}", storage.files.count());
    }
}
