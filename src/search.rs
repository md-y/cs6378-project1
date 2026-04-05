use log::info;

use crate::{
    bus::{Event, EventBus},
    config::Config,
    file::FileSearchResult,
    file_manifest::FileManifest,
    message::{Message, MessageBody},
    session::SessionLayer,
};
use std::{
    collections::{HashMap, HashSet},
    error::Error,
    sync::Arc,
};
use tokio::sync::Mutex;

pub struct SearchLayer {
    config: Arc<Config>,
    event_bus: Arc<EventBus>,
    sessions: Arc<SessionLayer>,
    seen_messages: Mutex<HashMap<String, SeenMessage>>,
    our_requests: Mutex<HashSet<String>>,
    file_manifest: Arc<FileManifest>,
}

impl SearchLayer {
    pub fn new(
        config: Arc<Config>,
        event_bus: Arc<EventBus>,
        sessions: Arc<SessionLayer>,
        file_manifest: Arc<FileManifest>,
    ) -> Self {
        return Self {
            config,
            event_bus,
            sessions,
            file_manifest,
            our_requests: Mutex::new(HashSet::new()),
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
                .wait_for(|ev| match ev.clone() {
                    Event::MessageReceived(msg) => match msg.body {
                        MessageBody::SearchRequest { .. } => Some(ev),
                        MessageBody::SearchResponse { .. } => Some(ev),
                        MessageBody::AdjUpdate { .. } => Some(ev),
                        _ => None,
                    },
                    Event::Shutdown => Some(ev),
                    _ => None,
                })
                .await;

            match res {
                Err(err) => {
                    info!(target: "SEARCH", "Error encountered while listening to messages to forward: {}", err);
                }
                Ok(Event::Shutdown) => break,
                Ok(Event::MessageReceived(msg)) => match msg.body {
                    MessageBody::SearchRequest { .. } => {
                        if self.should_consume_request(&msg).await {
                            if let Err(err) = self.try_consume_request(&msg).await {
                                info!(target: "SEARCH", "{}", err);
                            }
                        } else if let Err(err) = self.try_forward_request(&msg).await {
                            info!(target: "SEARCH", "{}", err);
                        }
                    }
                    MessageBody::SearchResponse { .. } => {
                        if self.should_consume_response(&msg).await {
                            if let Err(err) = self.try_consume_response(&msg).await {
                                info!(target: "SEARCH", "{}", err);
                            }
                        } else if let Err(err) = self.try_forward_response(&msg).await {
                            info!(target: "SEARCH", "{}", err);
                        }
                    }
                    MessageBody::AdjUpdate { .. } => {
                        if let Err(err) = self.try_forward_adj_update(&msg).await {
                            info!(target: "SEARCH", "{}", err);
                        }
                    }
                    _ => {}
                },
                _ => {}
            };
        }
    }

    async fn should_consume_request(&self, message: &Message) -> bool {
        return match &message.body {
            MessageBody::SearchRequest { file_name, .. } => {
                self.file_manifest.has_file(&file_name).await
            }
            _ => false,
        };
    }

    async fn try_consume_request(&self, message: &Message) -> Result<(), Box<dyn Error>> {
        match &message.body {
            MessageBody::SearchRequest {
                file_name,
                forwarder,
                ..
            } => {
                info!(target: "SEARCH", "Received request for {}, which we can fulfil. Consuming request and responding.", file_name);
                let response = Message::new(
                    &self.config,
                    MessageBody::SearchResponse {
                        forwarder: self.config.id,
                        file_name: file_name.clone(),
                        reply_to: message.get_key(),
                        // TODO: Do this more efficiently:
                        file_size: self.file_manifest.get_file_data(file_name)?.len() as u64,
                    },
                );
                self.sessions.send_message(&response, &*forwarder).await?;
                return Ok(());
            }
            _ => panic!(),
        };
    }

    async fn should_consume_response(&self, message: &Message) -> bool {
        return match &message.body {
            MessageBody::SearchResponse { reply_to, .. } => {
                self.our_requests.lock().await.contains(reply_to)
            }
            _ => false,
        };
    }

    async fn try_consume_response(&self, message: &Message) -> Result<(), Box<dyn Error>> {
        match &message.body {
            MessageBody::SearchResponse { file_name, .. } => {
                info!(target: "SEARCH", "Received search response for our request for {}", file_name);
                self.event_bus
                    .emit(Event::FileFound(FileSearchResult::new(message)))?;
                return Ok(());
            }
            _ => panic!(),
        };
    }

    async fn try_forward_request(&self, msg: &Message) -> Result<(), Box<dyn Error>> {
        let (hop_count, forwarder) = match msg.body {
            MessageBody::SearchRequest {
                hop_count,
                forwarder,
                ..
            } => (hop_count, forwarder),
            _ => panic!(),
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
                ..
            } => MessageBody::SearchRequest {
                hop_count: hop_count - 1,
                file_name: file_name.clone(),
                forwarder: self.config.id,
            },
            _ => panic!(), // Impossible to reach
        };

        let targets = self.get_broadcastable_nodes(&vec![&msg.sender]).await;
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
            _ => return Err("Message has wrong body format. Expected SearchResponse.".into()),
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

    async fn get_broadcastable_nodes(&self, exclusions: &Vec<&u32>) -> Vec<u32> {
        let mut targets_set = self.config.get_adjacent_nodes().await;
        targets_set.remove(&self.config.id);
        for id in exclusions {
            targets_set.remove(id);
        }
        return targets_set.into_iter().collect();
    }

    async fn try_forward_adj_update(&self, msg: &Message) -> Result<(), Box<dyn Error>> {
        let msg_key = msg.get_key();
        let seen = self.seen_messages.lock().await;
        if seen.contains_key(&msg_key) {
            return Ok(());
        }
        drop(seen);

        let msg_adj = match &msg.body {
            MessageBody::AdjUpdate { adj } => adj,
            _ => return Err("Message body does not contain Adj".into()),
        };

        self.config.replace_adj(msg_adj.clone()).await;

        let new_msg = msg.clone();
        let targets = self.get_broadcastable_nodes(&vec![&msg.sender]).await;
        let results = self.sessions.broadcast(&new_msg, &targets).await;

        let errors: Vec<&Box<dyn Error>> =
            results.iter().filter_map(|r| r.as_ref().err()).collect();
        if !errors.is_empty() {
            return Err(format!(
                "Failed to forward adjacency update to {} nodes. {:?}",
                errors.len(),
                errors
            )
            .into());
        }

        let mut seen = self.seen_messages.lock().await;
        seen.insert(
            msg_key,
            SeenMessage {
                message: msg.clone(),
                forwarder: msg.sender,
            },
        );

        return Ok(());
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
                forwarder: self.config.id,
            },
        );

        let mut requests = self.our_requests.lock().await;
        requests.insert(message.get_key());
        drop(requests);

        let targets = &self.get_broadcastable_nodes(&vec![]).await;
        self.sessions.broadcast(&message, targets).await;

        return Ok(());
    }
}

pub struct SeenMessage {
    message: Message,
    forwarder: u32,
}
