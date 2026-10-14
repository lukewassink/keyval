use std::error::Error;
use std::io::Cursor;
use std::io::Read;
use std::io::Write;

use crate::ContainsRequest;
use crate::ContainsResponse;
use crate::DeleteRequest;
use crate::DeleteResponse;
use crate::GetRequest;
use crate::GetResponse;
use crate::Operation;
use crate::PutRequest;
use crate::PutResponse;
use crate::Status;
use crate::Status::Success;

#[test]
fn get_request() -> Result<(), Box<dyn Error>> {
    let request = GetRequest {
        key: "testkey".to_string(),
    };

    let mut buffer: Vec<u8> = vec![];
    buffer.write_all(&request.encode()?)?;
    let mut buffer = Cursor::new(buffer);

    let mut op: [u8; 1] = [0; 1];
    buffer.read_exact(&mut op)?;
    assert!(Operation::decode(op[0])? == Operation::Get);
    assert!(GetRequest::decode(&mut buffer)? == request);

    Ok(())
}

#[test]
fn get_response() -> Result<(), Box<dyn Error>> {
    let response = GetResponse {
        status: Success,
        val: "testvalue".to_string(),
    };

    let mut buffer: Vec<u8> = vec![];
    buffer.write_all(&response.encode()?)?;
    let mut buffer = Cursor::new(buffer);

    let mut op: [u8; 1] = [0; 1];
    buffer.read_exact(&mut op)?;
    assert!(Operation::decode(op[0])? == Operation::Get);
    assert!(GetResponse::decode(&mut buffer)? == response);

    Ok(())
}

#[test]
fn put_request() -> Result<(), Box<dyn Error>> {
    let request = PutRequest {
        key: "here".to_string(),
        val: "there and everywhere".to_string(),
    };

    let mut buffer: Vec<u8> = vec![];
    buffer.write_all(&request.encode()?)?;
    let mut buffer = Cursor::new(buffer);

    let mut op: [u8; 1] = [0; 1];
    buffer.read_exact(&mut op)?;
    assert!(Operation::decode(op[0])? == Operation::Put);
    assert!(PutRequest::decode(&mut buffer)? == request);

    Ok(())
}

#[test]
fn put_respose() -> Result<(), Box<dyn Error>> {
    let response = PutResponse {
        status: Status::InvalidRequest,
    };

    let mut buffer: Vec<u8> = vec![];
    buffer.write_all(&response.encode()?)?;
    let mut buffer = Cursor::new(buffer);

    let mut op: [u8; 1] = [0; 1];
    buffer.read_exact(&mut op)?;
    assert!(Operation::decode(op[0])? == Operation::Put);
    assert!(PutResponse::decode(&mut buffer)? == response);

    Ok(())
}

#[test]
fn delete_request() -> Result<(), Box<dyn Error>> {
    let request = DeleteRequest {
        key: "testkey".to_string(),
    };

    let mut buffer: Vec<u8> = vec![];
    buffer.write_all(&request.encode()?)?;
    let mut buffer = Cursor::new(buffer);

    let mut op: [u8; 1] = [0; 1];
    buffer.read_exact(&mut op)?;
    assert!(Operation::decode(op[0])? == Operation::Delete);
    assert!(DeleteRequest::decode(&mut buffer)? == request);

    Ok(())
}

#[test]
fn delete_response() -> Result<(), Box<dyn Error>> {
    let response = DeleteResponse { status: Success };

    let mut buffer: Vec<u8> = vec![];
    buffer.write_all(&response.encode()?)?;
    let mut buffer = Cursor::new(buffer);

    let mut op: [u8; 1] = [0; 1];
    buffer.read_exact(&mut op)?;
    assert!(Operation::decode(op[0])? == Operation::Delete);
    assert!(DeleteResponse::decode(&mut buffer)? == response);

    Ok(())
}

#[test]
fn contains_request() -> Result<(), Box<dyn Error>> {
    let request = ContainsRequest {
        key: "testkey".to_string(),
    };

    let mut buffer: Vec<u8> = vec![];
    buffer.write_all(&request.encode()?)?;
    let mut buffer = Cursor::new(buffer);

    let mut op: [u8; 1] = [0; 1];
    buffer.read_exact(&mut op)?;
    assert!(Operation::decode(op[0])? == Operation::Contains);
    assert!(ContainsRequest::decode(&mut buffer)? == request);

    Ok(())
}

#[test]
fn contians_response() -> Result<(), Box<dyn Error>> {
    let response = ContainsResponse {
        status: Success,
        contains: true,
    };

    let mut buffer: Vec<u8> = vec![];
    buffer.write_all(&response.encode()?)?;
    let mut buffer = Cursor::new(buffer);

    let mut op: [u8; 1] = [0; 1];
    buffer.read_exact(&mut op)?;
    assert!(Operation::decode(op[0])? == Operation::Contains);
    assert!(ContainsResponse::decode(&mut buffer)? == response);

    Ok(())
}
