#![allow(dead_code)]

use crate::cli::{CLIError, run_cli};

mod cli;
mod storage;
mod tcp_handler;

fn main() -> Result<(), CLIError> {
    run_cli()?;
    Ok(())
}
