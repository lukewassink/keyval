use clap::error::Result;
use crossbeam_channel::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::thread::JoinHandle;
use thiserror::Error;

use crate::engine::EngineError::{InvalidShutdownRequest, NotARead, NotAWrite};
use crate::store::{
    ContainsRequest, ContainsResponse, DeleteRequest, DeleteResponse, GetRequest, GetResponse,
    PutRequest, PutResponse, Store, StoreError,
};

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("{0}")]
    StoreLayer(#[from] StoreError),
    #[error("Requset {0:?} is not a read, but was sent to the reads channel")]
    NotARead(Request),
    #[error("Requset {0:?} is not a write, but was sent to the writes channel")]
    NotAWrite(Request),
    #[error("Error sending request: {0}")]
    CrossbeamSendError(#[from] crossbeam_channel::SendError<(Request, oneshot::Sender<Response>)>),
    #[error(
        "Shutdown should only be passed directly to the read and write receivers, not to proccess()."
    )]
    InvalidShutdownRequest,
    #[error("Send error: {0}")]
    SendError(#[from] oneshot::SendError<Response>),
}

#[derive(Debug)]
pub enum Request {
    Put(PutRequest),
    Get(GetRequest),
    Delete(DeleteRequest),
    Contains(ContainsRequest),
    Shutdown(Private),
}

// Require Private as an argument to Shutdown
// to prevent external callers from shutting down the engine.
// Shutdown should only be passed by the drop method.
#[derive(Debug)]
struct Private;

#[derive(Debug)]
pub enum Response {
    Put(PutResponse),
    Get(GetResponse),
    Delete(DeleteResponse),
    Contains(ContainsResponse),
}

type Handle = JoinHandle<Result<(), EngineError>>;
type Handles = Vec<Handle>;

pub struct Engine {
    readers: Mutex<Option<Handles>>,
    writer: Mutex<Option<Handle>>,
    writes_tx: Sender<(Request, oneshot::Sender<Response>)>,
    reads_tx: Sender<(Request, oneshot::Sender<Response>)>,
}

impl Engine {
    const READER_THREAD_COUNT: usize = 15;
    const CHANNEL_BOUND: usize = 10_000;

    fn handle_reads(
        store: Arc<Store>,
        reads_rx: Arc<Receiver<(Request, oneshot::Sender<Response>)>>,
    ) -> Result<(), EngineError> {
        for (request, sender) in reads_rx.iter() {
            match request {
                Request::Get(get_request) => {
                    let response = store.get(get_request)?;
                    sender.send(Response::Get(response))?;
                }
                Request::Contains(contains_request) => {
                    let response = store.contains(contains_request)?;
                    sender.send(Response::Contains(response))?;
                }
                Request::Shutdown(_) => {
                    break;
                }
                _ => {
                    return Err(NotARead(request));
                }
            }
        }

        Ok(())
    }

    fn handle_writes(
        store: Arc<Store>,
        writes_rx: Receiver<(Request, oneshot::Sender<Response>)>,
    ) -> Result<(), EngineError> {
        for (request, sender) in writes_rx.iter() {
            match request {
                Request::Put(put_request) => {
                    let response = store.put(put_request)?;
                    sender.send(Response::Put(response))?;
                }
                Request::Delete(delete_request) => {
                    let response = store.delete(delete_request)?;
                    sender.send(Response::Delete(response))?;
                }
                Request::Shutdown(_) => {
                    break;
                }
                _ => {
                    return Err(NotAWrite(request));
                }
            }
        }

        Ok(())
    }

    pub fn process(&self, request: Request) -> Result<oneshot::Receiver<Response>, EngineError> {
        let (sender, receiver) = oneshot::channel();

        match request {
            Request::Get(_) | Request::Contains(_) => {
                self.reads_tx.send((request, sender))?;
            }
            Request::Put(_) | Request::Delete(_) => {
                self.writes_tx.send((request, sender))?;
            }
            Request::Shutdown(_) => {
                return Err(InvalidShutdownRequest);
            }
        }

        Ok(receiver)
    }

