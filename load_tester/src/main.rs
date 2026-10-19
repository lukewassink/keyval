use std::net::TcpStream;
use std::thread;
use std::thread::JoinHandle;
use thiserror::Error;

use crate::{
    config::{get_config_path, load_config},
    user::{TestRequests, generate_requests, run_user},
};

mod config;
mod output;
mod user;

#[derive(Debug, Error)]
enum Error {
    #[error("Invalid load tester config: {0}")]
    InvalidConfig(String),
    #[error("{0}")]
    Cli(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Error parsing the config: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("Error generating random values: {0}")]
    Random(#[from] rand::distr::uniform::Error),
    #[error("{0}")]
    Internal(#[from] std::string::FromUtf8Error),
    #[error("Error creating requests: {0}")]
    Encoding(#[from] protocol::EncodingError),
}

fn main() -> Result<(), Error> {
    // Config.
    let config_path = get_config_path()?;
    let config = load_config(config_path)?;
    let address = config.db_address;

    // Generate requests.
    let mut requests_list: Vec<TestRequests> = Vec::new();
    for id in 0..config.users {
        requests_list.push(generate_requests(&config, id)?);
    }

    let mut handles: Vec<JoinHandle<Result<(), Error>>> = Vec::new();

    println!("Streaming to {}...", address);
    for requests in requests_list.into_iter() {
        let mut stream = TcpStream::connect(address)?;

        handles.push(thread::spawn(move || -> Result<(), Error> {
            run_user(&mut stream, requests)
        }));
    }

    for (id, handle) in handles.into_iter().enumerate() {
        if let Err(e) = handle.join() {
            println!("Error for user {}: {:?}", id, e);
        }
    }

    Ok(())
}
