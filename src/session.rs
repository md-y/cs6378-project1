use std::error::Error;

use bson;
use futures::{
    future::{join_all, try_join_all},
    stream::FuturesUnordered,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

use crate::{config::Config, message::Message};

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
        println!("Started listing on: {}", socket_addr);

        let (mut stream, _) = listener.accept().await?;
        let mut conn = Connection::new(stream);
        let msg = conn.read_message().await?;
        match msg {
            Message::Init { sender } => println!("Received message from {}", sender),
        }

        Ok(())
    }

    async fn init_connections(&self) -> Result<(), Box<dyn Error>> {
        let nodes = self.config.get_nodes_to_connect();
        let tasks = nodes.iter().map(|node| self.init_connection(*node));
        try_join_all(tasks).await?;
        return Ok(());
    }

    async fn init_connection(&self, node: u32) -> Result<(), Box<dyn Error>> {
        let addr = self.config.resolve_route(node);
        println!("Connecting to node {} ({})", node, addr);
        let stream = TcpStream::connect(addr).await?;
        let mut conn = Connection::new(stream);
        let msg = Message::Init {
            sender: self.config.id,
        };
        conn.write_message(msg).await?;
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
