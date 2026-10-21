use std::time::{Duration, Instant};

use protocol::{Response, Status};

#[serde_with::serde_as]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MessageData {
    pub status: protocol::Status,
    pub operation: protocol::Operation,
    #[serde_as(as = "serde_with::DurationMicroSeconds<u64>")]
    pub round_trip: Duration,
}

impl MessageData {
    pub fn from_message(
        op: protocol::Operation,
        response: Result<Response, protocol::DecodingError>,
        send_time: Instant,
        recipt_time: Instant,
    ) -> Self {
        let status: protocol::Status = match response {
            Err(_) => Status::InternalError,
            Ok(r) => (*r.status()).clone(),
        };

        Self {
            round_trip: recipt_time - send_time,
            operation: op,
            status,
        }
    }
}
