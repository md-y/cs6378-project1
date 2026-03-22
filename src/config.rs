use http::uri::Authority;
use serde::Deserialize;
use std::collections::HashMap;
use std::io::ErrorKind;
use std::net::{AddrParseError, IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs};
use std::path::Path;
use std::{fmt, fs, io, vec};

use crate::adj;

pub struct Config {
    pub id: u32,
    pub listen_ip: IpAddr,
    pub adj: adj::Adj,
    pub routes: Vec<Authority>,
}

impl Config {
    /// Attempts to read a series of files, merging any it finds.
    pub fn read_files<P: AsRef<Path>>(file_paths: &[P]) -> Result<Option<Self>, ConfigError> {
        return match RawConfig::read_files(file_paths)? {
            Some(raw) => Ok(Some(raw.to_valid()?)),
            None => Ok(None),
        };
    }

    pub fn resolve_route(&self, id: u32) -> String {
        return self.routes.get(id as usize).unwrap().to_string();
    }

    pub fn get_listen_address(&self) -> SocketAddr {
        let port = self
            .routes
            .get(self.id as usize)
            .and_then(|r| r.port_u16())
            .unwrap();
        return SocketAddr::new(self.listen_ip, port);
    }

    pub fn get_nodes_to_connect(&self) -> Vec<u32> {
        let i = self.id;
        let mut nodes = Vec::<u32>::new();
        for j in 0..(self.adj.n) {
            if !self.adj.get(i, j) {
                continue;
            }
            let bidirectional = self.adj.get(j, i);
            let has_priority = i < j;
            if !bidirectional || (bidirectional && has_priority) {
                nodes.push(j);
            }
        }
        return nodes;
    }
}

#[derive(Deserialize)]
struct RawConfig {
    id: Option<u32>,
    listen_ip: Option<IpAddr>,
    adj: Option<adj::RawAdj>,
    routes: Option<HashMap<u32, String>>,
}

impl RawConfig {
    fn merge(a: Self, b: Self) -> Self {
        return Self {
            id: a.id.or(b.id),
            listen_ip: a.listen_ip.or(b.listen_ip),
            adj: a.adj.or(b.adj),
            routes: a.routes.or(b.routes),
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

    fn to_valid(&self) -> Result<Config, ConfigError> {
        let id = self.id.ok_or(ConfigError::MissingKey("id"))?;
        let raw_adj = self.adj.as_ref().ok_or(ConfigError::MissingKey("adj"))?;
        let adj = raw_adj.to_valid()?;

        if id >= adj.n {
            return Err(ConfigError::InvalidValue("Id is greater than n"));
        }

        let listen_ip = self
            .listen_ip
            .unwrap_or_else(|| IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)));

        let route_map = self.routes.clone().unwrap_or_else(|| HashMap::new());
        let mut routes = Vec::<Authority>::with_capacity(adj.n as usize);
        for i in 0..adj.n {
            match route_map.get(&i) {
                None => return Err(ConfigError::InvalidValue("Route list is incomplete")),
                Some(route_str) => match route_str.parse::<Authority>() {
                    Err(_) => {
                        return Err(ConfigError::InvalidValue(
                            "Route list has invalid authority",
                        ));
                    }
                    Ok(authority) if authority.port() == None => {
                        return Err(ConfigError::InvalidValue("A route is missing port"));
                    }
                    Ok(authority) => routes.push(authority),
                },
            }
        }

        return Ok(Config {
            id,
            listen_ip,
            adj,
            routes,
        });
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
            ConfigError::MissingKey(key) => {
                write!(f, "The \"{}\" setting is missing from the config.", key)
            }
            ConfigError::InvalidValue(value) => {
                write!(f, "The \"{}\" setting value is invalid.", value)
            }
            ConfigError::Adj(err) => write!(f, "The Adj matrix is incorrectly configured: {}", err),
            ConfigError::AddrParse(err) => write!(f, "Socket address parse error: {}", err),
            ConfigError::Io(err) => write!(f, "IO error: {}", err),
            ConfigError::Deserialize(err) => write!(f, "Config deserialization error: {}", err),
        }
    }
}

impl std::error::Error for ConfigError {}
