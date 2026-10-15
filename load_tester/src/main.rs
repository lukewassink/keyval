use std::io::prelude::*;
use std::net::TcpStream;

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
