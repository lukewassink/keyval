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
use derive_new::new;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use thiserror::Error;

use crate::storage::Operation::{Delete, Put};
use crate::storage::StorageError::*;

#[derive(Debug, new)]
pub struct WriteRequest<'a> {
    pub key: &'a [u8],
    pub val: &'a [u8],
}

#[derive(Debug, new)]
pub struct ReadRequest<'a> {
    pub key: &'a [u8],
}

#[derive(Debug, new)]
pub struct DeleteRequest<'a> {
    pub key: &'a [u8],
}

#[derive(Debug, new)]
pub struct ContainsRequest<'a> {
    pub key: &'a [u8],
}

#[derive(Debug, PartialEq, new)]
pub struct ReadResponse {
    #[allow(dead_code)]
    pub key: Vec<u8>,
    pub val: Vec<u8>,
}

#[derive(Error, Debug)]
pub enum StorageError {
    #[error("IO error: {0}")]
    IOError(#[from] std::io::Error),
    #[error("Key {0:?} not found.")]
    MissingKey(Vec<u8>),
    #[error("Operation code {0} does not exist.")]
    InvalidOpCode(u8),
    #[error("Corrupted data for key: {0:?}")]
    Corrupted(Vec<u8>),
    #[error("File corrupted at offset {0}. Truncating to last valid entry, {1:?} of key: {2:?}.")]
    Truncate(u64, Operation, Vec<u8>),
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
pub struct Storage<T> {
    file: T,
    keydir: HashMap<Vec<u8>, u64>,
    next_offset: u64,
}

fn pack_write(request: &WriteRequest) -> Vec<u8> {
    let key_len = request.key.len();
    let val_len = request.val.len();

    let mut bytes: Vec<u8> = Vec::with_capacity(key_len + val_len + 21);
    bytes.extend_from_slice(&[0u8; 4]); // Save space for the checksum;
    bytes.push(Put.code());
    bytes.extend_from_slice(&(key_len as u64).to_be_bytes());
    bytes.extend_from_slice(request.key);
    bytes.extend_from_slice(&(val_len as u64).to_be_bytes());
    bytes.extend_from_slice(request.val);

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
    bytes.extend_from_slice(request.key);

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

fn read_bytes(
    reader: &mut impl Read,
    len: u64,
    hasher: &mut Hasher,
) -> Result<Vec<u8>, StorageError> {
    // Consider explicitly handling the case when usize is 32 bits
    // to avoid silent truncation.
    let len = len as usize;
    let mut buf: Vec<u8> = vec![0; len];
    reader.read_exact(&mut buf)?;
    hasher.update(&buf);
    Ok(buf)
}

impl<T> Storage<T>
where
    T: Read + Write + Seek,
{
    fn parse_keydir(
        file: &mut T,
        file_length: u64,
    ) -> Result<(HashMap<Vec<u8>, u64>, u64), StorageError> {
        // Read u64 -> keyLen, read key_len to string, read val_len, increment past val
        let mut offsets = HashMap::new();
        let mut offset = 0;
        let mut last_valid_key = vec![0; 0];

        file.rewind()?;

        while offset < file_length {
            let mut hasher = Hasher::new();
            let crc = parse_u32(file)?;
            let op = parse_op(file, &mut hasher)?;
            let cur_key;

            let entry_len = match op {
                Put => {
                    let key_len = parse_u64(file, &mut hasher)?;
                    let key = read_bytes(file, key_len, &mut hasher)?;
                    cur_key = key.clone();

                    let val_len = parse_u64(file, &mut hasher)?;
                    let _ = read_bytes(file, val_len, &mut hasher)?;

                    offsets.insert(key, offset);
                    key_len + val_len + 21
                }
                Delete => {
                    let key_len = parse_u64(file, &mut hasher)?;
                    let key = read_bytes(file, key_len, &mut hasher)?;
                    cur_key = key.clone();

                    offsets.remove(&key);
                    key_len + 13
                }
            };
            println!("Op: {:?}", op);

            if hasher.finalize() != crc {
                return Err(Truncate(offset, op, last_valid_key));
            }

            last_valid_key = cur_key;
            offset += entry_len;
        }

        Ok((offsets, offset))
    }

    pub fn initialize(file_path: &str) -> Result<Storage<File>, StorageError> {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(file_path)?;

        let length = file.metadata()?.len();
        let keydir;
        let next_offset;

        match Storage::<File>::parse_keydir(&mut file, length) {
            Ok(output) => {
                (keydir, next_offset) = output;
            }
            Err(e @ Truncate(offset, _, _)) => {
                file.set_len(offset)?;
                return Err(e);
            }
            Err(e) => {
                return Err(e);
            }
        }

        Ok(Storage {
            file,
            keydir,
            next_offset,
        })
    }

    pub fn write(&mut self, request: &WriteRequest) -> std::io::Result<()> {
        let to_write = pack_write(request);
        self.file.seek(std::io::SeekFrom::Start(self.next_offset))?;
        self.file.write_all(&to_write)?;

        self.keydir.insert(request.key.into(), self.next_offset);
        self.next_offset += to_write.len() as u64;

        self.file.flush()?;
        Ok(())
    }

    pub fn delete(&mut self, request: &DeleteRequest) -> std::io::Result<()> {
        let to_write = pack_delete(request);
        self.file.seek(std::io::SeekFrom::Start(self.next_offset))?;
        self.file.write_all(&to_write)?;

        self.keydir.remove(request.key);
        self.next_offset += to_write.len() as u64;

        self.file.flush()?;
        Ok(())
    }

    pub fn read(&mut self, request: &ReadRequest) -> Result<ReadResponse, StorageError> {
        let offset = self.keydir.get(request.key);

        if offset.is_none() {
            return Err(MissingKey(request.key.into()));
        }
        let offset = offset.unwrap();

        self.file.seek(SeekFrom::Start(*offset))?;
        let crc = parse_u32(&mut self.file)?;
        let mut hasher = Hasher::new();

        let op = parse_op(&mut self.file, &mut hasher)?;
        if op != Put {
            return Err(Corrupted(request.key.into()));
        }

        let key_len = parse_u64(&mut self.file, &mut hasher)?;
        let key = read_bytes(&mut self.file, key_len, &mut hasher)?;

        let val_len = parse_u64(&mut self.file, &mut hasher)?;
        let val = read_bytes(&mut self.file, val_len, &mut hasher)?;

        if hasher.finalize() != crc {
            return Err(Corrupted(request.key.into()));
        }

        Ok(ReadResponse { key, val })
    }

    pub fn contains(&self, request: &ContainsRequest) -> bool {
        self.keydir.contains_key(request.key)
    }

    pub fn keys(&self) -> Vec<Vec<u8>> {
        let mut keys: Vec<Vec<u8>> = self.keydir.keys().cloned().collect();
        keys.sort();
        keys
    }
}

impl Storage<File> {
    pub fn quit(&mut self) -> Result<(), StorageError> {
        self.file.flush()?;
        self.file.sync_all().map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn read_write() -> Result<(), StorageError> {
        let mut storage = Storage {
            file: Cursor::new(vec![0u8; 0]),
            keydir: HashMap::new(),
            next_offset: 0,
        };
        let response = ReadResponse {
            key: vec![0, 1, 2],
            val: vec![0, 4, 4, 2],
        };
        let write_request = WriteRequest {
            key: &response.key,
            val: &response.val,
        };
        let read_request = ReadRequest {
            key: write_request.key,
        };

        storage.write(&write_request)?;

        assert!(storage.read(&read_request)? == response);
        Ok(())
    }

    #[test]
    fn multiple_ops() -> Result<(), StorageError> {
        let mut storage = Storage {
            file: Cursor::new(vec![0u8; 0]),
            keydir: HashMap::new(),
            next_offset: 0,
        };
        let key1 = "a".as_bytes();
        let key2 = "b".as_bytes();
        let key3 = "c".as_bytes();
        let val1 = "xyz".as_bytes();
        let val2 = "qwe".as_bytes();
        let val3 = "ttt".as_bytes();
        let val4 = "01234".as_bytes();

        storage.write(&WriteRequest::new(key1, val1))?;
        storage.write(&WriteRequest::new(key2, val2))?;
        storage.write(&WriteRequest::new(key3, val3))?;
        storage.write(&WriteRequest::new(key1, val4))?;
        storage.delete(&DeleteRequest::new(key2))?;

        assert!(storage.contains(&ContainsRequest::new(key1)));
        assert!(storage.contains(&ContainsRequest::new(key3)));
        assert!(!storage.contains(&ContainsRequest::new(key2)));

        assert!(
            storage.read(&ReadRequest::new(key1))? == ReadResponse::new(key1.into(), val4.into())
        );
        assert!(
            storage.read(&ReadRequest::new(key3))? == ReadResponse::new(key3.into(), val3.into())
        );
        Ok(())
    }

    #[test]
    fn parse_keydir() -> Result<(), StorageError> {
        let file = vec![0u8; 0];
        let mut storage = Storage {
            file: Cursor::new(file),
            keydir: HashMap::new(),
            next_offset: 0,
        };

        let key1 = "a".as_bytes();
        let key2 = "b".as_bytes();
        let key3 = "c".as_bytes();
        let val1 = "xyz".as_bytes();
        let val2 = "qwe".as_bytes();
        let val3 = "ttt".as_bytes();
        let val4 = "01234".as_bytes();

        storage.write(&WriteRequest::new(key1, val1))?;
        storage.write(&WriteRequest::new(key2, val2))?;
        storage.write(&WriteRequest::new(key3, val3))?;
        storage.write(&WriteRequest::new(key1, val4))?;
        storage.delete(&DeleteRequest::new(key2))?;

        println!("{:?}", storage.file);

        let len = storage.file.get_ref().len() as u64;
        let (parsed_key_dir, _) = Storage::parse_keydir(&mut storage.file, len)?;

        assert!(parsed_key_dir == storage.keydir);
        Ok(())
    }
}
