use std::error::Error;

use crate::config::Config;

mod config;
mod adj;

fn main() -> Result<(), Box<dyn Error>> {
    let config = Config::read_files(&["config.toml", "../config.toml"])?.unwrap();

    dbg!(config.adj.get(0, 0));

    return Ok(());
}
