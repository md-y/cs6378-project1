use std::{collections::HashMap, error::Error, time::Duration};

use bson;
use futures::future::join_all;
use tokio::{
    io::{self, AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Mutex,
    time::sleep,
};

use crate::{config::Config, message::Message, message::MessageBody};

pub struct SessionLayer<'a> {
    config: &'a Config,
}

impl<'a> SessionLayer<'a> {
    pub fn new(config: &'a Config) -> SessionLayer<'a> {
        return SessionLayer { config };
    }

    pub async fn run(&self) -> Result<(), Box<dyn Error>> {
        let conn_manager = ConnectionManager::new(self.config, self);
        conn_manager.fork().await?;
        return Ok(());
    }

    fn on_required_connections_established(&self) {
        println!(
            "[Session Layer] Ready! All required connections for the P2P network have been made."
        );
    }
}

struct ConnectionManager<'a> {
    connections: Mutex<HashMap<u32, Connection>>,
    config: &'a Config,
    session_layer: &'a SessionLayer<'a>,
}

impl<'a> ConnectionManager<'a> {
    pub fn new(config: &'a Config, session_layer: &'a SessionLayer<'a>) -> Self {
        return Self {
            connections: Mutex::new(HashMap::new()),
            config,
            session_layer,
        };
    }

    pub async fn add_connection(&self, node_id: u32, conn: Connection) {
        let mut conns = self.connections.lock().await;
        println!(
            "[Session Layer] Connection established with node {}",
            node_id
        );
        conns.insert(node_id, conn);

        let (outgoing, incoming) = self.config.get_nodes_to_connect();
        let mut required = outgoing.iter().chain(incoming.iter());
        if required.all(|n| conns.contains_key(n)) {
            self.session_layer.on_required_connections_established();
        }
    }

    pub async fn remove_connection(&self, node_id: u32) {
        let mut conns = self.connections.lock().await;
        conns.remove(&node_id);
    }

    pub async fn fork(&self) -> Result<(), Box<dyn Error>> {
        tokio::try_join!(self.send_requests(), self.listen_requests())?;
        return Ok(());
    }

    async fn send_requests(&self) -> Result<(), Box<dyn Error>> {
        let nodes = self.config.get_nodes_to_connect().0;

        if nodes.is_empty() {
            println!(
                "[Session Layer] This node isn't setup to make any connection requests, so it'll just listen."
            );
            return Ok(());
        }

        println!(
            "[Session Layer] This node will connect to {} nodes: {:?}",
            nodes.len(),
            nodes
        );

        let tasks = nodes.iter().map(|node| self.connect_to(*node));
        join_all(tasks).await;
        return Ok(());
    }

    pub async fn connect_to(&self, node_id: u32) -> Result<(), Box<dyn Error>> {
        let stream = establish_stream(self.config, node_id).await;
        let mut conn = Connection::new(stream);

        let msg = Message::new(&self.config, MessageBody::InitRequest);
        conn.write_message(msg).await?;

        let msg = conn.read_message().await?;
        match msg.body {
            MessageBody::InitResponse => {
                self.add_connection(node_id, conn).await;
                return Ok(());
            }
            _ => {
                return Err(format!("Node {}'s response wasn't Init Response.", node_id).into());
            }
        };
    }

    async fn listen_requests(&self) -> Result<(), Box<dyn Error>> {
        let socket_addr = self.config.get_listen_address();
        let listener = TcpListener::bind(&socket_addr).await?;
        println!("[Session Layer] Started listing on: {}", socket_addr);

        loop {
            let (stream, incoming_addr) = listener.accept().await?;
            let res = self.handle_incoming_request(stream).await;
            if let Err(err) = res {
                eprintln!(
                    "[Session Layer] Ignoring incoming request from {} because: {}",
                    incoming_addr, err
                );
            }
        }
    }

    async fn handle_incoming_request(&self, stream: TcpStream) -> Result<(), Box<dyn Error>> {
        let mut conn = Connection::new(stream);
        let msg = conn.read_message().await?;

        let is_init = match msg.body {
            MessageBody::InitRequest => true,
            _ => false,
        };
        if !is_init {
            return Err(String::from("Initial message was not a connection init request.").into());
        }

        let res_msg = Message::new(&self.config, MessageBody::InitResponse);
        conn.write_message(res_msg).await?;

        self.add_connection(msg.sender, conn).await;
        return Ok(());
    }
}

struct Connection {
    stream: TcpStream,
}

impl Connection {
    pub fn new(stream: TcpStream) -> Self {
        return Self { stream };
    }

    pub async fn write_message(&mut self, message: Message) -> Result<(), Box<dyn Error>> {
        let bytes = bson::serialize_to_vec(&message)?;
        self.stream.write_all(&bytes).await?;
        return Ok(());
    }

    pub async fn read_message(&mut self) -> Result<Message, Box<dyn Error>> {
        let mut len_buf = [0u8; 4];
        self.stream.read_exact(&mut len_buf).await?;

        // BSON uses little endian
        let len = i32::from_le_bytes(len_buf);

        let mut doc_buf = vec![0u8; len as usize];
        doc_buf[0..4].copy_from_slice(&len_buf);
        self.stream.read_exact(&mut doc_buf[4..]).await?;

        let msg: Message = bson::deserialize_from_reader(&doc_buf[..])?;
        return Ok(msg);
    }
}

async fn establish_stream(config: &Config, node: u32) -> TcpStream {
    let addr = config.resolve_route(node);
    let mut delay: u64 = 1;
    loop {
        if delay > 1 {
            println!(
                "[Session Layer] Waiting {} seconds before trying again...",
                delay
            );
            sleep(Duration::from_secs(delay)).await;
        }
        delay *= 2;
        println!(
            "[Session Layer] Attempting to connect to node {} ({})",
            node, addr
        );
        match TcpStream::connect(addr.clone()).await {
            Ok(stream) => return stream,
            Err(err) => {
                eprintln!(
                    "[Session Layer] Failed to connect to node {} ({}). {}",
                    node, addr, err
                );
            }
        }
    }
}
