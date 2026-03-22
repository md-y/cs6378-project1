use std::fmt;

use serde::{Deserialize, Serialize};

use crate::config::Config;

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type")]
pub struct Message {
    pub sender: u32,
    pub body: MessageBody,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum MessageBody {
    InitRequest,
    InitResponse,
    None,
}

impl Message {
    pub fn new(config: &Config, body: MessageBody) -> Self {
        return Self {
            sender: config.id,
            body,
        };
    }
}

impl fmt::Display for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let body_text = match self.body {
            MessageBody::InitRequest => "Init Request",
            MessageBody::InitResponse => "Init Response",
            MessageBody::None => "Empty Body"
        };
        return write!(f, "{} from node {}", body_text, self.sender);
    }
}
