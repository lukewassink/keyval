use crate::{
    ContainsRequest, ContainsResponse, DecodingError, DeleteRequest, DeleteResponse,
    EncodingError::{self, InvalidFailureResponse},
    GetRequest, GetResponse,
    Operation::{self, Contains, Delete, Get, Put},
    PutRequest, PutResponse,
    Status::{self, InternalError, InvalidRequest, KeyNotFound, Success},
};

impl Operation {
    fn encode(&self) -> u8 {
        match self {
            Put => 0,
            Get => 1,
            Delete => 2,
            Contains => 3,
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
}

impl GetRequest {
    pub fn encode(&self) -> Result<Vec<u8>, EncodingError> {
        let mut buf: Vec<u8> = Vec::with_capacity(self.key.len() + 9);
        buf.push(Get.encode());
        buf.extend_from_slice(&self.key.len().to_be_bytes());
        buf.extend_from_slice(self.key.as_bytes());
        Ok(buf)
    }
}

impl GetResponse {
    pub fn encode(&self) -> Result<Vec<u8>, EncodingError> {
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
}

impl PutRequest {
    pub fn encode(&self) -> Result<Vec<u8>, EncodingError> {
        let mut buf: Vec<u8> = Vec::with_capacity(self.key.len() + self.val.len() + 17);
        buf.push(Put.encode());
        buf.extend_from_slice(&self.key.len().to_be_bytes());
        buf.extend_from_slice(&self.val.len().to_be_bytes());
        buf.extend_from_slice(self.key.as_bytes());
        buf.extend_from_slice(self.val.as_bytes());
        Ok(buf)
    }
}

impl PutResponse {
    pub fn encode(&self) -> Result<Vec<u8>, EncodingError> {
        let mut buf: [u8; 2] = [0; 2];
        buf[0] = Put.encode();
        buf[1] = self.status.encode();

        Ok(buf.into())
    }
}

impl DeleteRequest {
    pub fn encode(&self) -> Result<Vec<u8>, EncodingError> {
        let mut buf: Vec<u8> = Vec::with_capacity(self.key.len() + 9);
        buf.push(Delete.encode());
        buf.extend_from_slice(&self.key.len().to_be_bytes());
        buf.extend_from_slice(self.key.as_bytes());
        Ok(buf)
    }
}

impl DeleteResponse {
    pub fn encode(&self) -> Result<Vec<u8>, EncodingError> {
        let mut buf: [u8; 2] = [0; 2];
        buf[0] = Delete.encode();
        buf[1] = self.status.encode();

        Ok(buf.into())
    }
}

impl ContainsRequest {
    pub fn encode(&self) -> Result<Vec<u8>, EncodingError> {
        let mut buf: Vec<u8> = Vec::with_capacity(self.key.len() + 9);
        buf.push(Contains.encode());
        buf.extend_from_slice(&self.key.len().to_be_bytes());
        buf.extend_from_slice(self.key.as_bytes());
        Ok(buf)
    }
}

impl ContainsResponse {
    pub fn encode(&self) -> Result<Vec<u8>, EncodingError> {
        let mut buf: [u8; 3] = [0; 3];
        buf[0] = Contains.encode();
        buf[1] = self.status.encode();
        buf[2] = self.contains.into();

        Ok(buf.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
