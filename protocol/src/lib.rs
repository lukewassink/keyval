use thiserror::Error;

pub mod decoding;
pub mod encoding;

#[derive(Debug, Error)]
pub enum EncodingError {
    #[error("{0}")]
    InvalidFailureResponse(String),
}

#[derive(Debug, Error)]
pub enum DecodingError {
    #[error("Status number {0} does not correspond to a valid response status.")]
    InvalidStatus(u8),
    #[error("Operation number {0} does not correspond to a valid operation code.")]
    InvalidOperation(u8),
    #[error("{0}")]
    InvalidUTTF8(#[from] std::string::FromUtf8Error),
    #[error("{0}")]
    IOError(#[from] std::io::Error),
    #[error("Byte {0} is not a valid boolean.")]
    InvalidBoolean(u8),
}

#[derive(PartialEq, Eq, Debug)]
pub enum Operation {
    Put,
    Get,
    Delete,
    Contains,
}

#[derive(PartialEq, Eq, Debug)]
pub enum Status {
    Success,
    InternalError,
    InvalidRequest,
    KeyNotFound,
}

pub struct GetRequest {
    pub key: String,
}

pub struct GetResponse {
    pub status: Status,
    pub val: String,
}

pub struct PutRequest {
    pub key: String,
    pub val: String,
}

pub struct PutResponse {
    pub status: Status,
}

pub struct DeleteRequest {
    pub key: String,
}

pub struct DeleteResponse {
    pub status: Status,
}

pub struct ContainsRequest {
    pub key: String,
}

pub struct ContainsResponse {
    pub status: Status,
    pub contains: bool,
}
