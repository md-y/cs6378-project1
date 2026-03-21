use std::error::Error;

use crate::config::Config;

mod config;

fn main() -> Result<(), Box<dyn Error>> {
    let config = Config::read()?;
    let addr = config.get_socket_addr()?;

    dbg!(addr);

    return Ok(());
}
