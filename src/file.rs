use log::info;

use crate::{
    bus::{Event, EventBus},
    config::Config,
    file_manifest::FileManifest,
    message::{Message, MessageBody},
    session::SessionLayer,
};
use std::{collections::HashMap, error::Error, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    join,
    sync::{broadcast::error::RecvError, Mutex},
    time::sleep,
    try_join,
};

use crate::search::SearchLayer;

const HELP_TEXT: &'static str = include_str!("./static/help_text.txt");

pub struct FileLayer {
    config: Arc<Config>,
    event_bus: Arc<EventBus>,
    search: Arc<SearchLayer>,
    sessions: Arc<SessionLayer>,
    last_search_result: Mutex<HashMap<String, FileSearchResult>>,
    file_manifest: Arc<FileManifest>,
}

impl FileLayer {
    pub fn new(
        config: Arc<Config>,
        event_bus: Arc<EventBus>,
        search: Arc<SearchLayer>,
        sessions: Arc<SessionLayer>,
        file_manifest: Arc<FileManifest>,
    ) -> Self {
        return Self {
            config,
            file_manifest,
            event_bus,
            search,
            sessions,
            last_search_result: Mutex::new(HashMap::new()),
        };
    }

    pub async fn run(&self) -> Result<(), Box<dyn Error>> {
        self.event_bus
            .wait_for_once(|ev| match ev {
                Event::NetworkEstablished => Some(()),
                _ => None,
            })
            .await?;

        info!(target: "FILE", "Initialized file layer since the network has been established.");

        join!(
            self.run_command_processing(),
            self.listen_for_file_requests()
        );

        return Ok(());
    }

