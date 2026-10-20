use crate::{
    ContainsRequest, ContainsResponse,
    DecodingError::{self, InvalidBoolean, InvalidOperation, InvalidStatus},
    DeleteRequest, DeleteResponse,
    EncodingError::{self, *},
    GetRequest, GetResponse,
    Operation::{self, Contains, Delete, Get, Put},
    PutRequest, PutResponse,
    Status::{self, InternalError, InvalidRequest, KeyNotFound, Success},
};
use crate::{Request, Response};
use std::io::Read;

pub trait Message: Sized {
    fn encode(&self) -> Result<Vec<u8>, EncodingError>;

    fn decode(reader: &mut impl Read) -> Result<Self, DecodingError>;
}

impl Message for Request {
    fn encode(&self) -> Result<Vec<u8>, EncodingError> {
        match self {
            Request::Get(g) => g.encode(),
            Request::Put(p) => p.encode(),
            Request::Delete(d) => d.encode(),
            Request::Contains(c) => c.encode(),
        }
    }

    fn decode(reader: &mut impl Read) -> Result<Self, DecodingError> {
        let op_buf: &mut [u8; 1] = &mut [0; 1];
        reader.read_exact(op_buf)?;
        let op = Operation::decode(op_buf[0])?;

        match op {
            Operation::Get => Ok(Request::Get(GetRequest::decode(reader)?)),
            Operation::Put => Ok(Request::Put(PutRequest::decode(reader)?)),
            Operation::Delete => Ok(Request::Delete(DeleteRequest::decode(reader)?)),
            Operation::Contains => Ok(Request::Contains(ContainsRequest::decode(reader)?)),
        }
    }
}

impl Message for Response {
    fn encode(&self) -> Result<Vec<u8>, EncodingError> {
        match self {
            Response::Get(g) => g.encode(),
            Response::Put(p) => p.encode(),
            Response::Delete(d) => d.encode(),
            Response::Contains(c) => c.encode(),
        }
    }

    fn decode(reader: &mut impl Read) -> Result<Self, DecodingError> {
        let op_buf: &mut [u8; 1] = &mut [0; 1];
        reader.read_exact(op_buf)?;
        let op = Operation::decode(op_buf[0])?;

        match op {
            Operation::Get => Ok(Response::Get(GetResponse::decode(reader)?)),
            Operation::Put => Ok(Response::Put(PutResponse::decode(reader)?)),
            Operation::Delete => Ok(Response::Delete(DeleteResponse::decode(reader)?)),
            Operation::Contains => Ok(Response::Contains(ContainsResponse::decode(reader)?)),
        }
    }
}

impl Operation {
    fn encode(&self) -> u8 {
        match self {
            Put => 0,
            Get => 1,
            Delete => 2,
            Contains => 3,
        }
    }

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
    pub fn encode(&self) -> u8 {
        match self {
            Success => 0,
            InternalError => 1,
            InvalidRequest => 2,
            KeyNotFound => 3,
        }
    }

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
    String::from_utf8(buf).map_err(Into::into)
}

impl Message for GetRequest {
    fn encode(&self) -> Result<Vec<u8>, EncodingError> {
        let mut buf: Vec<u8> = Vec::with_capacity(self.key.len() + 9);
        buf.push(Get.encode());
        buf.extend_from_slice(&self.key.len().to_be_bytes());
        buf.extend_from_slice(self.key.as_bytes());
        Ok(buf)
    }

    fn decode(reader: &mut impl Read) -> Result<GetRequest, DecodingError> {
        let key_len = decode_u64(reader)?;
        let key = decode_string(reader, key_len)?;
        Ok(GetRequest { key })
    }
}

impl Message for GetResponse {
    fn encode(&self) -> Result<Vec<u8>, EncodingError> {
        if self.status != Success && !self.val.is_empty() {
            return Err(InvalidFailureResponse(format!(
                "A GetResponse with status {:?} must have an empty value.",
                self.status
            )));
        }

        let mut buf: Vec<u8> = Vec::with_capacity(self.val.len() + 9);
        buf.push(Get.encode());
        buf.push(self.status.encode());
        buf.extend_from_slice(&self.val.len().to_be_bytes());
        buf.extend_from_slice(self.val.as_bytes());

        Ok(buf)
    }

    fn decode(reader: &mut impl Read) -> Result<GetResponse, DecodingError> {
        let status = decode_status(reader)?;
        let val_len = decode_u64(reader)?;
        let val = decode_string(reader, val_len)?;
        Ok(GetResponse { status, val })
    }
}

