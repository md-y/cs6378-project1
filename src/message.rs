use std::fmt;

use serde::{Deserialize, Serialize};

use crate::config::Config;

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type")]
pub struct Message {
    pub sender: u32,
    pub body: MessageBody,
    pub hop_count: u32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum MessageBody {
    InitRequest,
    InitResponse,
    None,
}

impl Message {
    pub fn new(config: &Config, body: MessageBody, hop_count: u32) -> Self {
        return Self {
            sender: config.id,
            body,
            hop_count,
        };
    }
}

impl fmt::Display for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let body_text = match self.body {
            MessageBody::InitRequest => "Init Request",
            MessageBody::InitResponse => "Init Response",
            MessageBody::None => "Empty Body",
        };
        return write!(f, "{} from node {}", body_text, self.sender);
    }
}
