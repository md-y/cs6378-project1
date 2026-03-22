use std::{error::Error, thread::sleep, time::Duration};

use bson;
use futures::{
    future::{try_join_all},
};
use tokio::{
    io::{self, AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

use crate::{config::Config, message::Message, message::MessageBody};

const MAX_CONN_RETRIES: usize = 5;

pub struct SessionLayer<'a> {
    config: &'a Config,
}

impl<'a> SessionLayer<'a> {
    pub fn new(config: &'a Config) -> SessionLayer<'a> {
        return SessionLayer { config };
    }

    pub async fn run(&self) -> Result<(), Box<dyn Error>> {
        tokio::try_join!(self.listen(), self.init_connections())?;
        return Ok(());
    }

    async fn listen(&self) -> Result<(), Box<dyn Error>> {
        let socket_addr = self.config.get_listen_address();
        let listener = TcpListener::bind(&socket_addr).await?;
        println!("[Session Layer] Started listing on: {}", socket_addr);

        loop {
            let (stream, _) = listener.accept().await?;
            let mut conn = Connection::new(stream);
            let msg = conn.read_message().await?;
            self.handle_message(&mut conn, msg).await?;
        }
    }

    async fn init_connections(&self) -> Result<(), Box<dyn Error>> {
        let nodes = self.config.get_nodes_to_connect();
        if nodes.is_empty() {
            println!("[Session Layer] This node is not configured to initialize any connections, so it will wait.");
            return Ok(());
        } else {
            println!("[Session Layer] This node will initialize connections with {} nodes: {:?}", nodes.len(), nodes);
        }

        let tasks = nodes.iter().map(|node| self.init_connection(*node));
        try_join_all(tasks).await?;
        println!("[Session Layer] This node is ready!");
        return Ok(());
    }

    async fn init_connection(&self, node: u32) -> Result<(), Box<dyn Error>> {
        let stream = self.establish_stream(node).await?;
        let mut conn = Connection::new(stream);
        let msg = Message::new(self.config, MessageBody::InitRequest);
        conn.write_message(msg).await?;
        let msg = conn.read_message().await?;
        println!("[Session Layer] Connection established with node {}", msg.sender);
        return Ok(());
    }

    async fn establish_stream(&self, node: u32) -> Result<TcpStream, io::Error> {
        let addr = self.config.resolve_route(node);
        let mut last_err: Option<io::Error> = None;
        for i in 0..MAX_CONN_RETRIES {
            if i > 0 {
                let delay = i.pow(2) as u64;
                println!("[Session Layer] Waiting {} seconds before trying again...", delay);
                sleep(Duration::from_secs(delay));
            }
            let addr_copy = addr.clone();
            println!("[Session Layer] Attempting to connect to node {} ({})", node, addr_copy);
            match TcpStream::connect(addr_copy).await {
                Ok(stream) => return Ok(stream),
                Err(err) => {
                    eprintln!("[Session Layer] Failed to connect to node {}", node);
                    last_err = Some(err);
                }
            }
        }
        eprintln!("[Session Layer] Maximum connection reties reached. Stopping...");
        let err = last_err.unwrap_or_else(|| io::Error::new(io::ErrorKind::Other, "Unknown Error"));
        return Err(err);
    }

    async fn handle_message(&self, conn: &mut Connection, msg: Message) -> Result<(), Box<dyn Error>> {
        println!("[Session Layer] Received message: {}", msg);
        match msg.body {
            MessageBody::InitRequest => conn.write_message(Message::new(self.config, MessageBody::InitResponse)).await?,
            _ => (),
        }
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
