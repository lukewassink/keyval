use easy_repl::{CommandStatus, Repl, command};
use thiserror::Error;

use crate::engine::{Engine, Request, Response};
use crate::store::{ContainsRequest, DeleteRequest, GetRequest, PutRequest};
use crate::tcp;

#[derive(Error, Debug)]
pub enum ReplError {
    #[error("{0}")]
    Database(#[from] crate::storage::StorageError),
    #[error("{0}")]
    Tcp(#[from] tcp::TcpHandlerError),
    #[error("IO error: {0}")]
    IOError(#[from] std::io::Error),
    #[error("REPL error: {0}")]
    Repl(String),
}

impl From<easy_repl::CriticalError> for ReplError {
    fn from(e: easy_repl::CriticalError) -> Self {
        ReplError::Repl(format!("{:?}", e))
    }
}

impl From<easy_repl::anyhow::Error> for ReplError {
    fn from(e: easy_repl::anyhow::Error) -> Self {
        ReplError::Repl(format!("{:?}", e))
    }
}

impl From<easy_repl::repl::BuilderError> for ReplError {
    fn from(e: easy_repl::repl::BuilderError) -> Self {
        ReplError::Repl(format!("{:?}", e))
    }
}

pub fn run_repl(engine: &Engine) -> Result<(), ReplError> {
    let mut repl = Repl::builder()
        .add(
            "put",
            command! {
                "Put the pair (key, val)",
                (key: String, val: String) => |key: String, val: String| {
                    match engine.process(Request::Put(PutRequest {key: key.as_bytes().to_vec(), val: val.as_bytes().to_vec()}))?.recv() {
                        Ok(_) => { println!("Done"); }
                        Err(e) => { println!("Put failed with error {:?}", e)}
                    }
                    Ok(CommandStatus::Done)
            }},
        )
        .add(
            "get",
            command! {
                "Get the value for <key>",
                (key: String) => |key: String| {
                    match engine.process(Request::Get(GetRequest {key: key.as_bytes().to_vec() }))?.recv() {
                        Ok(Response::Get(response)) => { println!("val: {}", String::from_utf8_lossy(&response.val)); }
                        Err(e) => { println!("Get failed with error {:?}", e)}
                        _ => { println!("Error: incorrect response type."); }
                    }
                    Ok(CommandStatus::Done)
            }},
        )
        .add(
            "delete",
            command! {
                "Delete the entry for <key>",
                (key: String) => |key: String| {
                    match engine.process(Request::Delete(DeleteRequest {key: key.as_bytes().to_vec() }))?.recv() {
                        Ok(_) => { println!("Done"); }
                        Err(e) => { println!("Get failed with error {:?}", e)}
                    }
                    Ok(CommandStatus::Done)
            }},
        )
        .add(
            "contains",
            command! {
                "Check if there is an entry for <key>",
                (key: String) => |key: String| {
                    match engine.process(Request::Contains(ContainsRequest {key: key.as_bytes().to_vec() }))?.recv() {
                        Ok(Response::Contains(response)) => { println!("contains: {}", response.contains); }
                        Err(e) => { println!("Get failed with error {:?}", e)}
                        _ => { println!("Error: incorrect response type."); }
                    }
                    Ok(CommandStatus::Done)
                }},
        )
        .add(
            "keys",
            command! {
                "Print the first 100 keys",
                () => || {
                    println!("Not yet implemented.");
                    Ok(CommandStatus::Done)
                }},
        )
        .build()?;

    repl.run()?;
    Ok(())
}