    pub fn new(store: Arc<Store>) -> Self {
        let mut readers = Vec::with_capacity(Self::READER_THREAD_COUNT);
        let writer;

        let (reads_tx, reads_rx) = crossbeam_channel::bounded(Self::CHANNEL_BOUND);
        let reads_rx = Arc::new(reads_rx);

        for _ in 0..Self::READER_THREAD_COUNT {
            let reads_rx = reads_rx.clone();
            let store = store.clone();
            readers.push(thread::spawn(move || Self::handle_reads(store, reads_rx)));
        }

        let (writes_tx, writes_rx) = crossbeam_channel::bounded(Self::CHANNEL_BOUND);
        {
            let store = store.clone();
            writer = Mutex::new(Some(thread::spawn(move || {
                Self::handle_writes(store, writes_rx)
            })));
        }

        Engine {
            readers: Mutex::new(Some(readers)),
            writer,
            writes_tx,
            reads_tx,
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        // Shut down the write channel.
        let (sender, _receiver) = oneshot::channel();
        let _ = self.writes_tx.send((Request::Shutdown(Private), sender));

        // Shut down all the read channels.
        loop {
            let (sender, _receiver) = oneshot::channel();
            if self
                .reads_tx
                .send((Request::Shutdown(Private), sender))
                .is_err()
            {
                break;
            }
        }

        // Join the threads to allow requests to finish.
        // TODO: maybe log join thread errors instead of ignoring them.
        if let Some(writer) = self.writer.lock().unwrap().take() {
            let _ = writer.join().unwrap();
        }
        if let Some(readers) = self.readers.lock().unwrap().take() {
            for reader in readers {
                let _ = reader.join().unwrap();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{
        Storage,
        tests::{TempDir, config},
    };
    use crate::store::engine_test_helpers::{
        contains_request, contains_value, delete_request, get_request, get_value, put_request,
        put_timestamp,
    };
    use std::fs::File;
    use std::sync::mpsc;
    use std::time::Duration;

    fn make_engine() -> (TempDir, Arc<Engine>) {
        let database = TempDir::new();
        File::create(database.path().join("db_file_00000")).unwrap();

        let store_config = config(database.path(), 1 << 20, 32 << 20);
        let store = Store::initialize(Storage::initialize(store_config).unwrap()).unwrap();

        (database, Arc::new(Engine::new(Arc::new(store))))
    }

    fn close_workers_and_drop(mut engine: Engine) {
        let (writes_tx, _) = crossbeam_channel::bounded(1);
        drop(std::mem::replace(&mut engine.writes_tx, writes_tx));
        let (reads_tx, _) = crossbeam_channel::bounded(1);
        drop(std::mem::replace(&mut engine.reads_tx, reads_tx));
        drop(engine);
    }

    fn receive(receiver: oneshot::Receiver<Response>) -> Response {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            match receiver.try_recv() {
                Ok(response) => return response,
                Err(oneshot::TryRecvError::Empty) if std::time::Instant::now() < deadline => {
                    std::thread::yield_now();
                }
                Err(error) => panic!("failed to receive engine response: {error}"),
            }
        }
    }

    fn assert_put(response: Response) {
        match response {
            Response::Put(response) => assert!(put_timestamp(response) > 0),
            other => panic!("expected Put response, got {other:?}"),
        }
    }

    fn assert_get(response: Response, expected: &[u8]) {
        match response {
            Response::Get(response) => assert_eq!(get_value(response), expected),
            other => panic!("expected Get response, got {other:?}"),
        }
    }

    fn assert_contains(response: Response, expected: bool) {
        match response {
            Response::Contains(response) => {
                assert_eq!(contains_value(response), expected)
            }
            other => panic!("expected Contains response, got {other:?}"),
        }
    }

    #[test]
    fn processes_put_get_delete_and_contains_requests() {
        let (_database, engine) = make_engine();
        let key = b"single-thread-key".to_vec();
        let value = b"first value".to_vec();

        assert_put(receive(
            engine
                .process(Request::Put(put_request(key.clone(), value.clone())))
                .unwrap(),
        ));
        assert_get(
            receive(
                engine
                    .process(Request::Get(get_request(key.clone())))
                    .unwrap(),
            ),
            &value,
        );
        assert_contains(
            receive(
                engine
                    .process(Request::Contains(contains_request(key.clone())))
                    .unwrap(),
            ),
            true,
        );

        let updated_value = b"updated value".to_vec();
        assert_put(receive(
            engine
                .process(Request::Put(put_request(
                    key.clone(),
                    updated_value.clone(),
                )))
                .unwrap(),
        ));
        assert_get(
            receive(
                engine
                    .process(Request::Get(get_request(key.clone())))
                    .unwrap(),
            ),
            &updated_value,
        );

        assert!(matches!(
            receive(
                engine
                    .process(Request::Delete(delete_request(key.clone())))
                    .unwrap(),
            ),
            Response::Delete(_)
        ));
        assert_contains(
            receive(
                engine
                    .process(Request::Contains(contains_request(key)))
                    .unwrap(),
            ),
            false,
        );

        close_workers_and_drop(Arc::try_unwrap(engine).ok().unwrap());
    }

    #[test]
    fn processes_concurrent_ordered_requests_from_ten_threads() {
        let (_database, engine) = make_engine();
        let workers: Vec<_> = (0..10)
            .map(|worker| {
                let engine = Arc::clone(&engine);
                std::thread::spawn(move || {
                    for item in 0..100 {
                        let key = format!("worker-{worker}-item-{item}").into_bytes();
                        let value = format!("value-{worker}-{item}").into_bytes();

                        assert_put(receive(
                            engine
                                .process(Request::Put(put_request(key.clone(), value.clone())))
                                .unwrap(),
                        ));
                        assert_get(
                            receive(
                                engine
                                    .process(Request::Get(get_request(key.clone())))
                                    .unwrap(),
                            ),
                            &value,
                        );
                        assert_contains(
                            receive(
                                engine
                                    .process(Request::Contains(contains_request(key.clone())))
                                    .unwrap(),
                            ),
                            true,
                        );
                        assert!(matches!(
                            receive(
                                engine
                                    .process(Request::Delete(delete_request(key.clone())))
                                    .unwrap(),
                            ),
                            Response::Delete(_)
                        ));
                        assert_contains(
                            receive(
                                engine
                                    .process(Request::Contains(contains_request(key)))
                                    .unwrap(),
                            ),
                            false,
                        );
                    }
                })
            })
            .collect();

        for worker in workers {
            worker.join().unwrap();
        }

        close_workers_and_drop(Arc::try_unwrap(engine).ok().unwrap());
    }

    #[test]
    fn drop_processes_already_queued_requests() {
        let (_database, engine) = make_engine();

        // Put 100 entries
        let entries: Vec<_> = (0..100)
            .map(|index| {
                let key = format!("drop-key-{index}").into_bytes();
                let value = format!("drop-value-{index}").into_bytes();
                assert_put(receive(
                    engine
                        .process(Request::Put(put_request(key.clone(), value.clone())))
                        .unwrap(),
                ));
                (key, value)
            })
            .collect();

        // Get 100 entries and collect the receivers
        let reads: Vec<_> = entries
            .iter()
            .map(|(key, value)| {
                (
                    engine
                        .process(Request::Get(get_request(key.clone())))
                        .unwrap(),
                    value.clone(),
                )
            })
            .collect();

        let engine = match Arc::try_unwrap(engine) {
            Ok(engine) => engine,
            Err(_) => panic!("unexpected outstanding Engine references"),
        };

        // Drop the engine
        let (dropped_tx, dropped_rx) = mpsc::channel();
        std::thread::spawn(move || {
            drop(engine);
            let _ = dropped_tx.send(());
        });

        for (receiver, expected_value) in reads {
            assert_get(receive(receiver), &expected_value);
        }
        assert!(
            dropped_rx.recv_timeout(Duration::from_secs(2)).is_ok(),
            "dropping the engine did not finish after processing queued requests"
        );
    }
}
