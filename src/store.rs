use std::io;

use dashmap::DashMap;
use regex::bytes;
use rkyv::{Archive, Deserialize, Serialize, rancor};
use thiserror::Error;

use crate::storage_new::{AppendResponse, Storage, StorageError, StorageInstance};

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("IO error: {0}")]
    IO(#[from] io::Error),
    #[error("Storage error: {0}")]
    Storage(#[from] StorageError),
    #[error("Error deserializing stored entry: {0}")]
    Deserialization(#[from] rancor::Error),
}

pub struct PutRequest {
    key: Vec<u8>,
    val: Vec<u8>,
}

pub struct GetRequest {
    key: Vec<u8>,
}

pub struct DeleteRequest {
    key: Vec<u8>,
}

pub struct ContainsRequest {
    key: Vec<u8>,
}

pub enum Request {
    Put(PutRequest),
    Get(GetRequest),
    Delete(DeleteRequest),
    Contains(ContainsRequest),
}

pub struct PutResponse {
    timestamp: u64,
}

pub struct GetResponse {
    val: Vec<u8>,
    timestamp: u64,
}

pub struct DeleteResponse {
    timestamp: u64,
}

pub struct ContainsResponse {
    contains: bool,
    timestamp: u64,
}

pub enum Response {
    Put(PutResponse),
    Get(GetResponse),
    Delete(DeleteResponse),
    Contains(ContainsResponse),
}

#[derive(Archive, Serialize, Deserialize, Debug, PartialEq)]
#[rkyv(compare(PartialEq), derive(Debug))]
#[repr(u8)]
enum StorageFormat {
    Put { key: Vec<u8>, val: Vec<u8> },
    Delete { key: Vec<u8> },
}

struct Location {
    file_index: u32,
    offset: u64,
    size: u32,
}

pub struct Store {
    storage: Storage,
    keydir: DashMap<Vec<u8>, Location>,
}

impl Store {
    fn build_keydir(storage: &Storage) -> Result<DashMap<Vec<u8>, Location>, StoreError> {
        let keydir = DashMap::new();

        for (idx, instance) in storage.files.iter() {
            let file_size = instance.len()?;
            let mut offset = 0;

            while offset < file_size {
                let response = instance.read_at(offset)?;

                let entry = rkyv::access::<ArchivedStorageFormat, rancor::Error>(&response.bytes)?;

                match entry {
                    ArchivedStorageFormat::Put { key, .. } => {
                        keydir.insert(
                            key.to_vec(),
                            Location {
                                file_index: idx as u32,
                                offset,
                                size: response.bytes.len() as u32,
                            },
                        );
                    }
                    ArchivedStorageFormat::Delete { key } => {
                        keydir.remove(&key.to_vec());
                    }
                }

                offset += StorageInstance::append_len(&response.bytes);
            }
        }

        return Ok(keydir);
    }

    pub fn initialize(storage: Storage) -> Result<Self, StoreError> {
        let keydir = Self::build_keydir(&storage)?;
        Ok(Self { storage, keydir })
    }

    pub fn put(&self, request: PutRequest) -> Result<PutResponse, StoreError> {
        let bytes = rkyv::to_bytes::<rancor::Error>(&StorageFormat::Put {
            key: request.key.clone(),
            val: request.val,
        })?;
        let AppendResponse {
            file_index,
            offset,
            timestamp,
        } = self.storage.append(&bytes)?;

        let location = Location {
            file_index,
            offset,
            size: bytes.len() as u32,
        };
        self.keydir.insert(request.key, location);

        Ok(PutResponse { timestamp })
    }
}
