use futures::lock::Mutex;
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
    sync::broadcast::error::RecvError,
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
            .wait_for(|ev| match ev {
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
        loop {
            let res = self
                .event_bus
                .wait_for(|ev| match ev {
                    Event::MessageReceived(msg) => match msg.body {
                        MessageBody::FileDownloadRequest { .. } => Some(msg),
                        _ => None,
                    },
                    _ => None,
                })
                .await;

            if let Ok(msg) = res {
                match msg.body {
                    MessageBody::FileDownloadRequest {
                        file_name, slice, ..
                    } => {
                        let response = Message::new(
                            &self.config,
                            MessageBody::FileDownloadResponse {
                                slice,
                                data: self
                                    .file_manifest
                                    .get_file_data(self.config.get_file_dir(), &file_name)
                                    .unwrap(),
                            },
                        );
                        if let Err(err) = self.sessions.send_message(&response, &msg.sender).await {
                            info!(target: "FILE", "{}", err)
                        }
                    }
                    _ => panic!(),
                }
            }
        }
    }

    async fn run_command_processing(&self) {
        loop {
            match self.read_command().await {
                Ok(cmd) => match self.execute_command(cmd).await {
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
                "find" if parts.len() > 1 => return Ok(Command::Find(parts[1].trim().to_string())),
                "download" if parts.len() > 1 => {
                    let mut ids = Vec::new();
                    for part in parts[1..].iter() {
                        let id = part.parse::<u32>()?;
                        ids.push(id);
                    }
                    return Ok(Command::Download(ids));
                }
                _ => return Err("Invalid command".into()),
            };
        }

        return Err("No command given".into());
    }

    async fn execute_command(&self, cmd: Command) -> Result<(), Box<dyn Error>> {
        match cmd {
            Command::Help => Self::print_help(),
            Command::Find(file_name) => self.find_file(file_name).await?,
            Command::Download(ids) => self.download_files(ids).await?,
        }

        return Ok(());
    }

    fn print_help() {
        println!("{}", HELP_TEXT);
    }

    async fn find_file(&self, file_name: String) -> Result<(), Box<dyn Error>> {
        if self.file_manifest.has_file(&file_name).await {
            info!(target: "FILE", "This node already has {}", file_name);
            return Ok(());
        }

        let mut results: Vec<FileSearchResult> = vec![];
        info!(target: "FILE", "Searching network for {}", file_name);

        for i in 0..5 {
            let hop_count = (2 as u32).pow(i);
            self.search
                .send_search_request(&file_name, &hop_count)
                .await?;
            results = self
                .wait_for_search_results(Duration::from_secs(i.into()))
                .await?;

            if results.is_empty() {
                info!(target: "FILE", "Received no search results.");
            } else {
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

        self.file_manifest.write_file_data(self.config.get_file_dir(), &items[0].file_name, &res.0).await?;

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
        let mut slices: Vec<Vec<u8>> = Vec::new();
        let mut response_count = 0;
        loop {
            let (s, data) = self
                .event_bus
                .wait_for(|ev| match ev {
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
}

pub enum Command {
    Help,
    Find(String),
    Download(Vec<u32>),
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
