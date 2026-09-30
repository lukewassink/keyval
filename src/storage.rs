use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};

use crate::storage::StorageError::{Corrupted, IOError, MissingKey};

struct WriteRequest {
    key: String,
    val: String,
}

struct WriteResponse {}

struct ReadRequest {
    key: String,
}

enum StorageError {
    IOError(std::io::Error),
    MissingKey(String),
    Corrupted(String),
}

struct ReadResponse {
    key: String,
    val: String,
}

struct Storage {
    file_path: String,
    file: File,
    offsets: HashMap<String, u64>,
    next_offset: u64,
}

impl Storage {
    fn pack(request: &WriteRequest) -> Vec<u8> {
        // [8 bytes keyLen][kenLen bytes key][8 bytes valLen][valLen val]
        // 0 - 8 - (8 + keyLen) - (16 + keyLen) - (16 + keyLen + valLen)
        let key_len = request.key.len();
        let val_len = request.val.len();

        let mut bytes: Vec<u8> = Vec::with_capacity(key_len + val_len + 16);
        bytes.extend_from_slice(&(key_len as u64).to_be_bytes());
        bytes.extend_from_slice(request.key.as_bytes());
        bytes.extend_from_slice(&(val_len as u64).to_be_bytes());
        bytes.extend_from_slice(request.val.as_bytes());

        bytes
    }

    fn read_long(reader: &mut impl Read) -> Result<u64, StorageError> {
        let buf: &mut [u8] = &mut [0; 8];
        reader.read_exact(buf).map_err(IOError)?;
        let len: [u8; 8] = buf.try_into().unwrap();
        Ok(u64::from_be_bytes(len))
    }

    fn read_string(reader: &mut impl Read, len: u64) -> Result<String, StorageError> {
        // Consider explicitly handling the case when usize is 32 bits to avoid silent truncation.
        let len = len as usize;
        let mut buf: Vec<u8> = Vec::with_capacity(len);
        reader.read_exact(&mut buf).map_err(IOError)?;
        // Once checksums are implmented, consider doing an unchecked conversion
        // so we don't have to pay to check for valid utf8.
        String::from_utf8(buf)
            .map_err(|e| Corrupted(format!("Could not parse string as valid UTF8: {}", e)))
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
            let key_len = Storage::read_long(reader)?;
            let key = Storage::read_string(reader, key_len)?;

            let val_len = Storage::read_long(reader)?;

            offsets.insert(key, offset);
            offset += key_len + val_len + 16;
            reader
                .seek(std::io::SeekFrom::Start(offset))
                .map_err(IOError)?;
        }

        Ok((offsets, offset))
    }

    fn initialize(file_path: String) -> Result<Storage, StorageError> {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&file_path)
            .map_err(IOError)?;

        let length = file.metadata().map_err(IOError)?.len();
        let (offsets, next_offset) = Storage::parse_offsets(&mut file, length)?;

        Ok(Storage {
            file_path,
            file,
            offsets,
            next_offset,
        })
    }

    fn write(&mut self, request: &WriteRequest) -> std::io::Result<()> {
        let to_write = Storage::pack(request);
        self.file.seek(std::io::SeekFrom::Start(self.next_offset))?;
        self.file.write_all(&to_write)?;

        self.offsets.insert(request.key.clone(), self.next_offset);
        self.next_offset += (16 + request.key.len() + request.val.len()) as u64;

        self.file.flush()?;
        Ok(())
    }

    fn read(&mut self, request: &ReadRequest) -> Result<ReadResponse, StorageError> {
        let offset = self.offsets.get(&request.key);
        if let Some(offset) = offset {
            self.file.seek(SeekFrom::Start(*offset)).map_err(IOError)?;

            let key_len = Storage::read_long(&mut self.file)?;
            let key = Storage::read_string(&mut self.file, key_len)?;

            let val_len = Storage::read_long(&mut self.file)?;
            let val = Storage::read_string(&mut self.file, val_len)?;

            Ok(ReadResponse { key, val })
        } else {
            Err(MissingKey(format!("Key not present: {}", request.key)))
        }
    }
}
