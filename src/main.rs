use std::error::Error;

use crate::config::Config;

mod adj;
mod config;

fn main() -> Result<(), Box<dyn Error>> {
    if let Err(err) = run() {
        eprintln!("An unrecoverable error was encountered!\n{}", err);
        return Err(err);
    }
    return Ok(());
}

fn run() -> Result<(), Box<dyn Error>> {
    let config = Config::read_files(&["config.toml", "../config.toml"])?.unwrap();

    dbg!(config.get_listen_address());

    return Ok(());
}
