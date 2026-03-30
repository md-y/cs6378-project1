use std::fmt;

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::config::Config;

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type")]
pub struct Message {
    pub sender: u32,
    pub body: MessageBody,
    pub timestamp: i64,
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
    },
    FileDownloadRequest {
        file_name: String,
        slice: u32,
        total_slices: u32,
    },
    FileDownloadResponse {
        session: String,
        data: Vec<u8>,
    },
}

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

    fn get_current_timestamp() -> i64 {
        return Utc::now().timestamp_millis();
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
                    file_name, slice, total_slices,
                )
            }
            MessageBody::FileDownloadResponse { session, data } => {
                format!(
                    "File Download Response for session {} ({} bytes)",
                    session,
                    data.len(),
                )
            }
        };
        return write!(f, "{} from node {}", body_text, self.sender);
    }
}
