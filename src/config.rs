use http::uri::Authority;
use log::info;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::io::ErrorKind;
use std::net::{AddrParseError, IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;
use std::{fmt, fs, io};
use tokio::sync::Mutex;

use crate::adj::{self, Adj};

pub struct Config {
    pub id: u32,
    pub target_node: Option<u32>,
    pub listen_ip: IpAddr,
    pub adj: Mutex<adj::Adj>,
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

    /// Returns two vectors: first is nodes to connect to, and second is nodes to listen for
    pub async fn get_nodes_to_connect(&self) -> (Vec<u32>, Vec<u32>) {
        if self.is_node_outside(&self.id).await {
            let t = self
                .target_node
                .expect("outside nodes must have target_node set");
            return (vec![t], vec![]);
        }
        let adj = self.adj.lock().await;
        let i = self.id;
        let mut outgoing_nodes = Vec::<u32>::new();
        let mut incoming_nodes = Vec::<u32>::new();
        for j in 0..(adj.n) {
            let outgoing = adj.get_with_deactivated(i, j);
            let incoming = adj.get_with_deactivated(j, i);
            let bidirectional = outgoing && incoming;
            let has_priority = i < j;
            if bidirectional {
                if has_priority {
                    outgoing_nodes.push(j);
                } else {
                    incoming_nodes.push(j);
                }
            } else if outgoing {
                outgoing_nodes.push(j);
            } else if incoming {
                incoming_nodes.push(j);
            }
        }
        return (outgoing_nodes, incoming_nodes);
    }

    pub async fn get_adjacent_nodes(&self) -> HashSet<u32> {
        let adj = self.adj.lock().await;
        let i = self.id;
        let mut set = HashSet::new();
        for j in 0..(adj.n) {
            if (adj.get(i, j) || adj.get(j, i)) && i != j {
                set.insert(j);
            }
        }
        return set;
    }

    pub async fn replace_adj(&self, new_adj: adj::Adj) {
        let mut guard = self.adj.lock().await;
        *guard = new_adj;
        drop(guard);
        info!(target: "CONFIG", "Updated adj to:");
        self.print_adj().await;
    }

    pub async fn is_node_outside(&self, node_id: &u32) -> bool {
        let adj = self.adj.lock().await;
        return *node_id >= adj.n || adj.is_deactivated(node_id);
    }

    pub async fn is_outside(&self) -> bool {
        return self.is_node_outside(&self.id).await;
    }

    pub async fn print_adj(&self) {
        println!("0 = No Edge, 1 = Edge, _ = Node is disconnected from network");
        let adj = self.adj.lock().await;
        for (i, row) in adj.data.chunks(adj.n as usize).enumerate() {
            for (j, &val) in row.iter().enumerate() {
                if adj.is_deactivated(&(i as u32)) || adj.is_deactivated(&(j as u32)) {
                    print!("_ ");
                } else {
                    print!("{} ", val as u8);
                }
            }
            println!();
        }
    }

    pub async fn apply_outside_connection(&self, node_id: u32, target_node: u32) -> Adj {
        let adj = self.adj.lock().await;
        let mut new_adj: Adj;
        if node_id >= adj.n {
            new_adj = adj.expand(node_id + 1);
        } else {
            new_adj = adj.clone();
        }
        drop(adj);

        new_adj.activate_node(&node_id);
        new_adj.set(node_id, target_node, true);

        return new_adj;
    }

    pub async fn is_node_active(&self, node_id: &u32) -> bool {
        let adj = self.adj.lock().await;
        return adj.is_activated(node_id);
    }
}

#[derive(Deserialize)]
struct RawConfig {
    id: Option<u32>,
    target_node: Option<u32>,
    listen_ip: Option<IpAddr>,
    adj: Option<adj::RawAdj>,
    routes: Option<HashMap<String, String>>,
}

impl RawConfig {
    fn merge(a: Self, b: Self) -> Self {
        return Self {
            id: a.id.or(b.id),
            target_node: a.target_node.or(b.target_node),
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

        let target_node = self.target_node;
        if let Some(val) = target_node {
            if val >= adj.n {
                return Err(ConfigError::InvalidValue("target_node"));
            }
        } else if id >= adj.n {
            return Err(ConfigError::MissingKey("target_node"));
        }

        let listen_ip = self
            .listen_ip
            .unwrap_or_else(|| IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)));

        let route_map = self.routes.clone().unwrap_or_else(|| HashMap::new());
        if route_map.len() == 0 {
            return Err(ConfigError::InvalidValue("Route list is empty"));
        }

        let max_route_key = route_map
            .keys()
            .max_by_key(|r| r.parse::<u32>().unwrap())
            .unwrap();
        let max_route_id = max_route_key.parse::<u32>().unwrap();
        let mut routes = Vec::<Authority>::new();

        for i in 0..(max_route_id + 1) {
            match route_map.get(&i.to_string()) {
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
            target_node,
            listen_ip,
            adj: Mutex::new(adj),
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
