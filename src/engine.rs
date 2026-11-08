use crossbeam_channel::{Receiver, Sender};
use rkyv::{Archive, Deserialize, Serialize};
use std::sync::Arc;
use std::thread;
use std::thread::JoinHandle;

use crate::config::Config;
use crate::storage_new::Storage;

#[derive(Archive, Serialize, Deserialize, Debug, PartialEq)]
#[rkyv(compare(PartialEq), derive(Debug))]
enum Request {
    Put { key: Vec<u8>, val: Vec<u8> },
    Get { key: Vec<u8> },
    Delete { key: Vec<u8> },
    Contains { key: Vec<u8> },
}

#[derive(Archive, Serialize, Deserialize, Debug, PartialEq)]
#[rkyv(compare(PartialEq), derive(Debug))]
enum Response {
    Put { timestamp: u64 },
    Get { timestamp: u64, val: Vec<u8> },
    Delete { timestamp: u64 },
    Contains { timestamp: u64, contains: bool },
}

pub struct Engine {
    config: Config,
    readers: Vec<JoinHandle<()>>,
    writer: JoinHandle<()>,
    background: JoinHandle<()>,
    writes_tx: Sender<Request>,
    reads_tx: Sender<Request>,
}

impl Engine {
    const READER_THREAD_COUNT: usize = 15;
    const CHANNEL_BOUND: usize = 10_000;

    fn handle_reads(storage: Arc<Storage>, reads_rx: Arc<Receiver<Request>>) {}

    fn handle_writes(storage: Arc<Storage>, writes_rx: Receiver<Request>) {}

    fn background(storage: Arc<Storage>) {}

    fn shutdown(&self) {}

    pub fn new(config: Config, storage: Arc<Storage>) -> Self {
        let mut readers = Vec::with_capacity(Self::READER_THREAD_COUNT);
        let writer;
        let background;

        let (reads_tx, reads_rx) = crossbeam_channel::bounded(Self::CHANNEL_BOUND);
        let reads_rx = Arc::new(reads_rx);

        for _ in 0..Self::READER_THREAD_COUNT {
            let reads_rx = reads_rx.clone();
            let storage = storage.clone();
            readers.push(thread::spawn(move || Self::handle_reads(storage, reads_rx)));
        }

        let (writes_tx, writes_rx) = crossbeam_channel::bounded(Self::CHANNEL_BOUND);
        {
            let storage = storage.clone();
            writer = thread::spawn(move || Self::handle_writes(storage, writes_rx));
        }

        {
            let storage = storage.clone();
            background = thread::spawn(move || Self::background(storage));
        }

        Engine {
            config,
            readers,
            writer,
            background,
            writes_tx,
            reads_tx,
        }
    }
}
