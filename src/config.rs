use serde::Deserialize;
use std::io::ErrorKind;
use std::net::{AddrParseError, IpAddr, SocketAddr};
use std::path::Path;
use std::{fmt, fs, io};

use crate::adj;

const DEFAULT_IP: &str = "127.0.0.1";
const DEFAULT_PORT: u16 = 25563;

pub struct Config {
    pub id: u32,
    pub addr: SocketAddr,
    pub adj: adj::Adj,
}

impl Config {
    pub fn read_file<P: AsRef<Path>>(file_path: P) -> Result<Self, ConfigError> {
        let raw = RawConfig::read_file(file_path)?;
        return Ok(raw.to_valid()?);
    }

    /// Attempts to read a series of files, merging any it finds.
    pub fn read_files<P: AsRef<Path>>(file_paths: &[P]) -> Result<Option<Self>, ConfigError> {
        return match RawConfig::read_files(file_paths)? {
            Some(raw) => Ok(Some(raw.to_valid()?)),
            None => Ok(None),
        };
    }
}

#[derive(Deserialize)]
struct RawConfig {
    id: Option<u32>,
    ip: Option<String>,
    port: Option<u16>,
    adj: Option<adj::RawAdj>,
}

impl RawConfig {
    fn merge(a: Self, b: Self) -> Self {
        return Self {
            id: a.id.or(b.id),
            ip: a.ip.or(b.ip),
            port: a.port.or(b.port),
            adj: a.adj.or(b.adj),
        };
    }

    fn read_file<P: AsRef<Path>>(file_path: P) -> Result<Self, ConfigError> {
        let config_str = fs::read_to_string(file_path)?;
        let config: Self = toml::from_str(&config_str)?;
        return Ok(config);
    }

    fn read_files<P: AsRef<Path>>(file_paths: &[P]) -> Result<Option<Self>, ConfigError> {
        let mut cfg: Option<RawConfig> = None;
        for p in file_paths {
            match RawConfig::read_file(p) {
                Ok(cfg2) => {
                    if let Some(cfg1) = cfg {
                        cfg = Some(RawConfig::merge(cfg1, cfg2))
                    } else {
                        cfg = Some(cfg2)
                    }
                }
                Err(ConfigError::Io(err)) if err.kind() == ErrorKind::NotFound => continue,
                Err(err) => return Err(err),
            }
        }
        return Ok(cfg);
    }

    fn get_socket_addr(&self) -> Result<SocketAddr, AddrParseError> {
        let ip_str = self.ip.as_deref().unwrap_or(DEFAULT_IP);
        let ip: IpAddr = ip_str.parse()?;
        let port = self.port.unwrap_or(DEFAULT_PORT);
        return Ok(SocketAddr::new(ip, port));
    }

    fn to_valid(&self) -> Result<Config, ConfigError> {
        let id = self.id.ok_or(ConfigError::MissingKey("id"))?;
        let raw_adj = self.adj.as_ref().ok_or(ConfigError::MissingKey("adj"))?;
        let adj = raw_adj.to_valid()?;

        if id >= adj.n {
            return Err(ConfigError::InvalidValue("Id is greater than n"));
        }

        let addr = self.get_socket_addr()?;
        return Ok(Config { id, adj, addr });
    }
}

#[derive(Debug)]
pub enum ConfigError {
    MissingKey(&'static str),
    InvalidValue(&'static str),
    AddrParse(AddrParseError),
    Adj(adj::AdjError),
    Io(io::Error),
    Deserialize(toml::de::Error),
}

impl From<AddrParseError> for ConfigError {
    fn from(err: AddrParseError) -> Self {
        ConfigError::AddrParse(err)
    }
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

impl From<adj::AdjError> for ConfigError {
    fn from(err: adj::AdjError) -> Self {
        ConfigError::Adj(err)
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ConfigError::MissingKey(key) => write!(f, "The \"{}\" setting is missing from the config.", key),
            ConfigError::InvalidValue(value) => write!(f, "The \"{}\" setting value is invalid.", value),
            ConfigError::Adj(err) => write!(f, "The Adj matrix is incorrectly configured: {}", err),
            ConfigError::AddrParse(err) => write!(f, "Socket address parse error: {}", err),
            ConfigError::Io(err) => write!(f, "IO error: {}", err),
            ConfigError::Deserialize(err) => write!(f, "Config deserialization error: {}", err),
        }
    }
}

impl std::error::Error for ConfigError {}
