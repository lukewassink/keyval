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

struct StorageInstance {
    file: File,

    filename: String,

    capacity: u64,

    // Only used for writes.
    next_offset: Mutex<u64>,
}

pub struct ReadResponse {
    timestamp: u64,
    bytes: Vec<u8>,
}

pub struct AppendResponse {
    file_index: u32,
    offset: u64,
}

impl StorageInstance {
    const HEADER_SIZE: u64 = 12;

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
            capacity: config.file_size_mb * 2u64.pow(20),
            next_offset,
        })
    }

    fn would_exceed_capacity(&self, bytes: &[u8]) -> bool {
        bytes.len() as u64 + Self::HEADER_SIZE > self.capacity
    }

    fn append(&self, bytes: &[u8]) -> Result<u64, StorageError> {
        if self.would_exceed_capacity(bytes) {
            return Err(FileCapacityExceeded(
                self.filename.clone(),
                self.capacity,
                bytes.len(),
            ));
        }

        let mut next_offset = self.next_offset.lock().unwrap();

        self.file.write_all_at(
            &StorageInstance::checksum(bytes).to_be_bytes(),
            *next_offset,
        )?;

        let timestamp = StorageInstance::timestamp();
        self.file
            .write_all_at(&timestamp.to_be_bytes(), *next_offset + 4)?;
        self.file
            .write_all_at(bytes, *next_offset + Self::HEADER_SIZE)?;
        *next_offset += Self::HEADER_SIZE + bytes.len() as u64; // Swich next_offset to an atomic, and use a

        Ok(timestamp)
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
}

struct Storage {
    files: boxcar::Vec<StorageInstance>,
    config: Config,
    // In bytes.
    size: u64,
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

        let caps = regex!(r"^db_file_(?<number>)\d{5}$").captures(&filename)?;

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

    fn set_read_only(file: &File) -> Result<(), StorageError> {
        file.metadata()?.permissions().set_readonly(true);
        Ok(())
    }

    fn head_file(&self) -> Option<&StorageInstance> {
        self.files.get(self.files.count() - 1)
    }

    pub fn new(config: Config) -> Result<Self, StorageError> {
        let path = &config.database_path;
        if !fs::exists(path)? {
            fs::create_dir_all(path)?;
        }

        if !fs::metadata(path)?.is_dir() {
            return Err(InvalidDBPath(path.into()));
        }

        let mut num_to_file: BTreeMap<u32, StorageInstance> = BTreeMap::new();
        for file in fs::read_dir(path)? {
            let file = file?;

            if !file.metadata()?.is_file() {
                continue;
            }

            if let Some((filename, num)) = Self::extract_file_number(file.file_name()) {
                let file = Self::open_file(file.path().to_str().unwrap(), false)?;

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

        let max_bytes = config.max_db_size_mb * 2u64.pow(20);

        Ok(Storage {
            config,
            files,
            max_bytes,
            size: 0,
        })
    }

    pub fn append(&self, bytes: &[u8]) -> Result<AppendResponse, StorageError> {
        // Check if max db size would be exceeded.
        if bytes.len() as u64 + StorageInstance::HEADER_SIZE + self.size > self.max_bytes {
            return Err(StorageCapacityExceeded(self.max_bytes, bytes.len() as u64));
        }

        // Create a new file if current head file would overflow.
        if self.head_file().unwrap().would_exceed_capacity(bytes) {
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
        let offset = self.head_file().unwrap().append(bytes)?;
        Ok(AppendResponse {
            file_index: self.files.count() as u32,
            offset,
        })
    }

    pub fn read(
        &self,
        file_idx: u32,
        offset: u64,
        size: u64,
    ) -> Result<ReadResponse, StorageError> {
        if file_idx > self.files.count() as u32 {
            return Err(InvalidIndex(file_idx));
        }

        self.files
            .get(file_idx as usize)
            .unwrap()
            .read(offset, size)
    }
}
