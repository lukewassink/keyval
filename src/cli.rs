use clap::Parser;
use easy_repl::{CommandStatus, Repl, command, repl::BuilderError};
use std::cell::RefCell;
use storage::Storage;

use crate::storage::{self, ContainsRequest, DeleteRequest, ReadRequest, WriteRequest};

#[derive(Parser)]
struct Cli {
    path: String,
}

pub fn run_cli() {
    let cli = Cli::parse();

    let mut s = match Storage::initialize(&cli.path) {
        Ok(s) => s,
        Err(e) => {
            println!("Could not initialize database due to error: {:?}", e);
            return;
        }
    };

    let s = RefCell::new(s);
    let put_s = &s;
    let get_s = &s;
    let delete_s = &s;
    let contains_s = &s;

    let repl = Repl::builder()
        .add(
            "put",
            command! {
                "Put the pair (key, val)",
                (key: String, val: String) => |key: String, val: String| {
                    match put_s.borrow_mut().write(&WriteRequest{key: &key, val: &val}) {
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
                    match get_s.borrow_mut().read(&ReadRequest{key: &key}) {
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
                    match delete_s.borrow_mut().delete(&DeleteRequest{key: &key}) {
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
                    let c = contains_s.borrow_mut().contains(&ContainsRequest{key: &key});
                    println!("contains: {}", c);
                    Ok(CommandStatus::Done)
            }},
        )
        .build();

    match repl {
        Ok(mut r) => {
            r.run();
        }
        Err(e) => {
            println!("Failed to start with error: {:?}", e);
        }
    }
}
