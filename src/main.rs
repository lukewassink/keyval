mod config;
mod repl;
mod storage;
mod tcp;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
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
    let mut storage = Storage::initialize(&config.database_path)?;
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    ctrlc::set_handler(move || r.store(false, Ordering::SeqCst))?;

    // Run database.
    match config.command_mode {
        CommandMode::Repl => {
            run_repl(&mut storage, running.clone())?;
        }
        CommandMode::Tcp => {
            let listener = create_listener(config.socket_address)?;
            tcp::handle_requests(listener, &mut storage, running.clone())?;
        }
    }

    storage.quit()?;

    Ok(())
}
