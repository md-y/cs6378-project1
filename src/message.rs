use std::{fmt, sync::Mutex};

use serde::{Deserialize, Serialize};

use crate::{adj::Adj, config::Config};

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type")]
pub struct Message {
    pub sender: u32,
    pub body: MessageBody,
    pub timestamp: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum MessageBody {
    None,
    InitRequest,
    InitResponse,
    SearchRequest {
        forwarder: u32,
        hop_count: u32,
        file_name: String,
    },
    SearchResponse {
        forwarder: u32,
        file_name: String,
        reply_to: String,
        file_size: u64,
    },
    FileDownloadRequest {
        file_name: String,
        slice: u32,
        total_slices: u32,
    },
    FileDownloadResponse {
        slice: u32,
        data: Vec<u8>,
    },
    AdjUpdate {
        adj: Adj,
    },
}

static LOGICAL_CLOCK: Mutex<u64> = Mutex::new(0);

impl Message {
    pub fn new(config: &Config, body: MessageBody) -> Self {
        return Self {
            sender: config.id,
            body,
            timestamp: Self::get_current_timestamp(),
        };
    }

    pub fn get_key(&self) -> String {
        return format!("{}-{}", self.sender, self.timestamp);
    }

    fn get_current_timestamp() -> u64 {
        let mut clock = LOGICAL_CLOCK.lock().unwrap();
        *clock += 1;
        return *clock;
    }
}

impl fmt::Display for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let body_text: String = match &self.body {
            MessageBody::None => "Empty Body".into(),
            MessageBody::InitRequest => "Init Request".into(),
            MessageBody::InitResponse => "Init Response".into(),
            MessageBody::SearchRequest {
                hop_count,
                file_name,
                ..
            } => {
                format!(
                    "Search Request with hop count {} for {}",
                    hop_count, file_name
                )
            }
            MessageBody::SearchResponse {
                file_name,
                reply_to,
                ..
            } => {
                format!(
                    "Search Response from {} with file {} (is a reply to message {})",
                    self.sender, file_name, reply_to
                )
            }
            MessageBody::FileDownloadRequest {
                file_name,
                slice,
                total_slices,
                ..
            } => {
                format!(
                    "File Download Request for file {} (slice {} of {})",
                    file_name,
                    slice + 1,
                    total_slices,
                )
            }
            MessageBody::FileDownloadResponse { data, .. } => {
                format!(
                    "File Download Response from {} ({} bytes)",
                    self.sender,
                    data.len(),
                )
            }
            MessageBody::AdjUpdate { adj, .. } => {
                format!("Adjacency update (matrix size {})", adj.n)
            }
        };
        return write!(f, "{} from node {}", body_text, self.sender);
    }
}
