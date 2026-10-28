// The storage format is:
//
// Put:
// [4 bytes CRC32 checksum][1 byte Put code][8 bytes key length][8 bytes value length][key][value]
//
// Delete tombstone:
// [4 bytes CRC32 checksum][1 byte Delete code][8 bytes key length][key]
//
// All integers use big-endian encoding.

use crc32fast::Hasher;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use thiserror::Error;

use crate::storage::Operation::{Delete, Put};
use crate::storage::StorageError::*;

#[derive(Debug)]
pub struct WriteRequest<'a> {
    pub key: &'a str,
    pub val: &'a str,
}

#[derive(Debug)]
pub struct ReadRequest<'a> {
    pub key: &'a str,
}

#[derive(Debug)]
pub struct DeleteRequest<'a> {
    pub key: &'a str,
}

#[derive(Debug)]
pub struct ContainsRequest<'a> {
    pub key: &'a str,
}

#[derive(Debug)]
pub struct ReadResponse {
    #[allow(dead_code)]
    pub key: String,
    pub val: String,
}

#[derive(Error, Debug)]
pub enum StorageError {
    #[error("UTF8 error: {0}")]
    InvalidUTTF8(#[from] std::string::FromUtf8Error),
    #[error("IO error: {0}")]
    IOError(#[from] std::io::Error),
    #[error("Key {0} not found.")]
    MissingKey(String),
    #[error("Operation code {0} does not exist.")]
    InvalidOpCode(u8),
    #[error("{0}")]
    InvalidUtf8(String),
    #[error("File corrupted at key: {0}")]
    Corrupted(String),
    #[error("File corrupted at offset {0}. Truncating to last valid entry, {1:?} of key: {2}.")]
    Truncate(u64, Operation, String),
}

#[derive(Debug, PartialEq)]
pub enum Operation {
    Put,
    Delete,
}

impl Operation {
    pub const fn code(&self) -> u8 {
        match self {
            Put => 0,
            Delete => 1,
        }
    }

    pub fn decode(code: u8) -> Result<Operation, StorageError> {
        match code {
            0 => Ok(Put),
            1 => Ok(Delete),
            _ => Err(InvalidOpCode(code)),
        }
    }
}

#[derive(Debug)]
pub struct Storage {
    file: File,
    offsets: HashMap<String, u64>,
    next_offset: u64,
}

fn pack_write(request: &WriteRequest) -> Vec<u8> {
    let key_len = request.key.len();
    let val_len = request.val.len();

    let mut bytes: Vec<u8> = Vec::with_capacity(key_len + val_len + 21);
    bytes.extend_from_slice(&[0u8; 4]); // Save space for the checksum;
    bytes.push(Put.code());
    bytes.extend_from_slice(&(key_len as u64).to_be_bytes());
    bytes.extend_from_slice(request.key.as_bytes());
    bytes.extend_from_slice(&(val_len as u64).to_be_bytes());
    bytes.extend_from_slice(request.val.as_bytes());

    let checksum = &crc32fast::hash(&bytes[4..]).to_be_bytes();
    bytes[..4].copy_from_slice(checksum);

    bytes
}

fn pack_delete(request: &DeleteRequest) -> Vec<u8> {
    let key_len = request.key.len();

    let mut bytes: Vec<u8> = Vec::with_capacity(key_len + 13);
    bytes.extend_from_slice(&[0u8; 4]); // Save space for the checksum;
    bytes.push(Delete.code());
    bytes.extend_from_slice(&(key_len as u64).to_be_bytes());
    bytes.extend_from_slice(request.key.as_bytes());

    let checksum = &crc32fast::hash(&bytes[4..]).to_be_bytes();
    bytes[..4].copy_from_slice(checksum);

    bytes
}

fn parse_u32(reader: &mut impl Read) -> Result<u32, StorageError> {
    let buf: &mut [u8] = &mut [0; 4];
    reader.read_exact(buf)?;
    let len: [u8; 4] = buf.try_into().unwrap();
    Ok(u32::from_be_bytes(len))
}

fn parse_op(reader: &mut impl Read, hasher: &mut Hasher) -> Result<Operation, StorageError> {
    let buf: &mut [u8] = &mut [0; 1];
    reader.read_exact(buf)?;
    hasher.update(buf);
    Operation::decode(buf[0])
}

fn parse_u64(reader: &mut impl Read, hasher: &mut Hasher) -> Result<u64, StorageError> {
    let buf: &mut [u8] = &mut [0; 8];
    reader.read_exact(buf)?;
    hasher.update(buf);
    let len: [u8; 8] = buf.try_into().unwrap();
    Ok(u64::from_be_bytes(len))
}

fn parse_string(
    reader: &mut impl Read,
    len: u64,
    hasher: &mut Hasher,
) -> Result<String, StorageError> {
    // Consider explicitly handling the case when usize is 32 bits
    // to avoid silent truncation.
    let len = len as usize;
    let mut buf: Vec<u8> = vec![0; len];
    reader.read_exact(&mut buf)?;
    hasher.update(&buf);

    // Once checksums are implmented, consider doing an unchecked conversion
    // so we don't have to pay to check for valid utf8.
    Ok(String::from_utf8(buf)?)
}

impl Storage {
    fn parse_offsets(
        file: &mut File,
        file_length: u64,
    ) -> Result<(HashMap<String, u64>, u64), StorageError> {
        // Read u64 -> keyLen, read key_len to string, read val_len, increment past val
        let mut offsets = HashMap::new();
        let mut offset = 0;
        let mut last_valid_key: String = "".into();

        file.rewind()?;

        while offset < file_length {
            let entry_len;
            let mut hasher = Hasher::new();
            let crc = parse_u32(file)?;
            let op = parse_op(file, &mut hasher)?;
            let cur_key;

            match op {
                Put => {
                    let key_len = parse_u64(file, &mut hasher)?;
                    let key = parse_string(file, key_len, &mut hasher)?;
                    cur_key = key.clone();

                    let val_len = parse_u64(file, &mut hasher)?;

                    offsets.insert(key, offset);
                    entry_len = key_len + val_len + 17;
                    file.seek(std::io::SeekFrom::Start(offset))?;
                }
                Delete => {
                    let key_len = parse_u64(file, &mut hasher)?;
                    let key = parse_string(file, key_len, &mut hasher)?;
                    cur_key = key.clone();

                    offsets.remove(&key);
                    entry_len = key_len + 9;
                }
            }

            if hasher.finalize() != crc {
                file.set_len(offset)?;
                return Err(Truncate(offset, op, last_valid_key));
            }

            last_valid_key = cur_key;
            offset += entry_len;
        }

        Ok((offsets, offset))
    }

    pub fn initialize(file_path: &str) -> Result<Storage, StorageError> {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(file_path)?;

        let length = file.metadata()?.len();
        let (offsets, next_offset) = Storage::parse_offsets(&mut file, length)?;

        Ok(Storage {
            file,
            offsets,
            next_offset,
        })
    }

    pub fn quit(&mut self) -> Result<(), StorageError> {
        self.file.flush()?;
        self.file.sync_all().map_err(Into::into)
    }

    pub fn write(&mut self, request: &WriteRequest) -> std::io::Result<()> {
        let to_write = pack_write(request);
        self.file.seek(std::io::SeekFrom::Start(self.next_offset))?;
        self.file.write_all(&to_write)?;

        self.offsets
            .insert(request.key.to_string(), self.next_offset);
        self.next_offset += to_write.len() as u64;

        self.file.flush()?;
        Ok(())
    }

    pub fn delete(&mut self, request: &DeleteRequest) -> std::io::Result<()> {
        let to_write = pack_delete(request);
        self.file.seek(std::io::SeekFrom::Start(self.next_offset))?;
        self.file.write_all(&to_write)?;

        self.offsets.remove(request.key);
        self.next_offset += to_write.len() as u64;

        self.file.flush()?;
        Ok(())
    }

    pub fn read(&mut self, request: &ReadRequest) -> Result<ReadResponse, StorageError> {
        let offset = self.offsets.get(request.key);

        if offset.is_none() {
            return Err(MissingKey(request.key.into()));
        }
        let offset = offset.unwrap();

        self.file.seek(SeekFrom::Start(*offset))?;
        let crc = parse_u32(&mut self.file)?;
        let mut hasher = Hasher::new();

        let op = parse_op(&mut self.file, &mut hasher)?;
        if op != Put {
            return Err(InvalidUtf8(format!(
                "Offset at key {} does not correspond to a Put operation.",
                request.key
            )));
        }

        let key_len = parse_u64(&mut self.file, &mut hasher)?;
        let key = parse_string(&mut self.file, key_len, &mut hasher)?;

        let val_len = parse_u64(&mut self.file, &mut hasher)?;
        let val = parse_string(&mut self.file, val_len, &mut hasher)?;

        if hasher.finalize() != crc {
            return Err(Corrupted(request.key.into()));
        }

        Ok(ReadResponse { key, val })
    }

    pub fn contains(&self, request: &ContainsRequest) -> bool {
        self.offsets.contains_key(request.key)
    }

    pub fn keys(&self) -> Vec<String> {
        let mut keys: Vec<String> = self.offsets.keys().cloned().collect();
        keys.sort();
        keys
    }
}
