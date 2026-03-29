use log::info;
use tokio::{sync::broadcast::error::RecvError, try_join};

use crate::{
    bus::{Event, EventBus},
    config::Config,
    connections::ConnectionManager,
};
use std::{collections::HashSet, error::Error, sync::Arc};

pub struct SessionLayer {
    config: Arc<Config>,
    event_bus: Arc<EventBus>,
}

impl SessionLayer {
    pub fn new(config: Arc<Config>, event_bus: Arc<EventBus>) -> Self {
        return Self { config, event_bus };
    }

    pub async fn run(&self) -> Result<(), Box<dyn Error>> {
        let conn_manager = ConnectionManager::new(self.config.clone(), self.event_bus.clone());
        try_join!(conn_manager.fork(), self.track_new_connections())?;
        return Ok(());
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
}
