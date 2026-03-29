#![allow(dead_code)]

use std::env;
use std::error::Error;
use std::sync::Arc;

use log::error;
use tokio::try_join;

use crate::bus::EventBus;
use crate::config::Config;
use crate::logger::setup_logger;
use crate::search::SearchLayer;
use crate::session::SessionLayer;

mod adj;
mod bus;
mod config;
mod connections;
mod logger;
mod message;
mod search;
mod session;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    setup_logger();
    if let Err(err) = run().await {
        error!("An unrecoverable error was encountered!\n{}", err);
        return Err(err);
    }
    return Ok(());
}

async fn run() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        return Err(String::from(
            "No arguments were provided. At least one path to a config file must be provided.",
        )
        .into());
    }

    let config = Config::read_files(&args)?.unwrap();
    let arc_config = Arc::new(config);
    let event_bus = Arc::new(EventBus::new());
    let session_layer = SessionLayer::new(arc_config.clone(), event_bus.clone());
    let search_layer = SearchLayer::new(arc_config.clone(), event_bus.clone());
    try_join!(session_layer.run(), search_layer.run())?;
    return Ok(());
}
