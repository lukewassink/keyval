use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};

use crate::storage::Operation::{Delete, Put};
use crate::storage::StorageError::{Corrupted, IOError, MissingKey};

pub struct WriteRequest<'a> {
    pub key: &'a str,
    pub val: &'a str,
}

pub struct ReadRequest<'a> {
    pub key: &'a str,
}

pub struct DeleteRequest<'a> {
    pub key: &'a str,
}

pub struct ContainsRequest<'a> {
    pub key: &'a str,
}

#[derive(Debug)]
pub struct ReadResponse {
    pub key: String,
    pub val: String,
}

#[derive(Debug)]
pub enum StorageError {
    IOError(std::io::Error),
    MissingKey(String),
    Corrupted(String),
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
            _ => Err(Corrupted(format!(
                "Invalid code {} does not match any operation.",
                code
            ))),
        }
    }
}

pub struct Storage {
    file_path: String,
    file: File,
    offsets: HashMap<String, u64>,
    next_offset: u64,
}

fn pack_write(request: &WriteRequest) -> Vec<u8> {
    let key_len = request.key.len();
    let val_len = request.val.len();

    let mut bytes: Vec<u8> = Vec::with_capacity(key_len + val_len + 17);
    bytes.push(Put.code());
    bytes.extend_from_slice(&(key_len as u64).to_be_bytes());
    bytes.extend_from_slice(request.key.as_bytes());
    bytes.extend_from_slice(&(val_len as u64).to_be_bytes());
    bytes.extend_from_slice(request.val.as_bytes());

    bytes
}

fn pack_delete(request: &DeleteRequest) -> Vec<u8> {
    let key_len = request.key.len();

    let mut bytes: Vec<u8> = Vec::with_capacity(key_len + 9);
    bytes.push(Delete.code());
    bytes.extend_from_slice(&(key_len as u64).to_be_bytes());
    bytes.extend_from_slice(request.key.as_bytes());

    bytes
}

fn parse_op(reader: &mut impl Read) -> Result<Operation, StorageError> {
    let buf: &mut [u8] = &mut [0; 1];
    reader.read_exact(buf).map_err(IOError)?;
    Operation::decode(buf[0])
}

fn parse_long(reader: &mut impl Read) -> Result<u64, StorageError> {
    let buf: &mut [u8] = &mut [0; 8];
    reader.read_exact(buf).map_err(IOError)?;
    let len: [u8; 8] = buf.try_into().unwrap();
    Ok(u64::from_be_bytes(len))
}

fn parse_string(reader: &mut impl Read, len: u64) -> Result<String, StorageError> {
    // Consider explicitly handling the case when usize is 32 bits
    // to avoid silent truncation.
    let len = len as usize;
    let mut buf: Vec<u8> = vec![0; len];
    reader.read_exact(&mut buf).map_err(IOError)?;

    // Once checksums are implmented, consider doing an unchecked conversion
    // so we don't have to pay to check for valid utf8.
    String::from_utf8(buf)
        .map_err(|e| Corrupted(format!("Could not parse string as valid UTF8: {}", e)))
}

impl Storage {
    pub fn print_offsets(&self) -> String {
        format!("{:?}", self.offsets)
    }

    fn parse_offsets(
        reader: &mut (impl Read + Seek),
        file_length: u64,
    ) -> Result<(HashMap<String, u64>, u64), StorageError> {
        // Read u64 -> keyLen, read key_len to string, read val_len, increment past val
        let mut offsets = HashMap::new();
        let mut offset = 0;

        reader.rewind().map_err(IOError)?;
        while offset < file_length {
            let op = parse_op(reader)?;

            match op {
                Put => {
                    let key_len = parse_long(reader)?;
                    let key = parse_string(reader, key_len)?;

                    let val_len = parse_long(reader)?;

                    offsets.insert(key, offset);
                    offset += key_len + val_len + 17;
                    reader
                        .seek(std::io::SeekFrom::Start(offset))
                        .map_err(IOError)?;
                }
                Delete => {
                    let key_len = parse_long(reader)?;
                    let key = parse_string(reader, key_len)?;

                    offsets.remove(&key);
                    offset += key_len + 9;
                }
            }
        }

        Ok((offsets, offset))
    }

    pub fn initialize(file_path: &str) -> Result<Storage, StorageError> {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(file_path)
            .map_err(IOError)?;

        let length = file.metadata().map_err(IOError)?.len();
        let (offsets, next_offset) = Storage::parse_offsets(&mut file, length)?;

        Ok(Storage {
            file_path: file_path.to_string(),
            file,
            offsets,
            next_offset,
        })
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
        if let Some(offset) = offset {
            self.file.seek(SeekFrom::Start(*offset)).map_err(IOError)?;
            let op = parse_op(&mut self.file)?;
            if op != Put {
                return Err(Corrupted(format!(
                    "Offset at key {} does not correspond to a Put operation.",
                    request.key
                )));
            }

            let key_len = parse_long(&mut self.file)?;
            let key = parse_string(&mut self.file, key_len)?;

            let val_len = parse_long(&mut self.file)?;
            let val = parse_string(&mut self.file, val_len)?;

            Ok(ReadResponse { key, val })
        } else {
            Err(MissingKey(format!("Key not present: {}", request.key)))
        }
    }

    pub fn contains(&self, request: &ContainsRequest) -> bool {
        self.offsets.contains_key(request.key)
    }
}
