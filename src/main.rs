mod config;
mod engine;
mod repl;
mod storage;
mod store;
mod tcp;

use std::{
    net::{Shutdown, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use thiserror::Error;

use crate::{
    config::{CommandMode, ConfigError, build_config},
    engine::Engine,
    repl::run_repl,
    storage::Storage,
    store::Store,
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
    #[error("{0}")]
    Store(#[from] store::StoreError),
    #[error("{0}")]
    Engine(#[from] engine::EngineError),
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
    let storage = Storage::initialize(config.clone())?;
    let store = Arc::new(Store::initialize(storage)?);
    let engine = Engine::new(store);

    let quit = Arc::new(AtomicBool::new(false));
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
        match config.command_mode {
            CommandMode::Repl => {
                run_repl(&engine).unwrap();
            }
            CommandMode::Tcp => {
                let listener = create_listener(config.socket_address).unwrap();
                tcp::handle_requests(listener, Arc::new(engine), quit).unwrap();
            }
        };
    }

    println!("\n\nData saved to disk. Exiting...");

    Ok(())
}
