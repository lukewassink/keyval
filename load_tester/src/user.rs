use crate::Error;
use crate::config::Config;
use core::ops::Range;
use protocol::message::Message;
use protocol::{GetRequest, PutRequest, Response};
use rand::{RngExt, distr::uniform::SampleRange};
use std::io::{Read, Write};

fn random_string(range: &Range<usize>) -> Result<String, Error> {
    let mut rng = rand::rng();
    let len: usize = range.clone().sample_single(&mut rng)?;

    let s = String::from_utf8(
        rng.sample_iter(rand::distr::Alphanumeric)
            .take(len)
            .collect(),
    )?;

    Ok(s)
}

pub fn generate_requests(config: &Config, user_id: usize) -> Result<Vec<Vec<u8>>, Error> {
    let mut rng = rand::rng();
    let key_size_range = config.min_val_length..config.max_val_length;

    let mut writes = config.writes;
    let mut reads = config.reads;

    let key_start = user_id * writes;
    let mut next_key = key_start;
    let mut requests: Vec<Vec<u8>> = Vec::with_capacity(reads + writes);

    for _ in 0..(reads + writes) {
        let pick = (0..reads + writes).sample_single(&mut rng)?;

        if pick < writes || requests.is_empty() {
            writes -= 1;
            let request = PutRequest {
                key: next_key.to_string(),
                val: random_string(&key_size_range)?,
            };
            next_key += 1;
            requests.push(request.encode()?);
        } else {
            reads -= 1;
            let request = GetRequest {
                key: (key_start..key_start + next_key)
                    .sample_single(&mut rng)?
                    .to_string(),
            };
            requests.push(request.encode()?);
        }
    }

    Ok(requests)
}

pub fn run_user(stream: &mut (impl Read + Write), requests: Vec<Vec<u8>>) -> Result<(), Error> {
    for request in requests {
        stream.write_all(&request)?;
        let _ = Response::decode(stream);
    }
    Ok(())
}
