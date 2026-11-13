use std::{
    io,
    sync::atomic::{AtomicU64, Ordering},
};

use clap::error::Result;
use dashmap::DashMap;
use rkyv::{Archive, Deserialize, Serialize, rancor};
use thiserror::Error;

use crate::{
    storage::{AppendResponse, Storage, StorageError, StorageInstance},
    store::StoreError::*,
};

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("{0}")]
    IO(#[from] io::Error),
    #[error("{0}")]
    StorageLayer(#[from] StorageError),
    #[error("Error deserializing stored entry: {0}")]
    Deserialization(#[from] rancor::Error),
    #[error("Key {0:?} was not found")]
    KeyNotFound(Vec<u8>),
    #[error("The entry for key {0:?} at location {1:?} is not StorageFormat::Put.")]
    InvalidGetLocation(Vec<u8>, Location),
}

#[derive(Debug)]
pub struct PutRequest {
    pub key: Vec<u8>,
    pub val: Vec<u8>,
}

#[derive(Debug)]
pub struct GetRequest {
    pub key: Vec<u8>,
}

#[derive(Debug)]
pub struct DeleteRequest {
    pub key: Vec<u8>,
}

#[derive(Debug)]
pub struct ContainsRequest {
    pub key: Vec<u8>,
}

#[derive(Debug)]
pub struct PutResponse {
    pub timestamp: u64,
}

#[derive(Debug)]
pub struct GetResponse {
    pub val: Vec<u8>,
    pub timestamp: u64,
}

#[derive(Debug)]
pub struct DeleteResponse {
    pub timestamp: u64,
}

#[derive(Debug)]
pub struct ContainsResponse {
    pub contains: bool,
}

#[derive(Archive, Serialize, Deserialize, Debug, PartialEq)]
#[rkyv(compare(PartialEq), derive(Debug))]
#[repr(u8)]
enum StorageFormat {
    Put { key: Vec<u8>, val: Vec<u8> },
    Delete { key: Vec<u8> },
}

#[derive(Debug, Clone)]
pub struct Location {
    file_no: u32,
    offset: u64,
    size: u32,
}

pub struct Store {
    storage: Storage,
    keydir: DashMap<Vec<u8>, Location>,
    total_bytes: AtomicU64,
    dead_bytes: AtomicU64,
}

type KeyDir = DashMap<Vec<u8>, Location>;

impl Store {
    fn insert_key(&self, key: Vec<u8>, location: Location) {
        if self.keydir.contains_key(&key) {
            let old_location = self.keydir.get(&key).unwrap();
            self.dead_bytes
                .fetch_add(old_location.size as u64, Ordering::Relaxed);
        }

        self.total_bytes
            .fetch_add(location.size as u64, Ordering::Relaxed);
        self.keydir.insert(key, location);
    }

    fn delete_key(&self, key: Vec<u8>, size: u64) {
        if let Some(old_location) = self.keydir.remove(&key) {
            self.dead_bytes
                .fetch_add(old_location.1.size as u64, Ordering::Relaxed);
        } else {
            self.dead_bytes.fetch_add(size, Ordering::Relaxed);
        }
        self.total_bytes.fetch_add(size, Ordering::Relaxed);
    }

    fn build_keydir(&self) -> Result<(), StoreError> {
        let guard = self.storage.files.guard();
        let mut files: Vec<(&u32, &StorageInstance)> = self.storage.files.iter(&guard).collect();
        files.sort_by_key(|(k, _v)| *k);

        for (file_no, instance) in files {
            let file_size = instance.len()?;
            let mut offset = StorageInstance::HEADER_SIZE;

            while offset < file_size {
                let response = instance.read_at(offset)?;
                let entry_len = StorageInstance::append_len(&response.bytes);

                let entry = rkyv::access::<ArchivedStorageFormat, rancor::Error>(&response.bytes)?;

                match entry {
                    ArchivedStorageFormat::Put { key, .. } => {
                        self.insert_key(
                            key.to_vec(),
                            Location {
                                file_no: *file_no,
                                offset,
                                size: response.bytes.len() as u32,
                            },
                        );
                    }
                    ArchivedStorageFormat::Delete { key } => {
                        self.delete_key(key.to_vec(), entry_len);
                    }
                }

                offset += entry_len;
            }
        }
        Ok(())
    }

    pub fn initialize(storage: Storage) -> Result<Self, StoreError> {
        let store = Self {
            storage,
            keydir: KeyDir::new(),
            total_bytes: AtomicU64::new(0),
            dead_bytes: AtomicU64::new(0),
        };
        store.build_keydir()?;
        Ok(store)
    }

    pub fn put(&self, request: PutRequest) -> Result<PutResponse, StoreError> {
        let bytes = rkyv::to_bytes::<rancor::Error>(&StorageFormat::Put {
            key: request.key.clone(),
            val: request.val,
        })?;

        let AppendResponse {
            file_no: file_index,
            offset,
            timestamp,
        } = self.storage.append(&bytes)?;

        let location = Location {
            file_no: file_index,
            offset,
            size: bytes.len() as u32,
        };
        self.insert_key(request.key, location);

        Ok(PutResponse { timestamp })
    }

    pub fn get(&self, request: GetRequest) -> Result<GetResponse, StoreError> {
        let Some(location) = self.keydir.get(&request.key) else {
            return Err(KeyNotFound(request.key));
        };

        let response =
            self.storage
                .read(location.file_no, location.offset, location.size as u64)?;

        let ArchivedStorageFormat::Put { val, .. } =
            rkyv::access::<ArchivedStorageFormat, rancor::Error>(&response.bytes)?
        else {
            return Err(InvalidGetLocation(request.key, location.value().clone()));
        };

        Ok(GetResponse {
            val: val.to_vec(),
            timestamp: response.timestamp,
        })
    }

    pub fn delete(&self, request: DeleteRequest) -> Result<DeleteResponse, StoreError> {
        let bytes = rkyv::to_bytes::<rancor::Error>(&StorageFormat::Delete {
            key: request.key.clone(),
        })?;

        let AppendResponse { timestamp, .. } = self.storage.append(&bytes)?;

        self.delete_key(request.key, StorageInstance::append_len(&bytes));

        Ok(DeleteResponse { timestamp })
    }

    pub fn contains(&self, request: ContainsRequest) -> Result<ContainsResponse, StoreError> {
        let contains = self.keydir.contains_key(&request.key);

        Ok(ContainsResponse { contains })
    }
}

#[cfg(test)]
pub(crate) mod engine_test_helpers {
    use super::*;

    pub(crate) fn put_request(key: Vec<u8>, val: Vec<u8>) -> PutRequest {
        PutRequest { key, val }
    }

    pub(crate) fn get_request(key: Vec<u8>) -> GetRequest {
        GetRequest { key }
    }

    pub(crate) fn delete_request(key: Vec<u8>) -> DeleteRequest {
        DeleteRequest { key }
    }

    pub(crate) fn contains_request(key: Vec<u8>) -> ContainsRequest {
        ContainsRequest { key }
    }

    pub(crate) fn get_value(response: GetResponse) -> Vec<u8> {
        response.val
    }

    pub(crate) fn contains_value(response: ContainsResponse) -> bool {
        response.contains
    }

    pub(crate) fn put_timestamp(response: PutResponse) -> u64 {
        response.timestamp
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::tests::{TempDir, config};

    fn open_store(database: &TempDir, file_size: u64) -> Store {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(database.path().join("db_file_00000"))
            .unwrap();
        let config = config(database.path(), file_size, 2 * 1024 * 1024);
        Store::initialize(Storage::initialize(config).unwrap()).unwrap()
    }

    #[test]
    fn initializes_keydir_from_an_empty_file() {
        let database = TempDir::new();
        let store = open_store(&database, 1024);

        assert!(store.keydir.is_empty());
    }

    #[test]
    fn put_get_delete_and_contains_keep_keydir_in_sync() {
        let database = TempDir::new();
        let store = open_store(&database, 1024);
        let key = b"key".to_vec();
        let value = b"value".to_vec();

        store
            .put(PutRequest {
                key: key.clone(),
                val: value.clone(),
            })
            .unwrap();
        assert!(
            store
                .contains(ContainsRequest { key: key.clone() })
                .unwrap()
                .contains
        );
        assert_eq!(
            store.get(GetRequest { key: key.clone() }).unwrap().val,
            value
        );

        store.delete(DeleteRequest { key: key.clone() }).unwrap();
        assert!(
            !store
                .contains(ContainsRequest { key: key.clone() })
                .unwrap()
                .contains
        );
        assert!(matches!(
            store.get(GetRequest { key }),
            Err(StoreError::KeyNotFound(_))
        ));
    }

    #[test]
    fn initializes_keydir_from_writes_and_deletes_in_a_single_file() {
        let database = TempDir::new();
        let store = open_store(&database, 1024);
        let retained_key = b"retained".to_vec();
        let deleted_key = b"deleted".to_vec();

        store
            .put(PutRequest {
                key: retained_key.clone(),
                val: b"retained value".to_vec(),
            })
            .unwrap();
        store
            .put(PutRequest {
                key: deleted_key.clone(),
                val: b"deleted value".to_vec(),
            })
            .unwrap();
        store
            .delete(DeleteRequest {
                key: deleted_key.clone(),
            })
            .unwrap();
        drop(store);

        let reopened = open_store(&database, 1024);

        assert!(
            reopened
                .contains(ContainsRequest {
                    key: retained_key.clone()
                })
                .unwrap()
                .contains
        );
        assert_eq!(
            reopened.get(GetRequest { key: retained_key }).unwrap().val,
            b"retained value"
        );
        assert!(
            !reopened
                .contains(ContainsRequest {
                    key: deleted_key.clone()
                })
                .unwrap()
                .contains
        );
        assert!(matches!(
            reopened.get(GetRequest { key: deleted_key }),
            Err(StoreError::KeyNotFound(_))
        ));
    }

    #[test]
    fn initializes_keydir_from_records_in_multiple_files() {
        let database = TempDir::new();
        let store = open_store(&database, 128);
        let entries = [
            (b"key-one".to_vec(), b"value-one".to_vec()),
            (b"key-two".to_vec(), b"value-two".to_vec()),
            (b"key-three".to_vec(), b"value-three".to_vec()),
        ];

        for (key, val) in &entries {
            store
                .put(PutRequest {
                    key: key.clone(),
                    val: val.clone(),
                })
                .unwrap();
        }
        assert!(store.storage.files.pin().keys().count() > 1);
        drop(store);

        let reopened = open_store(&database, 128);

        for (key, val) in entries {
            assert_eq!(reopened.get(GetRequest { key }).unwrap().val, val);
        }
    }
}
