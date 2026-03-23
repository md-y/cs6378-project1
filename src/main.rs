use std::env;
use std::error::Error;

use crate::config::Config;
use crate::session::SessionLayer;

mod adj;
mod config;
mod message;
mod session;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    if let Err(err) = run().await {
        eprintln!("An unrecoverable error was encountered!\n{}", err);
        return Err(err);
    }
    return Ok(());
}

async fn run() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    let config = Config::read_files(&args)?.unwrap();
    let session_layer = SessionLayer::new(&config);
    session_layer.run().await?;
    return Ok(());
}
