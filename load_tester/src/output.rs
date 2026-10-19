use std::time::Instant;

pub struct MessageData {
    pub status: protocol::Status,
    pub operation: protocol::Operation,
    pub send_time: Instant,
    pub recipt_time: Instant,
}

impl MessageData {
    fn from_message(
        op: protocol::Operation,
        response: protocol::Response,
        send_time: Instant,
        recipt_time: Instant,
    ) -> Self {
        Self {
            send_time,
            recipt_time,
            operation: op,
            status: (*response.status()).clone(),
        }
    }
}
