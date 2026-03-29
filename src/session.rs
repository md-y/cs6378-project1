use log::info;
use tokio::{sync::broadcast::error::RecvError, try_join};

use crate::{
    bus::{Event, EventBus},
    config::Config,
    connections::ConnectionManager,
};
use std::error::Error;

pub struct SessionLayer<'a> {
    config: &'a Config,
    event_bus: &'a EventBus,
}

impl<'a> SessionLayer<'a> {
    pub fn new(config: &'a Config, event_bus: &'a EventBus) -> SessionLayer<'a> {
        return SessionLayer { config, event_bus };
    }

    pub async fn run(&self) -> Result<(), Box<dyn Error>> {
        let conn_manager = ConnectionManager::new(self.config, self.event_bus);
        try_join!(conn_manager.fork(), self.listen_for_network_established())?;
        return Ok(());
    }

    async fn listen_for_network_established(&self) -> Result<(), Box<dyn Error>> {
        let mut receiver = self.event_bus.subscribe();
        loop {
            match receiver.recv().await {
                Ok(Event::NetworkEstablished) => {
                    info!(target: "SESSION", "Ready! All required connections for the P2P network have been made.");
                    return Ok(());
                }
                Ok(_) => {},
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
