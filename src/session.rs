use log::info;
use tokio::{sync::broadcast::error::RecvError, try_join};

use crate::{
    adj::Adj,
    bus::{Event, EventBus},
    config::Config,
    connections::ConnectionManager,
    message::{Message, MessageBody},
};
use std::{collections::HashSet, error::Error, sync::Arc};

pub struct SessionLayer {
    config: Arc<Config>,
    event_bus: Arc<EventBus>,
    conn_manager: ConnectionManager,
}

impl SessionLayer {
    pub fn new(config: Arc<Config>, event_bus: Arc<EventBus>) -> Self {
        return Self {
            conn_manager: ConnectionManager::new(config.clone(), event_bus.clone()),
            config,
            event_bus,
        };
    }

    pub async fn run(&self) -> Result<(), Box<dyn Error>> {
        try_join!(self.conn_manager.run(), self.run_worker())?;
        return Ok(());
    }

    pub async fn broadcast(
        &self,
        message: &Message,
        targets: &Vec<u32>,
    ) -> Vec<Result<(), Box<dyn Error>>> {
        return self.conn_manager.broadcast(message, targets).await;
    }

    pub async fn send_message(
        &self,
        message: &Message,
        target: &u32,
    ) -> Result<(), Box<dyn Error>> {
        return self.conn_manager.send_message(message, target).await;
    }

    async fn run_worker(&self) -> Result<(), Box<dyn Error>> {
        let conn_tuple = self.config.get_nodes_to_connect().await;
        let mut remaining_conns: HashSet<u32> =
            HashSet::from_iter([conn_tuple.0, conn_tuple.1].into_iter().flatten());
        let mut recv = self.event_bus.subscribe();

        loop {
            let res = self
                .event_bus
                .wait_for(&mut recv, |ev| match ev.clone() {
                    Event::NewConnection(_) => Some(ev),
                    Event::ConnectionClosed(_) => Some(ev),
                    Event::Shutdown => Some(ev),
                    _ => None,
                })
                .await;
            match res {
                Ok(Event::NewConnection(node_id)) => {
                    if self.config.is_node_outside(&node_id).await {
                        self.handle_new_outside_connection(node_id).await?;
                    } else if !remaining_conns.is_empty() {
                        remaining_conns.remove(&node_id);
                        if remaining_conns.is_empty() {
                            info!(target: "SESSION", "Ready! All connections configured for this node have been established.");
                            self.event_bus.emit(Event::NetworkEstablished)?;
                        }
                    }
                }
                Ok(Event::Shutdown) => {
                    info!(target: "SESSION", "Session coordinator exiting.");
                    break;
                }
                Err(RecvError::Closed) => {
                    return Err("Event bus closed".into());
                }
                Err(RecvError::Lagged(skipped)) => {
                    info!(target: "SESSION", "Event bus lagged, missed {} messages", skipped);
                }
                _ => continue,
            }
        }

        return Ok(());
    }

    async fn handle_new_outside_connection(&self, node_id: u32) -> Result<(), Box<dyn Error>> {
        info!(target: "SESSION", "Node {} is an outside node, so we will update Adj to include it", node_id);

        let new_adj = self
            .config
            .apply_outside_connection(node_id, self.config.id)
            .await;

        let msg_adj = new_adj.clone();
        self.config.replace_adj(new_adj).await;
        self.broadcast_adj_update(msg_adj).await?;

        return Ok(());
    }

    pub async fn broadcast_adj_update(&self, new_adj: Adj) -> Result<(), Box<dyn Error>> {
        let msg = Message::new(&self.config, MessageBody::AdjUpdate { adj: new_adj });

        let targets = self.config.get_adjacent_nodes().await.into_iter().collect();
        let results = self.broadcast(&msg, &targets).await;
        let errors: Vec<&Box<dyn Error>> =
            results.iter().filter_map(|r| r.as_ref().err()).collect();
        if !errors.is_empty() {
            return Err(format!(
                "Failed to broadcast adjacency update to {} peers: {:?}",
                errors.len(),
                errors
            )
            .into());
        }

        return Ok(());
    }

    pub async fn establish_download_streams(
        &self,
        targets: &Vec<u32>,
    ) -> Result<Vec<u32>, Box<dyn Error>> {
        let mut temp_conns: Vec<u32> = Vec::new();
        for target in targets {
            if self.conn_manager.has_connection(target).await {
                continue;
            }
            self.conn_manager.connect_to(*target).await?;
            temp_conns.push(*target);
        }

        return Ok(temp_conns);
    }

    pub async fn kill_connection(&self, node_id: &u32) -> Result<(), Box<dyn Error>> {
        return self.conn_manager.kill_connection(&node_id).await;
    }

    pub async fn repair_connections(&self) -> Result<bool, Box<dyn Error>> {
        let required_conns = self.config.get_nodes_to_connect().await.0;
        let mut missing_conns: Vec<u32> = vec![];
        for c in required_conns {
            if !self.conn_manager.has_connection(&c).await {
                missing_conns.push(c);
            }
        }

        if missing_conns.is_empty() {
            return Ok(false);
        }

        for c in missing_conns {
            self.conn_manager.connect_to(c).await?;
            println!("6");
        }

        return Ok(true);
    }
}
