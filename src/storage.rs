use std::fs;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
use std::{fs::File, os::unix::fs::FileExt, time::SystemTime};
use thiserror::Error;

use crate::config::Config;
use crate::file_utils::{self, create_data_file, get_files};
use crate::storage::StorageError::{
    Corupted, FileCapacityExceeded, InvalidDBPath, InvalidFileNumber, StorageCapacityExceeded,
};

#[derive(Error, Debug)]
pub enum StorageError {
    #[error("IO error: {0}")]
    IOError(#[from] std::io::Error),
    #[error("Checksum for file number {0} at offset {1} with length {2} is not valid.")]
    Corupted(u32, u64, u64),
    #[error("File number {0} with capacity {1}MB would be overfilled by write of {2} bytes.")]
    FileCapacityExceeded(u32, u64, usize),
    #[error("Database capacity of {0} would be exceeded by write of size {1}.")]
    StorageCapacityExceeded(u64, u64),
    #[error("The item at the path {0} is not a directory.")]
    InvalidDBPath(String),
    #[error("The file number {0} does not correspond to any database file.")]
    InvalidFileNumber(u32),
}

#[derive(Debug)]
pub struct StorageInstance {
    // Use Rc so StorageInstance can be cloned.
    file: File,

    file_no: u32,

    capacity: u64,

    // Only used for writes.
    next_offset: Mutex<u64>,
}

pub struct ReadResponse {
    pub timestamp: u64,
    pub bytes: Vec<u8>,
}

pub struct AppendResponse {
    pub file_no: u32,
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

    fn new(file: File, file_no: u32, config: &Config) -> Result<Self, StorageError> {
        let next_offset = Mutex::new(file.metadata()?.len());
        Ok(StorageInstance {
            file,
            file_no,
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
                self.file_no,
                self.capacity,
                bytes.len(),
            ));
        }

        let mut next_offset = self.next_offset.lock().unwrap();
        let cur_offset = *next_offset;
        let timestamp = StorageInstance::timestamp();

        // Checksum
        self.file
            .write_all_at(&crc32fast::hash(bytes).to_be_bytes(), cur_offset)?;
        // Timestamp
        self.file
            .write_all_at(&timestamp.to_be_bytes(), cur_offset + 4)?;
        // Size
        self.file
            .write_all_at(&(bytes.len() as u32).to_be_bytes(), cur_offset + 12)?;
        // Bytes
        self.file
            .write_all_at(bytes, *next_offset + Self::HEADER_SIZE)?;

        *next_offset += Self::HEADER_SIZE + bytes.len() as u64;
        Ok((cur_offset, timestamp))
    }

    fn set_read_only(&self) -> Result<(), StorageError> {
        self.file.metadata()?.permissions().set_readonly(true);
        Ok(())
    }

    fn read(&self, offset: u64, size: u64) -> Result<ReadResponse, StorageError> {
        let mut buffer: Vec<u8> = vec![0; (size + Self::HEADER_SIZE) as usize];

        self.file.read_exact_at(&mut buffer, offset)?;

        let checksum = file_utils::parse_u32(&buffer, 0);
        if checksum != crc32fast::hash(&buffer[Self::HEADER_SIZE as usize..]) {
            return Err(Corupted(self.file_no, offset, size));
        }

        let timestamp = file_utils::parse_u64(&buffer, 4);

        Ok(ReadResponse {
            bytes: buffer[Self::HEADER_SIZE as usize..].into(),
            timestamp,
        })
    }

    // Uses the encoded size rather than reading the size from the keydir.
    pub fn read_at(&self, offset: u64) -> Result<ReadResponse, StorageError> {
        let mut header: Vec<u8> = vec![0; Self::HEADER_SIZE as usize];
        self.file.read_exact_at(&mut header, offset)?;

        let size = file_utils::parse_u32(&header, 12);
        let mut bytes: Vec<u8> = vec![0; size as usize];
        self.file
            .read_exact_at(&mut bytes, offset + Self::HEADER_SIZE)?;

        let checksum = file_utils::parse_u32(&header, 0);
        if checksum != crc32fast::hash(&bytes) {
            return Err(Corupted(self.file_no, offset, size as u64));
        }

        let timestamp = file_utils::parse_u64(&header, 4);

        Ok(ReadResponse { bytes, timestamp })
    }

    fn flush(&self) -> Result<(), StorageError> {
        self.file.sync_all().map_err(Into::into)
    }

    pub fn len(&self) -> Result<u64, StorageError> {
        Ok(self.file.metadata()?.len())
    }
}

pub struct Storage {
    pub files: papaya::HashMap<u32, StorageInstance>,
    config: Config,
    size: Mutex<u64>,
    active_file_no: AtomicU32,
}

impl Storage {
    pub fn initialize(config: Config) -> Result<Self, StorageError> {
        let path = &config.database_path;
        if !fs::exists(path)? {
            fs::create_dir_all(path)?;
        }

        if !fs::metadata(path)?.is_dir() {
            return Err(InvalidDBPath(path.into()));
        }

        let mut files = get_files(path)?;
        if files.is_empty() {
            files.insert(0, create_data_file(path, 0)?);
        }

        let size = files.values().map(|f| f.metadata().unwrap().len()).sum();
        let files: papaya::HashMap<u32, StorageInstance> = files
            .into_iter()
            .map(|(id, file)| (id, StorageInstance::new(file, id, &config).unwrap()))
            .collect();
        let active_file_no = AtomicU32::new(*files.pin().keys().max().unwrap());

        Ok(Storage {
            config,
            files,
            size: Mutex::new(size),
            active_file_no,
        })
    }

