use protocol::Operation::{self, Contains, Delete, Get, Put};
use protocol::Status::{InternalError, InvalidRequest, Success};
use protocol::{
    ContainsRequest, DecodingError, DeleteRequest, GetRequest, GetResponse, PutRequest, PutResponse,
};
use std::io::Read;
use std::io::Write;
use std::io::prelude::*;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::net::{TcpListener, TcpStream};

use crate::storage::{ReadRequest, ReadResponse, Storage, WriteRequest};

pub struct TCPConfig {
    socket_addr: SocketAddr,
}

pub fn create_listener(config: TCPConfig) -> std::io::Result<TcpListener> {
    TcpListener::bind(config.socket_addr)
}

pub fn handle_requests(listener: TcpListener, mut storage: Storage) -> std::io::Result<()> {
    for stream in listener.incoming() {
        let mut stream = stream?;
        loop {
            let mut op_buf: [u8; 1] = [0; 1];
            match stream.read(&mut op_buf) {
                Ok(0) => {
                    break;
                }
                Err(_) => {
                    break;
                }
                Ok(_) => match Operation::decode(op_buf[0]) {
                    Ok(Put) => {
                        handle_put_request(&mut stream, &mut storage);
                    }
                    Ok(Get) => {}
                    Ok(Delete) => {}
                    Ok(Contains) => {}
                    Err(e) => {}
                },
            }
        }
    }
    Ok(())
}

fn handle_put_request(
    stream: &mut (impl Read + Write),
    storage: &mut Storage,
) -> Result<(), Box<dyn std::error::Error>> {
    let response = match PutRequest::decode(&mut stream) {
        Err(_) => PutResponse {
            status: InvalidRequest,
        },
        Ok(request) => {
            let writeRequest = WriteRequest {
                key: &request.key,
                val: &request.val,
            };
            match storage.write(&writeRequest) {
                Ok(_) => PutResponse { status: Success },
                Err(_) => PutResponse {
                    status: InternalError,
                },
            }
        }
    };

    let buf = response.encode()?;
    stream.write_all(&buf)?;
}

fn handle_get_request(stream: &mut (impl Read + Write), storage: &mut Storage) {
    let response = match GetRequest::decode(&mut stream) {
        Err(_) => GetResponse {
            status: InvalidRequest,
            val: "".into(),
        },
        Ok(request) => {
            let read_request = ReadRequest { key: &request.key };
            match storage.read(&read_request) {
                Ok(ReadResponse { key, val }) => GetResponse {
                    status: Success,
                    val,
                },
                Err(_) => GetResponse {
                    status: InternalError,
                    val: "".into(),
                },
            }
        }
    };

    stream.write_all(response.encode());
}
