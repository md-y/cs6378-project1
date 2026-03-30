use log::info;

use crate::{
    bus::{Event, EventBus},
    config::Config,
    message::{Message, MessageBody},
    session::SessionLayer,
};
use std::{collections::HashMap, error::Error, sync::Arc};
use tokio::sync::Mutex;

pub struct SearchLayer {
    config: Arc<Config>,
    event_bus: Arc<EventBus>,
    sessions: Arc<SessionLayer>,
    seen_messages: Mutex<HashMap<String, SeenMessage>>,
}

impl SearchLayer {
    pub fn new(config: Arc<Config>, event_bus: Arc<EventBus>, sessions: Arc<SessionLayer>) -> Self {
        return Self {
            config,
            event_bus,
            sessions,
            seen_messages: Mutex::new(HashMap::new()),
        };
    }

    pub async fn run(&self) -> Result<(), Box<dyn Error>> {
        self.event_bus
            .wait_for(|ev| match ev {
                Event::NetworkEstablished => Some(()),
                _ => None,
            })
            .await?;

        info!(target: "SEARCH", "Initialized search layer since the network has been established.");

        self.run_request_forwarding().await;

        return Ok(());
    }

    async fn run_request_forwarding(&self) {
        loop {
            let res = self
                .event_bus
                .wait_for(|ev| match ev {
                    Event::MessageReceived(msg, forwarder) => match msg.body {
                        MessageBody::SearchResponse { .. } => Some((msg, forwarder)),
                        _ => None,
                    },
                    Event::ShouldForward(msg, forwarder) => Some((msg, forwarder)),
                    _ => None,
                })
                .await;

            match res {
                Err(err) => {
                    info!(target: "SEARCH", "Error encountered while listening to messages to forward: {}", err);
                }
                Ok((msg, forwarder)) => match msg.body {
                    MessageBody::SearchRequest { .. } => {
                        if let Err(err) = self.try_forward_request(&msg, forwarder).await {
                            info!(target: "SEARCH", "{}", err);
                        }
                    }
                    MessageBody::SearchResponse { .. } => {
                        if let Err(err) = self.try_forward_response(&msg).await {
                            info!(target: "SEARCH", "{}", err);
                        }
                    }
                    _ => {}
                },
            };
        }
    }

    async fn try_forward_request(
        &self,
        msg: &Message,
        forwarder: u32,
    ) -> Result<(), Box<dyn Error>> {
        let hop_count = match msg.body {
            MessageBody::SearchRequest { hop_count, .. } => hop_count,
            _ => 0,
        };

        if hop_count == 0 {
            info!(target: "SEARCH", "Received message with hop count of zero, discarding: {}", msg);
            return Ok(());
        }

        // Check if duplicate:
        let msg_key = msg.get_key();
        let mut seen = self.seen_messages.lock().await;
        if seen.contains_key(&msg_key) {
            return Ok(());
        }

        let mut new_msg = msg.clone();
        new_msg.body = match &msg.body {
            MessageBody::SearchRequest {
                hop_count,
                file_name,
            } => MessageBody::SearchRequest {
                hop_count: hop_count - 1,
                file_name: file_name.clone(),
            },
            _ => panic!(), // Impossible to reach
        };

        let targets = self.get_broadcastable_nodes(&vec![&msg.sender]);
        let results = self.sessions.broadcast(&new_msg, &targets).await;

        let errors: Vec<&Box<dyn Error>> =
            results.iter().filter_map(|r| r.as_ref().err()).collect();
        if !errors.is_empty() {
            return Err(format!(
                "Failed to forward message to {} nodes. {:?}",
                errors.len(),
                errors
            )
            .into());
        }

        seen.insert(
            msg_key,
            SeenMessage {
                message: msg.clone(),
                forwarder,
            },
        );

        return Ok(());
    }

    async fn try_forward_response(&self, msg: &Message) -> Result<(), Box<dyn Error>> {
        let reply_key: &String = match &msg.body {
            MessageBody::SearchResponse { reply_to, .. } => reply_to,
            _ => &String::new(),
        };

        let seen = self.seen_messages.lock().await;
        match seen.get(reply_key) {
            None => return Err(format!("Received search request reply, but did not originally receive corresponding request. Request message key: {}", reply_key).into()),
            Some(seen_msg) => {
                let new_msg = msg.clone();
                self.sessions.send_message(&new_msg, &seen_msg.forwarder).await?;
            }
        };

        return Ok(());
    }

    fn get_broadcastable_nodes(&self, exclusions: &Vec<&u32>) -> Vec<u32> {
        let mut targets_set = self.config.get_adjacent_nodes();
        targets_set.remove(&self.config.id);
        for id in exclusions {
            targets_set.remove(id);
        }
        return targets_set.into_iter().collect();
    }

    pub async fn send_search_request(
        &self,
        file_name: &String,
        hop_count: &u32,
    ) -> Result<(), Box<dyn Error>> {
        let message = Message::new(
            &self.config,
            MessageBody::SearchRequest {
                hop_count: hop_count.clone(),
                file_name: file_name.clone(),
            },
        );

        let targets = &self.get_broadcastable_nodes(&vec![]);
        self.sessions.broadcast(&message, targets).await;

        return Ok(());
    }
}

pub struct SeenMessage {
    message: Message,
    forwarder: u32,
}