    pub fn append(&self, bytes: &[u8]) -> Result<AppendResponse, StorageError> {
        // Check if max db size would be exceeded.
        let mut size = self.size.lock().unwrap();
        if StorageInstance::append_len(bytes) + *size > self.config.max_db_size {
            return Err(StorageCapacityExceeded(
                self.config.max_db_size,
                bytes.len() as u64,
            ));
        }

        // Get active instance.
        let files = self.files.pin();
        let mut active_instance = files
            .get(&self.active_file_no.load(Ordering::Relaxed))
            .unwrap();
        let mut updated = false;

        // Create a new instance if it would overflow.
        if active_instance.would_exceed_capacity(bytes) {
            updated = true;
            active_instance.flush()?;
            active_instance.set_read_only()?;

            let file_no = self.active_file_no.fetch_add(1, Ordering::Relaxed) + 1;
            let new_active_file = create_data_file(&self.config.database_path, file_no)?;

            self.files.pin().insert(
                file_no,
                StorageInstance::new(new_active_file, file_no, &self.config)?,
            );
        }

        if updated {
            active_instance = files
                .get(&self.active_file_no.load(Ordering::Relaxed))
                .unwrap();
        }

        // Write the bytes.
        let (offset, timestamp) = active_instance.append(bytes)?;
        *size += StorageInstance::append_len(bytes);
        Ok(AppendResponse {
            file_no: active_instance.file_no,
            offset,
            timestamp,
        })
    }

    pub fn read(&self, file_no: u32, offset: u64, size: u64) -> Result<ReadResponse, StorageError> {
        if !self.files.pin().contains_key(&file_no) {
            return Err(InvalidFileNumber(file_no));
        }

        self.files.pin().get(&file_no).unwrap().read(offset, size)
    }
}

#[cfg(test)]
pub(crate) mod tests {
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

    pub(crate) struct TempDir(PathBuf);

    impl TempDir {
        pub(crate) fn new() -> Self {
            let id = NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("keyval-storage-new-{}-{id}", std::process::id()));
            fs::create_dir(&path).unwrap();
            Self(path)
        }

        pub(crate) fn path(&self) -> &Path {
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

    pub(crate) fn config(database_path: &Path, file_size: u64, max_db_size: u64) -> Config {
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

        assert_eq!(fs::read_dir(".").iter().count(), 0);

        let storage = Storage::initialize(config).unwrap();

        assert_eq!(storage.files.pin().keys().count(), 1);
        assert_eq!(fs::read_dir(".").iter().count(), 1);
    }

    #[test]
    fn initializes_from_numbered_files_with_gaps_and_ignores_other_files() {
        let temp_dir = TempDir::new();
        for filename in ["db_file_00000", "db_file_00002", "notes.txt"] {
            File::create(temp_dir.path().join(filename)).unwrap();
        }

        let storage = Storage::initialize(config(temp_dir.path(), 1, 4)).unwrap();

        assert_eq!(storage.files.pin().keys().count(), 2);
    }

    #[test]
    fn reads_and_writes_valid_data() {
        let temp_dir = TempDir::new();
        create_data_file(temp_dir.path().to_str().unwrap(), 0).unwrap();
        let storage = Storage::initialize(config(temp_dir.path(), 1024, 2048)).unwrap();
        let payload = b"append-only value";

        let append1 = storage.append(payload).unwrap();
        let read1 = storage.read(0, 0, payload.len() as u64).unwrap();
        let AppendResponse {
            file_no: file_index,
            offset,
            timestamp,
        } = storage.append(payload).unwrap();
        let read2 = storage
            .read(file_index, offset, payload.len() as u64)
            .unwrap();

        assert_eq!(read1.bytes, payload);
        assert_eq!(read1.timestamp, append1.timestamp);
        assert_eq!(append1.file_no, 0);
        assert_eq!(append1.offset, 0);
        assert_eq!(read2.bytes, payload);
        assert_eq!(read2.timestamp, timestamp);
    }

    #[test]
    fn rejects_a_record_with_an_invalid_checksum() {
        let temp_dir = TempDir::new();
        let mut file = create_data_file(temp_dir.path().to_str().unwrap(), 0).unwrap();
        let storage = Storage::initialize(config(temp_dir.path(), ONE_MB, 2 * ONE_MB)).unwrap();
        let payload = b"checksum-protected";
        storage.append(payload).unwrap();

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
        create_data_file(temp_dir.path().to_str().unwrap(), 0).unwrap();
        let storage = Storage::initialize(config(temp_dir.path(), 30, 200)).unwrap();
        storage.append(b"first").unwrap();

        storage.append(b"more").unwrap();

        assert_eq!(storage.files.pin().keys().count(), 2);
        assert!(temp_dir.path().join("db_file_00001").is_file());
    }

    #[test]
    fn concurrent_appends_and_reads_do_not_corrupt_records() {
        let temp_dir = TempDir::new();
        create_data_file(temp_dir.path().to_str().unwrap(), 0).unwrap();
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
        create_data_file(temp_dir.path().to_str().unwrap(), 0).unwrap();
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
                            file_no: file_index,
                            offset,
                            ..
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

        println!("Files: {}", storage.files.pin().keys().count());
    }
}
