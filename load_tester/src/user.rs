use crate::Error;
use crate::config::Config;
use crate::output::MessageData;
use core::ops::Range;
use protocol::message::Message;
use protocol::{DeleteRequest, GetRequest, Operation, PutRequest, Response};
use rand::{RngExt, distr::uniform::SampleRange};
use std::collections::HashSet;
use std::io::{Read, Write};
use std::time::Instant;

pub type TestRequests = Vec<(Operation, Vec<u8>)>;

fn random_bytes(range: &Range<usize>) -> Result<Vec<u8>, Error> {
    let mut rng = rand::rng();
    let len: usize = range.clone().sample_single(&mut rng)?;

    let b = rng
        .sample_iter(rand::distr::Alphanumeric)
        .take(len)
        .collect();

    Ok(b)
}

pub fn generate_requests(config: &Config, user_id: usize) -> Result<TestRequests, Error> {
    let mut rng = rand::rng();
    let key_size_range = config.min_val_length..config.max_val_length;

    let mut writes = config.writes;
    let mut reads = config.reads;
    let mut deletes = config.deletes;
    let mut misses = config.misses;

    let key_start = user_id * writes;
    let mut next_key = key_start;
    let mut available_keys = HashSet::with_capacity(writes);
    let mut requests: TestRequests = Vec::with_capacity(reads + writes + deletes + misses);

    for miss_id in 0..(reads + writes + deletes + misses) {
        let pick = (0..reads + writes + deletes + misses).sample_single(&mut rng)?;

        if pick < misses {
            misses -= 1;
            let mut key = Vec::with_capacity(1 + 2 * std::mem::size_of::<usize>());
            key.push(0xff);
            key.extend_from_slice(&user_id.to_be_bytes());
            key.extend_from_slice(&miss_id.to_be_bytes());
            let request = GetRequest { key };
            requests.push((Operation::Get, request.encode()?));
        } else if pick < writes + misses || available_keys.is_empty() {
            writes -= 1;
            let key = next_key;
            let request = PutRequest {
                key: key.to_be_bytes().into(),
                val: random_bytes(&key_size_range)?,
            };
            next_key += 1;
            available_keys.insert(key);
            requests.push((Operation::Put, request.encode()?));
        } else if pick < misses + writes + reads
            || (available_keys.len() == 1 && writes == 0 && reads > 0)
        {
            reads -= 1;
            let key = *available_keys
                .iter()
                .nth((0..available_keys.len()).sample_single(&mut rng)?)
                .unwrap();
            let request = GetRequest {
                key: key.to_be_bytes().into(),
            };
            requests.push((Operation::Get, request.encode()?));
        } else {
            deletes -= 1;
            let key = *available_keys
                .iter()
                .nth((0..available_keys.len()).sample_single(&mut rng)?)
                .unwrap();
            available_keys.remove(&key);
            let request = DeleteRequest {
                key: key.to_be_bytes().into(),
            };
            requests.push((Operation::Delete, request.encode()?));
        }
    }

    Ok(requests)
}

pub fn run_user(
    stream: &mut (impl Read + Write),
    requests: TestRequests,
) -> Result<Vec<MessageData>, Error> {
    let mut data: Vec<MessageData> = Vec::with_capacity(requests.len());
    for (op, request) in requests {
        let send_time = Instant::now();

        stream.write_all(&request)?;
        let response = Response::decode(stream);

        let recipt_time = Instant::now();

        data.push(MessageData::from_message(
            op,
            response,
            send_time,
            recipt_time,
        ));
    }
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use protocol::Request;
    use std::{io::Cursor, net::SocketAddr};

    #[test]
    fn generates_reads_deletes_and_misses() {
        let config = Config {
            users: 1,
            writes: 4,
            reads: 5,
            deletes: 4,
            misses: 6,
            db_address: "127.0.0.1:1234".parse::<SocketAddr>().unwrap(),
            output_path: String::new(),
            min_val_length: 10,
            max_val_length: 20,
        };
        let requests = generate_requests(&config, 0).unwrap();
        let mut available_keys = HashSet::new();
        let mut counts = [0; 4];

        for (operation, encoded) in requests {
            match Request::decode(&mut Cursor::new(encoded)).unwrap() {
                Request::Put(request) => {
                    assert_eq!(operation, Operation::Put);
                    available_keys.insert(request.key);
                    counts[0] += 1;
                }
                Request::Get(request) => {
                    assert_eq!(operation, Operation::Get);
                    if request.key.len() == std::mem::size_of::<usize>() {
                        assert!(available_keys.contains(&request.key));
                        counts[1] += 1;
                    } else {
                        assert!(!available_keys.contains(&request.key));
                        counts[3] += 1;
                    }
                }
                Request::Delete(request) => {
                    assert_eq!(operation, Operation::Delete);
                    assert!(available_keys.remove(&request.key));
                    counts[2] += 1;
                }
                Request::Contains(_) => panic!("unexpected contains request"),
            }
        }

        assert_eq!(counts, [4, 5, 4, 6]);
    }
}
