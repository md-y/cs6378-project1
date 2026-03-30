#![allow(dead_code)]

use std::env;
use std::error::Error;
use std::path::PathBuf;
use std::sync::Arc;

use log::error;
use tokio::try_join;

use crate::bus::EventBus;
use crate::config::Config;
use crate::file::FileLayer;
use crate::file_manifest::FileManifest;
use crate::logger::setup_logger;
use crate::search::SearchLayer;
use crate::session::SessionLayer;

mod adj;
mod bus;
mod config;
mod connections;
mod file;
mod file_manifest;
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

    let manifest_path = arc_config.get_manifest_path();
    let file_manifest = FileManifest::read_or_generate(&manifest_path).await?;
    let arc_file_manifest = Arc::new(file_manifest);

    let event_bus = Arc::new(EventBus::new());

    let session_layer = SessionLayer::new(arc_config.clone(), event_bus.clone());
    let arc_session_layer = Arc::new(session_layer);

    let search_layer = SearchLayer::new(
        arc_config.clone(),
        event_bus.clone(),
        arc_session_layer.clone(),
        arc_file_manifest.clone(),
    );
    let arc_search_layer = Arc::new(search_layer);

    let file_layer = FileLayer::new(
        arc_config.clone(),
        event_bus.clone(),
        arc_search_layer.clone(),
        arc_session_layer.clone(),
        arc_file_manifest.clone(),
    );

    let runnable_session_layer = arc_session_layer.clone();
    let runnable_search_layer = arc_search_layer.clone();
    try_join!(
        runnable_session_layer.run(),
        runnable_search_layer.run(),
        file_layer.run()
    )?;

    return Ok(());
}
