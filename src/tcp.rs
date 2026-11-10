use protocol::Operation::{self, Contains, Delete, Get, Put};
use protocol::Status::{InternalError, InvalidRequest, Success};
use protocol::message::Message;
use std::io::Read;
use std::io::Write;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::thread::{self};
use std::time::Duration;
use thiserror::*;

use crate::engine::{Engine, EngineError, Request, Response};
use crate::store;

#[derive(Error, Debug)]
pub enum TcpHandlerError {
    #[error("Error decoding request: {0}")]
    Request(#[from] protocol::DecodingError),
    #[error("Error encoding response: {0}")]
    Response(#[from] protocol::EncodingError),
    #[error("TCP stream error: {0}")]
    TCPStream(#[from] std::io::Error),
    #[error("Error processing the request {0}")]
    InternalError(#[from] EngineError),
}

pub fn create_listener(socket_addr: SocketAddr) -> std::io::Result<TcpListener> {
    TcpListener::bind(socket_addr)
}

pub fn handle_requests(
    listener: TcpListener,
    engine: Arc<Engine>,
    quit: Arc<AtomicBool>,
) -> Result<(), TcpHandlerError> {
    let Ok(local_address) = listener.local_addr() else {
        println!("Could not access local address.");
        return Ok(());
    };
    println!("Listening on {}...\n", local_address);

    thread::scope(|s| {
        for stream in listener.incoming() {
            if quit.load(std::sync::atomic::Ordering::Relaxed) {
                break;
            }

            let Ok(stream) = stream else {
                println!("Connection error: {:?}", stream);
                continue;
            };

            if let Err(e) = stream.set_read_timeout(Some(Duration::from_millis(1000))) {
                println!("Error setting stream read timeout: {:?}", e);
                continue;
            }

            if let Err(e) = stream.set_nodelay(true) {
                println!("Error setting nodelay: {:?}", e);
                continue;
            }

            let engine = engine.clone();
            let quit = quit.clone();
            s.spawn(move || {
                let _ = handle_connection(stream, engine, quit);
            });
        }
    });

    Ok(())
}

fn handle_connection(
    mut stream: TcpStream,
    engine: Arc<Engine>,
    quit: Arc<AtomicBool>,
) -> Result<(), TcpHandlerError> {
    let address = stream.peer_addr()?;

    loop {
        if quit.load(std::sync::atomic::Ordering::Relaxed) {
            return Ok(());
        }

        let mut op_buf: [u8; 1] = [0; 1];
        if stream.read(&mut op_buf)? == 0 {
            println!("Connection terminated by {}", address);
            return Ok(());
        }

        let engine = engine.clone();
        match Operation::decode(op_buf[0]) {
            Ok(Put) => handle_put_request(&mut stream, engine)?,
            Ok(Get) => handle_get_request(&mut stream, engine)?,
            Ok(Delete) => handle_delete_request(&mut stream, engine)?,
            Ok(Contains) => handle_contains_request(&mut stream, engine)?,
            // Rather than dropping invalid requests silently, consider an
            // InvalidRequestResponse message.
            Err(_) => {}
        }
    }
}

fn handle_put_request(
    mut stream: &mut (impl Read + Write),
    engine: Arc<Engine>,
) -> Result<(), TcpHandlerError> {
    let Ok(request) = protocol::PutRequest::decode(&mut stream) else {
        let response = protocol::PutResponse {
            status: InvalidRequest,
        };
        stream.write_all(&response.encode()?)?;
        return Ok(());
    };

    let request = store::PutRequest {
        key: request.key,
        val: request.val,
    };

    let response = match engine.process(Request::Put(request))?.recv() {
        Ok(_) => protocol::PutResponse { status: Success },
        Err(_) => protocol::PutResponse {
            status: InternalError,
        },
    };

    let buf = response.encode()?;
    stream.write_all(&buf)?;
    Ok(())
}

fn handle_get_request(
    mut stream: &mut (impl Read + Write),
    engine: Arc<Engine>,
) -> Result<(), TcpHandlerError> {
    let Ok(request) = protocol::GetRequest::decode(&mut stream) else {
        let response = protocol::GetResponse::failed(InvalidRequest);
        stream.write_all(&response.encode()?)?;
        return Ok(());
    };

    let request = store::GetRequest { key: request.key };

    let response = match engine.process(Request::Get(request))?.recv() {
        Ok(Response::Get(store::GetResponse { val, .. })) => protocol::GetResponse {
            status: Success,
            val,
        },
        _ => protocol::GetResponse::failed(InternalError),
    };

    stream.write_all(&response.encode()?)?;
    Ok(())
}

fn handle_delete_request(
    mut stream: &mut (impl Read + Write),
    engine: Arc<Engine>,
) -> Result<(), TcpHandlerError> {
    let Ok(request) = protocol::DeleteRequest::decode(&mut stream) else {
        let response = protocol::DeleteResponse {
            status: InvalidRequest,
        };
        stream.write_all(&response.encode()?)?;
        return Ok(());
    };

    let request = store::DeleteRequest { key: request.key };

    let response = match engine.process(Request::Delete(request))?.recv() {
        Ok(_) => protocol::DeleteResponse { status: Success },
        Err(_) => protocol::DeleteResponse {
            status: InternalError,
        },
    };

    let buf = response.encode()?;
    stream.write_all(&buf)?;
    Ok(())
}

fn handle_contains_request(
    mut stream: &mut (impl Read + Write),
    engine: Arc<Engine>,
) -> Result<(), TcpHandlerError> {
    let Ok(request) = protocol::ContainsRequest::decode(&mut stream) else {
        let response = protocol::ContainsResponse::failed(InvalidRequest);
        stream.write_all(&response.encode()?)?;
        return Ok(());
    };

    let request = store::ContainsRequest { key: request.key };

    let response = match engine.process(Request::Contains(request))?.recv() {
        Ok(Response::Contains(store::ContainsResponse { contains, .. })) => {
            protocol::ContainsResponse {
                status: Success,
                contains,
            }
        }
        _ => protocol::ContainsResponse {
            status: InternalError,
            contains: false,
        },
    };

    let buf = response.encode()?;
    stream.write_all(&buf)?;
    Ok(())
}