    async fn listen_for_file_requests(&self) {
        let mut recv = self.event_bus.subscribe();
        loop {
            let res = self
                .event_bus
                .wait_for(&mut recv, |ev| match ev.clone() {
                    Event::MessageReceived(msg) => match msg.body {
                        MessageBody::FileDownloadRequest { .. } => Some(ev),
                        _ => None,
                    },
                    Event::Shutdown => Some(ev),
                    _ => None,
                })
                .await;

            match res {
                Err(err) => {
                    info!(target: "FILE", "Error encountered while listening for file download requests: {}", err);
                }
                Ok(Event::Shutdown) => break,
                Ok(Event::MessageReceived(msg)) => match msg.body {
                    MessageBody::FileDownloadRequest {
                        file_name,
                        slice,
                        total_slices,
                        ..
                    } => {
                        let data = self.file_manifest.get_file_data(&file_name).unwrap();
                        let start_idx = (slice * (data.len() as u32) / total_slices) as usize;
                        let end_idx = ((slice + 1) * (data.len() as u32) / total_slices) as usize;
                        let response = Message::new(
                            &self.config,
                            MessageBody::FileDownloadResponse {
                                slice,
                                data: data[start_idx..end_idx].to_vec(),
                            },
                        );
                        if let Err(err) = self.sessions.send_message(&response, &msg.sender).await {
                            info!(target: "FILE", "{}", err)
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }

    async fn run_command_processing(&self) {
        loop {
            match self.read_command().await {
                Ok(cmd) => match self.execute_command(cmd).await {
                    Ok(false) => break,
                    _ => {}
                },
                Err(err) => info!(target: "FILE", "{}", err),
            };
        }
    }

    async fn read_command(&self) -> Result<Command, Box<dyn Error>> {
        let stdin = tokio::io::stdin();
        let reader = BufReader::new(stdin);

        let mut lines = reader.lines();
        while let Some(line) = lines.next_line().await? {
            let parts: Vec<&str> = line.split(' ').collect();
            match parts[0].trim() {
                "help" => return Ok(Command::Help),
                "find" if parts.len() > 1 => {
                    let min_hop_pow = parts
                        .get(2)
                        .and_then(|s| Some(s.parse::<u32>()))
                        .unwrap_or(Ok(0))?;
                    return Ok(Command::Find(
                        parts[1].trim().to_string(),
                        Some(min_hop_pow),
                    ));
                }
                "download" if parts.len() > 1 => {
                    let mut ids = Vec::new();
                    for part in parts[1..].iter() {
                        let id = part.parse::<u32>()?;
                        ids.push(id);
                    }
                    return Ok(Command::Download(ids));
                }
                "exit" => return Ok(Command::Exit),
                "adj" => return Ok(Command::Adj),
                "repair" => return Ok(Command::Repair),
                _ => return Err("Invalid command".into()),
            };
        }

        return Err("No command given".into());
    }

    async fn execute_command(&self, cmd: Command) -> Result<bool, Box<dyn Error>> {
        match cmd {
            Command::Help => Self::print_help(),
            Command::Find(file_name, min_hop) => self.find_file(file_name, min_hop).await?,
            Command::Download(ids) => self.download_files(ids).await?,
            Command::Exit => {
                self.request_exit().await?;
                return Ok(false);
            }
            Command::Adj => self.config.print_adj().await,
            Command::Repair => {
                match self.sessions.repair_connections(true).await {
                    Ok(true) => info!(target: "FILE", "Repaired all connections."),
                    Ok(false) => info!(target: "FILE", "No connections to repair."),
                    Err(err) => return Err(err),
                };
            }
        }

        return Ok(true);
    }

    fn print_help() {
        println!("{}", HELP_TEXT);
    }

    async fn find_file(
        &self,
        file_name: String,
        min_hop_pow: Option<u32>,
    ) -> Result<(), Box<dyn Error>> {
        if self.file_manifest.has_file(&file_name).await {
            info!(target: "FILE", "This node already has {}", file_name);
            return Ok(());
        }

        let mut results: Vec<FileSearchResult> = vec![];
        info!(target: "FILE", "Searching network for {}", file_name);

        let min_hop_pow_val = min_hop_pow.unwrap_or(0);
        if min_hop_pow_val > 0 {
            info!(target: "File", "Forcing search to start with a hop count of {}", (2 as u32).pow(min_hop_pow_val));
        }

        for i in min_hop_pow_val..5 {
            let hop_count = (2 as u32).pow(i);
            self.search
                .send_search_request(&file_name, &hop_count)
                .await?;
            results = self
                .wait_for_search_results(Duration::from_secs(((i + 1) * 2).into()))
                .await?;

            if results.is_empty() {
                info!(target: "FILE", "Received no search results.");
            } else {
                info!(target: "FILE", "A hop count of {} succeeded.", hop_count);
                break;
            }
        }

        if results.is_empty() {
            info!(target: "FILE", "Still no results. Giving up.");
            return Ok(());
        }

        let mut stored_results = self.last_search_result.lock().await;
        stored_results.clear();
        for result in results {
            stored_results.insert(result.get_key(), result);
        }
        drop(stored_results);

        self.print_stored_results().await;

        return Ok(());
    }

    async fn print_stored_results(&self) {
        let stored_results = self.last_search_result.lock().await;
        println!("Selectable search results:");
        for (key, val) in stored_results.iter() {
            println!("{} = (\"{}\", node {})", key, val.file_name, val.node_id);
        }
    }

    async fn wait_for_search_results(
        &self,
        duration: Duration,
    ) -> Result<Vec<FileSearchResult>, Box<dyn Error>> {
        let mut results: Vec<FileSearchResult> = Vec::new();
        let mut receiver = self.event_bus.subscribe();
        let sleep_timer = sleep(duration);
        tokio::pin!(sleep_timer);

        info!(target: "FILE", "Waiting for search results for {} seconds.", duration.as_secs());

        loop {
            tokio::select! {
                received = receiver.recv() => {
                    match received {
                        Ok(event) => match event {
                            Event::FileFound(result) => results.push(result),
                            _ => {},
                        },
                        Err(RecvError::Closed) => break,
                        _ => {},
                    }
                },
                _ = &mut sleep_timer => {
                    break;
                }
            };
        }

        return Ok(results);
    }

    async fn download_files(&self, source_nodes: Vec<u32>) -> Result<(), Box<dyn Error>> {
        let mut items: Vec<&FileSearchResult> = Vec::new();
        let last_results = self.last_search_result.lock().await;
        for id in &source_nodes {
            match last_results.get(&id.to_string()) {
                Some(item) => items.push(item),
                None => {
                    return Err(format!(
                        "Could not find any result from node {} in the list of search results",
                        id
                    )
                    .into())
                }
            }
        }

        let temp_conns = self
            .sessions
            .establish_download_streams(&source_nodes)
            .await?;

        let total_slices = items.len() as u32;
        let res = try_join!(
            self.listen_for_file_data(&total_slices),
            self.send_file_requests(&items)
        )?;

        for temp_conn in temp_conns {
            self.sessions.kill_connection(&temp_conn).await?;
        }

        self.file_manifest
            .write_file_data(&items[0].file_name, &res.0)
            .await?;

        info!(target: "FILE", "Download complete!");

        return Ok(());
    }

    async fn send_file_requests(
        &self,
        items: &Vec<&FileSearchResult>,
    ) -> Result<(), Box<dyn Error>> {
        let total_slices = items.len() as u32;
        for (slice, item) in items.iter().enumerate() {
            let msg = Message::new(
                &self.config,
                MessageBody::FileDownloadRequest {
                    file_name: item.file_name.clone(),
                    slice: slice as u32,
                    total_slices,
                },
            );
            self.sessions.send_message(&msg, &item.node_id).await?;
        }

        return Ok(());
    }

    async fn listen_for_file_data(&self, slice_count: &u32) -> Result<Vec<u8>, Box<dyn Error>> {
        let mut slices: Vec<Vec<u8>> = vec![vec![]; *slice_count as usize];
        let mut response_count = 0;
        let mut recv = self.event_bus.subscribe();
        loop {
            let (s, data) = self
                .event_bus
                .wait_for(&mut recv, |ev| match ev {
                    Event::MessageReceived(msg) => match msg.body {
                        MessageBody::FileDownloadResponse { slice, data } => Some((slice, data)),
                        _ => None,
                    },
                    _ => None,
                })
                .await?;
            slices.insert(s as usize, data);
            response_count += 1;
            if response_count == *slice_count {
                break;
            }
        }
        return Ok(slices.into_iter().flatten().collect());
    }

    async fn request_exit(&self) -> Result<(), Box<dyn Error>> {
        let mut new_adj = self.config.adj.lock().await.clone();
        new_adj.deactivate_node(self.config.id);
        if !new_adj.is_connected() {
            info!(target: "FILE", "Adj with this node removed is not connected. Will repair...");
            new_adj.repair();
        } else {
            info!(target: "FILE", "Adj with this node removed is still connected. Will not repair.");
        }

        self.sessions.broadcast_adj_update(new_adj).await?;
        self.event_bus.emit(Event::Shutdown)?;
        return Ok(());
    }
}

#[derive(Debug)]
pub enum Command {
    Help,
    Find(String, Option<u32>),
    Download(Vec<u32>),
    Exit,
    Adj,
    Repair,
}

#[derive(Clone, Debug)]
pub struct FileSearchResult {
    node_id: u32,
    file_name: String,
    file_size: u64,
}

impl FileSearchResult {
    pub fn new(message: &Message) -> Self {
        return match &message.body {
            MessageBody::SearchResponse {
                file_name,
                file_size,
                ..
            } => Self {
                node_id: message.sender,
                file_name: file_name.clone(),
                file_size: *file_size,
            },
            _ => panic!(),
        };
    }

    pub fn get_key(&self) -> String {
        return self.node_id.to_string();
    }
}
