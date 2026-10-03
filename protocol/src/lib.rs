pub mod decoding;
pub mod encoding;

pub enum EncodingError {
    InvalidFailureResponse(String),
}

pub enum DecodingError {
    InvalidStatus(String),
    InvalidOperation(String),
    InvalidUTF8(String),
    InvalidBoolean(String),
}

#[derive(PartialEq, Eq, Debug)]
enum Operation {
    Put,
    Get,
    Delete,
    Contains,
}

#[derive(PartialEq, Eq, Debug)]
pub enum Status {
    Success,
    Failure,
    NotFound,
}

pub struct GetRequest {
    key: String,
}

pub struct GetResponse {
    status: Status,
    val: String,
}

pub struct PutRequest {
    key: String,
    val: String,
}

pub struct PutResponse {
    status: Status,
}

pub struct DeleteRequest {
    key: String,
}

pub struct DeleteResponse {
    status: Status,
}

pub struct ContainsRequest {
    key: String,
}

pub struct ContainsResponse {
    status: Status,
    contains: bool,
}
