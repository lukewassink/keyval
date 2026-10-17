mod config;
mod user;

use clap::error;
use std::io::prelude::*;
use std::net::TcpStream;
use thiserror::Error;

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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let address = "localhost:49153";
    let mut stream = TcpStream::connect(address)?;
    println!("Streaming to {}...", address);

    let put = protocol::PutRequest {
        key: "hi".to_string(),
        val: "there".to_string(),
    }
    .encode()?;

    stream.write_all(&put)?;

    let mut op_buf: [u8; 1] = [0; 1];

    stream.read_exact(&mut op_buf)?;
    println!("Op: {:?}", protocol::Operation::decode(op_buf[0]));
    println!("Response: {:?}", protocol::PutResponse::decode(&mut stream));
    Ok(())
}
