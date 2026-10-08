use std::io::Read;

use crate::{
    ContainsRequest, ContainsResponse,
    DecodingError::{self, InvalidBoolean, InvalidOperation, InvalidStatus},
    DeleteRequest, DeleteResponse, GetRequest, GetResponse,
    Operation::{self, Contains, Delete, Get, Put},
    PutRequest, PutResponse,
    Status::{self, InternalError, InvalidRequest, KeyNotFound, Success},
};

impl Operation {
    pub fn decode(i: u8) -> Result<Operation, DecodingError> {
        match i {
            0 => Ok(Put),
            1 => Ok(Get),
            2 => Ok(Delete),
            3 => Ok(Contains),
            i => Err(InvalidOperation(i)),
        }
    }
}

impl Status {
    pub fn decode(i: u8) -> Result<Status, DecodingError> {
        match i {
            0 => Ok(Success),
            1 => Ok(InternalError),
            2 => Ok(InvalidRequest),
            3 => Ok(KeyNotFound),
            i => Err(InvalidStatus(i)),
        }
    }
}

fn decode_bool(reader: &mut impl Read) -> Result<bool, DecodingError> {
    let mut buf: [u8; 1] = [0; 1];
    reader.read_exact(&mut buf)?;
    match buf[0] {
        0 => Ok(false),
        1 => Ok(true),
        other => Err(InvalidBoolean(other)),
    }
}

fn decode_status(reader: &mut impl Read) -> Result<Status, DecodingError> {
    let mut buf: [u8; 1] = [0; 1];
    reader.read_exact(&mut buf)?;
    Status::decode(buf[0])
}

fn decode_u64(reader: &mut impl Read) -> Result<u64, DecodingError> {
    let mut header: [u8; 8] = [0; 8];
    reader.read_exact(&mut header)?;
    Ok(u64::from_be_bytes(header))
}

fn decode_string(reader: &mut impl Read, len: u64) -> Result<String, DecodingError> {
    let mut buf: Vec<u8> = vec![0; len as usize];
    reader.read_exact(&mut buf)?;
    String::from_utf8(buf.into()).map_err(Into::into)
}

impl GetRequest {
    pub fn decode(reader: &mut impl Read) -> Result<GetRequest, DecodingError> {
        let key_len = decode_u64(reader)?;
        let key = decode_string(reader, key_len)?;
        Ok(GetRequest { key })
    }
}

impl GetResponse {
    pub fn decode(reader: &mut impl Read) -> Result<GetResponse, DecodingError> {
        let status = decode_status(reader)?;
        let val_len = decode_u64(reader)?;
        let val = decode_string(reader, val_len)?;
        Ok(GetResponse { status, val })
    }
}

impl PutRequest {
    pub fn decode(reader: &mut impl Read) -> Result<PutRequest, DecodingError> {
        let key_len = decode_u64(reader)?;
        let key = decode_string(reader, key_len)?;
        let val_len = decode_u64(reader)?;
        let val = decode_string(reader, val_len)?;
        Ok(PutRequest { key, val })
    }
}

impl PutResponse {
    pub fn decode(reader: &mut impl Read) -> Result<PutResponse, DecodingError> {
        let status = decode_status(reader)?;
        Ok(PutResponse { status })
    }
}

impl DeleteRequest {
    pub fn decode(reader: &mut impl Read) -> Result<DeleteRequest, DecodingError> {
        let key_len = decode_u64(reader)?;
        let key = decode_string(reader, key_len)?;
        Ok(DeleteRequest { key })
    }
}

impl DeleteResponse {
    pub fn decode(reader: &mut impl Read) -> Result<DeleteResponse, DecodingError> {
        let status = decode_status(reader)?;
        Ok(DeleteResponse { status })
    }
}

impl ContainsRequest {
    pub fn decode(reader: &mut impl Read) -> Result<ContainsRequest, DecodingError> {
        let key_len = decode_u64(reader)?;
        let key = decode_string(reader, key_len)?;
        Ok(ContainsRequest { key })
    }
}

impl ContainsResponse {
    pub fn decode(reader: &mut impl Read) -> Result<ContainsResponse, DecodingError> {
        let status = decode_status(reader)?;
        let contains: bool = decode_bool(reader)?;
        Ok(ContainsResponse { status, contains })
    }
}
