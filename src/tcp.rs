use protocol::Operation::{self, Contains, Delete, Get, Put};
use protocol::Status::{InternalError, InvalidRequest, Success};
use protocol::message::Message;
use protocol::{
    ContainsRequest, ContainsResponse, DeleteRequest, DeleteResponse, GetRequest, GetResponse,
    PutRequest, PutResponse,
};
use std::io::Read;
use std::io::Write;
use std::net::{SocketAddr, TcpListener};
use thiserror::*;

use crate::storage::{ReadRequest, ReadResponse, Storage, WriteRequest};

#[derive(Error, Debug)]
pub enum TcpHandlerError {
    #[error("{0}")]
    Database(#[from] crate::storage::StorageError),
    #[error("Error decoding request: {0}")]
    Request(#[from] protocol::DecodingError),
    #[error("Error encoding response: {0}")]
    Response(#[from] protocol::EncodingError),
    #[error("TCP stream error: {0}")]
    TCPStream(#[from] std::io::Error),
}

pub fn create_listener(socket_addr: SocketAddr) -> std::io::Result<TcpListener> {
    TcpListener::bind(socket_addr)
}

pub fn handle_requests(
    listener: TcpListener,
    storage: &mut Storage,
) -> Result<(), TcpHandlerError> {
    let Ok(local_address) = listener.local_addr() else {
        println!("Could not access local address.");
        return Ok(());
    };
    println!("Listening on {}...\n", local_address);

    for stream in listener.incoming() {
        let Ok(mut stream) = stream else {
            break;
        };

        let Ok(address) = stream.peer_addr() else {
            break;
        };
        println!("Connected to {:?}", address);

        loop {
            let mut op_buf: [u8; 1] = [0; 1];
            match stream.read(&mut op_buf) {
                Ok(0) => {
                    println!("Connection terminated by {}", address);
                    break;
                }
                Err(e) => {
                    println!(
                        "Error in connection with {}, {}\nConnection terminated.",
                        address, e
                    );
                    break;
                }
                Ok(_) => match Operation::decode(op_buf[0]) {
                    // How to handle errors here? Maybe log them in some way?
                    Ok(Put) => match handle_put_request(&mut stream, storage) {
                        Ok(_) => println!("Put from {}", address),
                        Err(e) => println!("Error in put from {}, {}", address, e),
                    },
                    Ok(Get) => match handle_get_request(&mut stream, storage) {
                        Ok(_) => println!("Get from {}", address),
                        Err(e) => println!("Error in put from {}, {}", address, e),
                    },
                    Ok(Delete) => handle_delete_request(&mut stream, storage)?,
                    Ok(Contains) => handle_contains_request(&mut stream, storage)?,
                    // Rather than dropping invalid requests silently, consider an
                    // InvalidRequestResponse message.
                    Err(_) => {}
                },
            }
        }
    }
    Ok(())
}

fn handle_put_request(
    mut stream: &mut (impl Read + Write),
    storage: &mut Storage,
) -> Result<(), TcpHandlerError> {
    let Ok(request) = PutRequest::decode(&mut stream) else {
        println!("Error!");
        let response = PutResponse {
            status: InvalidRequest,
        };
        stream.write_all(&response.encode()?)?;
        return Ok(());
    };

    println!("Request: {:?}", request);

    let write_request = WriteRequest {
        key: &request.key,
        val: &request.val,
    };

    let response = match storage.write(&write_request) {
        Ok(_) => PutResponse { status: Success },
        Err(_) => PutResponse {
            status: InternalError,
        },
    };

    let buf = response.encode()?;
    stream.write_all(&buf)?;
    Ok(())
}

fn handle_get_request(
    mut stream: &mut (impl Read + Write),
    storage: &mut Storage,
) -> Result<(), TcpHandlerError> {
    let Ok(request) = GetRequest::decode(&mut stream) else {
        let response = GetResponse::failed(InvalidRequest);
        stream.write_all(&response.encode()?)?;
        return Ok(());
    };

    println!("Request: {:?}", request);

    let read_request = ReadRequest { key: &request.key };

    let response = match storage.read(&read_request) {
        Ok(ReadResponse { val, .. }) => GetResponse {
            status: Success,
            val,
        },
        Err(_) => GetResponse::failed(InternalError),
    };

    stream.write_all(&response.encode()?)?;
    Ok(())
}

fn handle_delete_request(
    mut stream: &mut (impl Read + Write),
    storage: &mut Storage,
) -> Result<(), TcpHandlerError> {
    let Ok(request) = DeleteRequest::decode(&mut stream) else {
        let response = DeleteResponse {
            status: InvalidRequest,
        };
        stream.write_all(&response.encode()?)?;
        return Ok(());
    };

    let delete_request = crate::storage::DeleteRequest { key: &request.key };

    let response = match storage.delete(&delete_request) {
        Ok(_) => DeleteResponse { status: Success },
        Err(_) => DeleteResponse {
            status: InternalError,
        },
    };

    let buf = response.encode()?;
    stream.write_all(&buf)?;
    Ok(())
}

fn handle_contains_request(
    mut stream: &mut (impl Read + Write),
    storage: &mut Storage,
) -> Result<(), TcpHandlerError> {
    let Ok(request) = ContainsRequest::decode(&mut stream) else {
        let response = ContainsResponse::failed(InvalidRequest);
        stream.write_all(&response.encode()?)?;
        return Ok(());
    };

    let contains_request = crate::storage::ContainsRequest { key: &request.key };

    let response = ContainsResponse {
        status: Success,
        contains: storage.contains(&contains_request),
    };

    let buf = response.encode()?;
    stream.write_all(&buf)?;
    Ok(())
}
