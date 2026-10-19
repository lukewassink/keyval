use thiserror::Error;

pub mod message;

#[cfg(test)]
mod message_tests;

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

#[derive(PartialEq, Eq, Debug, Clone, serde::Deserialize, serde::Serialize)]
pub enum Operation {
    Put,
    Get,
    Delete,
    Contains,
}

#[derive(PartialEq, Eq, Debug, Clone, serde::Deserialize, serde::Serialize)]
pub enum Status {
    Success,
    InternalError,
    InvalidRequest,
    KeyNotFound,
}

#[derive(PartialEq, Debug)]
pub enum Request {
    Get(GetRequest),
    Put(PutRequest),
    Delete(DeleteRequest),
    Contains(ContainsRequest),
}

#[derive(PartialEq, Debug)]
pub enum Response {
    Get(GetResponse),
    Put(PutResponse),
    Delete(DeleteResponse),
    Contains(ContainsResponse),
}

impl Response {
    pub fn status(&self) -> &Status {
        match self {
            Response::Get(g) => &g.status,
            Response::Put(p) => &p.status,
            Response::Delete(d) => &d.status,
            Response::Contains(c) => &c.status,
        }
    }
}

#[derive(PartialEq, Debug)]
pub struct GetRequest {
    pub key: String,
}

#[derive(PartialEq, Debug)]
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

#[derive(PartialEq, Debug)]
pub struct PutRequest {
    pub key: String,
    pub val: String,
}

#[derive(PartialEq, Debug)]
pub struct PutResponse {
    pub status: Status,
}

#[derive(PartialEq, Debug)]
pub struct DeleteRequest {
    pub key: String,
}

#[derive(PartialEq, Debug)]
pub struct DeleteResponse {
    pub status: Status,
}

#[derive(PartialEq, Debug)]
pub struct ContainsRequest {
    pub key: String,
}

#[derive(PartialEq, Debug)]
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
