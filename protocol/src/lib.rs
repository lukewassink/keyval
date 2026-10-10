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

#[derive(Debug)]
pub struct GetRequest {
    pub key: String,
}

#[derive(Debug)]
pub struct GetResponse {
    pub status: Status,
    pub val: String,
}

impl GetResponse {
    pub fn failed(status: Status) -> GetResponse {
        GetResponse {
            status,
            val: "".into(),
        }
    }
}

#[derive(Debug)]
pub struct PutRequest {
    pub key: String,
    pub val: String,
}

#[derive(Debug)]
pub struct PutResponse {
    pub status: Status,
}

#[derive(Debug)]
pub struct DeleteRequest {
    pub key: String,
}

#[derive(Debug)]
pub struct DeleteResponse {
    pub status: Status,
}

#[derive(Debug)]
pub struct ContainsRequest {
    pub key: String,
}

#[derive(Debug)]
pub struct ContainsResponse {
    pub status: Status,
    pub contains: bool,
}

impl ContainsResponse {
    pub fn failed(status: Status) -> ContainsResponse {
        ContainsResponse {
            status,
            contains: false,
        }
    }
}
