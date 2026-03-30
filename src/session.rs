use log::info;
use tokio::{sync::broadcast::error::RecvError, try_join};

use crate::{
    bus::{Event, EventBus},
    config::Config,
    connections::{Connection, ConnectionManager},
    message::Message,
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
        try_join!(self.conn_manager.fork(), self.track_new_connections())?;
        return Ok(());
    }

    pub async fn broadcast(&self, message: &Message, targets: &Vec<u32>) -> Vec<Result<(), Box<dyn Error>>> {
        return self.conn_manager.broadcast(message, targets).await;
    }

    pub async fn send_message(&self, message: &Message, target: &u32) -> Result<(), Box<dyn Error>> {
        return self.conn_manager.send_message(message, target).await;
    }

    async fn track_new_connections(&self) -> Result<(), Box<dyn Error>> {
        let conn_tuple = self.config.get_nodes_to_connect();
        let mut remaining_conns: HashSet<u32> = HashSet::new();
        for tuple in [conn_tuple.0, conn_tuple.1] {
            for id in tuple {
                remaining_conns.insert(id);
            }
        }

        loop {
            let res = self
                .event_bus
                .wait_for(|ev| match ev {
                    Event::NewConnection(node_id) => Some(node_id),
                    _ => None,
                })
                .await;
            match res {
                Ok(node_id) => {
                    remaining_conns.remove(&node_id);
                    if remaining_conns.is_empty() {
                        info!(target: "SESSION", "Ready! All required connections for the P2P network have been made.");
                        self.event_bus.emit(Event::NetworkEstablished)?;
                        return Ok(());
                    }
                }
                Err(RecvError::Closed) => {
                    return Err("Event bus closed before network was established".into());
                }
                Err(RecvError::Lagged(skipped)) => {
                    info!(target: "SESSION", "Event bus lagged, missed {} messages", skipped);
                }
            }
        }
    }

    pub async fn establish_download_streams(&self, targets: &Vec<u32>) -> Result<Vec<u32>, Box<dyn Error>> {
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
}
