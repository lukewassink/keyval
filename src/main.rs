use crate::{
    cli::run_cli,
    storage::{DeleteRequest, ReadRequest, Storage, WriteRequest},
};

mod cli;
mod storage;

fn test() {
    let file_path = "test_db";
    let mut s = Storage::initialize(file_path).unwrap();

    let wr = WriteRequest {
        key: "test",
        val: "a value",
    };

    // s.write(&wr);

    let dr = DeleteRequest { key: "test" };

    s.delete(&dr);

    let rr = ReadRequest { key: "test" };

    let rr2 = ReadRequest { key: "other_key" };

    println!("\n offsets: {}\n", s.print_offsets());
    println!("{:?}", s.read(&rr));
    println!("{:?}", s.read(&rr2));
}

fn main() {
    run_cli();
}
