mod config;
mod repl;
mod storage;
mod tcp;

use std::{
    net::{Shutdown, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread::JoinHandle,
};

use thiserror::Error;

use crate::{
    config::{CommandMode, ConfigError, build_config},
    repl::run_repl,
    storage::Storage,
    tcp::create_listener,
};

#[derive(Error, Debug)]
enum Error {
    #[error("{0}")]
    Config(#[from] ConfigError),
    #[error("{0}")]
    Repl(#[from] repl::ReplError),
    #[error("{0}")]
    Storage(#[from] storage::StorageError),
    #[error("IO error: {0}")]
    IO(#[from] std::io::Error),
    #[error("{0}")]
    TcpHandler(#[from] tcp::TcpHandlerError),
    #[error("Error attaching SIGINT listener: {0}")]
    SigintListener(#[from] ctrlc::Error),
}

fn main() -> Result<(), Error> {
    // Load config.
    let config = build_config()?;

    // Initialize database.
    let storage = Storage::initialize(&config.database_path)?;
    let storage = Arc::new(Mutex::new(storage));

    let quit = Arc::new(AtomicBool::new(false));
    let (send, rcv): (Sender<JoinHandle<()>>, Receiver<JoinHandle<()>>) = mpsc::channel();
    {
        let quit = quit.clone();
        ctrlc::set_handler(move || {
            quit.store(true, Ordering::Relaxed);
            let stream = TcpStream::connect(config.socket_address).unwrap();
            stream.shutdown(Shutdown::Both).unwrap();
        })?;
    }

    // Handle requests.
    {
        let storage = storage.clone();
        match config.command_mode {
            CommandMode::Repl => {
                let mut storage = storage.lock().unwrap();
                run_repl(&mut storage).unwrap();
            }
            CommandMode::Tcp => {
                let listener = create_listener(config.socket_address).unwrap();
                tcp::handle_requests(listener, storage, quit, send).unwrap();
            }
        };
    }

    for handle in rcv {
        if let Err(e) = handle.join() {
            println!("Could not join tcp thread: {:?}", e);
        }
    }

    storage.lock().unwrap().quit()?;
    println!("\n\nData saved to disk. Exiting...");

    Ok(())
}
