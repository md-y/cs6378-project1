use serde::Deserialize;
use std::io::ErrorKind;
use std::net::{AddrParseError, IpAddr, SocketAddr};
use std::{fmt, fs, io};

const DEFAULT_CONFIG_PATH: &str = "config.toml";
const DEFAULT_IP: &str = "127.0.0.1";
const DEFAULT_PORT: u16 = 25563;

#[derive(Deserialize)]
pub struct Config {
    ip: Option<String>,
    port: Option<u16>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            ip: None,
            port: None,
        }
    }
}

impl Config {
    /// Reads the config from the default location,
    /// or else returns the default config if it is missing
    pub fn read() -> Result<Self, ConfigError> {
        match Self::read_from(DEFAULT_CONFIG_PATH) {
            Err(ConfigError::Io(e)) if e.kind() == ErrorKind::NotFound => Ok(Self::default()),
            res => res,
        }
    }

    pub fn read_from(file_path: &str) -> Result<Self, ConfigError> {
        let config_str = fs::read_to_string(file_path)?;
        let config: Self = toml::from_str(&config_str)?;
        return Ok(config);
    }

    pub fn get_socket_addr(&self) -> Result<SocketAddr, AddrParseError> {
        let ip_str = self.ip.as_deref().unwrap_or_else(|| DEFAULT_IP);
        let ip: IpAddr = ip_str.parse()?;
        let port = self.port.unwrap_or(DEFAULT_PORT);
        return Ok(SocketAddr::new(ip, port));
    }
}

#[derive(Debug)]
pub enum ConfigError {
    Io(io::Error),
    Deserialize(toml::de::Error),
}

impl From<io::Error> for ConfigError {
    fn from(err: io::Error) -> Self {
        ConfigError::Io(err)
    }
}

impl From<toml::de::Error> for ConfigError {
    fn from(err: toml::de::Error) -> Self {
        ConfigError::Deserialize(err)
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ConfigError::Io(err) => write!(f, "IO error: {}", err),
            ConfigError::Deserialize(err) => write!(f, "Deserialize error: {}", err),
        }
    }
}

impl std::error::Error for ConfigError {}
