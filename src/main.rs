use crate::cli::run_cli;

mod cli;
mod storage;
mod tcp_handler;

fn main() {
    run_cli();
}