impl Message for PutRequest {
    fn encode(&self) -> Result<Vec<u8>, EncodingError> {
        let mut buf: Vec<u8> = Vec::with_capacity(self.key.len() + self.val.len() + 17);
        buf.push(Put.encode());
        buf.extend_from_slice(&self.key.len().to_be_bytes());
        buf.extend_from_slice(&self.val.len().to_be_bytes());
        buf.extend_from_slice(self.key.as_bytes());
        buf.extend_from_slice(self.val.as_bytes());
        Ok(buf)
    }

    fn decode(reader: &mut impl Read) -> Result<PutRequest, DecodingError> {
        let key_len = decode_u64(reader)?;
        let val_len = decode_u64(reader)?;
        let key = decode_string(reader, key_len)?;
        let val = decode_string(reader, val_len)?;
        Ok(PutRequest { key, val })
    }
}

impl Message for PutResponse {
    fn encode(&self) -> Result<Vec<u8>, EncodingError> {
        let mut buf: [u8; 2] = [0; 2];
        buf[0] = Put.encode();
        buf[1] = self.status.encode();

        Ok(buf.into())
    }

    fn decode(reader: &mut impl Read) -> Result<PutResponse, DecodingError> {
        let status = decode_status(reader)?;
        Ok(PutResponse { status })
    }
}

impl Message for DeleteRequest {
    fn encode(&self) -> Result<Vec<u8>, EncodingError> {
        let mut buf: Vec<u8> = Vec::with_capacity(self.key.len() + 9);
        buf.push(Delete.encode());
        buf.extend_from_slice(&self.key.len().to_be_bytes());
        buf.extend_from_slice(self.key.as_bytes());
        Ok(buf)
    }

    fn decode(reader: &mut impl Read) -> Result<DeleteRequest, DecodingError> {
        let key_len = decode_u64(reader)?;
        let key = decode_string(reader, key_len)?;
        Ok(DeleteRequest { key })
    }
}

impl Message for DeleteResponse {
    fn encode(&self) -> Result<Vec<u8>, EncodingError> {
        let mut buf: [u8; 2] = [0; 2];
        buf[0] = Delete.encode();
        buf[1] = self.status.encode();

        Ok(buf.into())
    }

    fn decode(reader: &mut impl Read) -> Result<DeleteResponse, DecodingError> {
        let status = decode_status(reader)?;
        Ok(DeleteResponse { status })
    }
}

impl Message for ContainsRequest {
    fn encode(&self) -> Result<Vec<u8>, EncodingError> {
        let mut buf: Vec<u8> = Vec::with_capacity(self.key.len() + 9);
        buf.push(Contains.encode());
        buf.extend_from_slice(&self.key.len().to_be_bytes());
        buf.extend_from_slice(self.key.as_bytes());
        Ok(buf)
    }

    fn decode(reader: &mut impl Read) -> Result<ContainsRequest, DecodingError> {
        let key_len = decode_u64(reader)?;
        let key = decode_string(reader, key_len)?;
        Ok(ContainsRequest { key })
    }
}

impl Message for ContainsResponse {
    fn encode(&self) -> Result<Vec<u8>, EncodingError> {
        let mut buf: [u8; 3] = [0; 3];
        buf[0] = Contains.encode();
        buf[1] = self.status.encode();
        buf[2] = self.contains.into();

        Ok(buf.into())
    }

    fn decode(reader: &mut impl Read) -> Result<ContainsResponse, DecodingError> {
        let status = decode_status(reader)?;
        let contains: bool = decode_bool(reader)?;
        Ok(ContainsResponse { status, contains })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DecodingError;

    #[test]
    fn encode_op_code() -> Result<(), DecodingError> {
        let p = Operation::Put;
        assert!(Operation::decode(p.encode())? == p);
        let g = Operation::Get;
        assert!(Operation::decode(g.encode())? == g);
        let d = Operation::Delete;
        assert!(Operation::decode(d.encode())? == d);
        let c = Operation::Contains;
        assert!(Operation::decode(c.encode())? == c);
        Ok(())
    }

    #[test]
    fn encode_status() -> Result<(), DecodingError> {
        let s = Status::Success;
        assert!(Status::decode(s.encode())? == s);
        let i = Status::InternalError;
        assert!(Status::decode(i.encode())? == i);
        let i = Status::InvalidRequest;
        assert!(Status::decode(i.encode())? == i);
        let k = Status::KeyNotFound;
        assert!(Status::decode(k.encode())? == k);
        Ok(())
    }
}
