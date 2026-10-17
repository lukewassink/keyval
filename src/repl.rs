use easy_repl::{CommandStatus, Repl, command};
use std::cell::RefCell;
use storage::Storage;
use thiserror::Error;

use crate::{
    storage::{self, ContainsRequest, DeleteRequest, ReadRequest, WriteRequest},
    tcp,
};

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

pub fn run_repl(storage: &mut Storage) -> Result<(), ReplError> {
    let storage = &RefCell::new(storage);

    let mut repl = Repl::builder()
        .add(
            "put",
            command! {
                "Put the pair (key, val)",
                (key: String, val: String) => |key: String, val: String| {
                    match storage.borrow_mut().write(&WriteRequest{key: &key, val: &val}) {
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
                    match storage.borrow_mut().read(&ReadRequest{key: &key}) {
                        Ok(response) => { println!("val: {}", response.val); }
                        Err(e) => { println!("Get failed with error {:?}", e)}
                    }
                    Ok(CommandStatus::Done)
            }},
        )
        .add(
            "delete",
            command! {
                "Delete the entry for <key>",
                (key: String) => |key: String| {
                    match storage.borrow_mut().delete(&DeleteRequest{key: &key}) {
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
                    let c = storage.borrow_mut().contains(&ContainsRequest{key: &key});
                    println!("contains: {}", c);
                    Ok(CommandStatus::Done)
            }},
        )
        .build()?;

    repl.run()?;
    Ok(())
}
