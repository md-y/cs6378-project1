use log::info;

use crate::{
    bus::{Event, EventBus},
    config::Config,
};
use std::{error::Error, sync::Arc};

pub struct SearchLayer {
    config: Arc<Config>,
    event_bus: Arc<EventBus>,
}

impl SearchLayer {
    pub fn new(config: Arc<Config>, event_bus: Arc<EventBus>) -> Self {
        return Self { config, event_bus };
    }

    pub async fn run(&self) -> Result<(), Box<dyn Error>> {
        self.event_bus
            .wait_for(|ev| match ev {
                Event::NetworkEstablished => Some(()),
                _ => None,
            })
            .await?;

        info!(target: "SEARCH", "Initialized search layer since the network has been established.");

        return Ok(());
    }
}
