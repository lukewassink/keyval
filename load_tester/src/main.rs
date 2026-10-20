use std::thread;
use std::thread::JoinHandle;
use std::{fs::OpenOptions, net::TcpStream};
use thiserror::Error;

use crate::{
    config::get_config,
    output::MessageData,
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
    #[error("Error serializing to CSV: {0}")]
    Csv(#[from] csv::Error),
}

fn main() -> Result<(), Error> {
    // Config.
    let config = get_config()?;
    let address = config.db_address;

    // Generate requests.
    let mut requests_list: Vec<TestRequests> = Vec::new();
    for id in 0..config.users {
        requests_list.push(generate_requests(&config, id)?);
    }

    let mut handles: Vec<JoinHandle<Result<Vec<MessageData>, Error>>> = Vec::new();

    println!("Streaming to {}...", address);
    for requests in requests_list.into_iter() {
        let mut stream = TcpStream::connect(address)?;

        handles.push(thread::spawn(move || run_user(&mut stream, requests)));
    }

    let mut test_data: Vec<MessageData> = Vec::with_capacity(config.total_requests());
    for (id, handle) in handles.into_iter().enumerate() {
        match handle.join().unwrap() {
            Err(e) => {
                println!("Error for user {}: {:?}", id, e);
            }
            Ok(data) => {
                test_data.extend_from_slice(&data);
            }
        }
    }

    let file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(config.output_path)?;
    let mut writer = csv::WriterBuilder::new()
        .has_headers(true)
        .from_writer(file);

    for row in test_data {
        writer.serialize(row)?;
    }
    writer.flush()?;

    Ok(())
}
