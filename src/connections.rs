use std::{collections::HashMap, error::Error, sync::Arc, time::Duration};

use bson;
use futures::future::join_all;
use log::{error, info};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{
        broadcast::{Sender},
        Mutex, MutexGuard,
    },
    time::sleep,
};

use crate::{
    bus::{Event, EventBus},
    config::Config,
    message::{Message, MessageBody},
};

pub struct ConnectionManager {
    connections: Mutex<HashMap<u32, Arc<Connection>>>,
    config: Arc<Config>,
    event_bus: Arc<EventBus>,
}

impl ConnectionManager {
    pub async fn fork(&self) -> Result<(), Box<dyn Error>> {
        tokio::try_join!(self.send_conn_requests(), self.listen_conn_requests())?;
        return Ok(());
    }

    pub fn new(config: Arc<Config>, event_bus: Arc<EventBus>) -> Self {
        return Self {
            connections: Mutex::new(HashMap::new()),
            config,
            event_bus,
        };
    }

    pub async fn broadcast(
        &self,
        message: &Message,
        targets: &Vec<u32>,
    ) -> Vec<Result<(), Box<dyn Error>>> {
        let tasks = targets.iter().map(async |t| -> Result<(), Box<dyn Error>> {
            let conn = self.get_connection(t).await?;
            conn.write_message(message).await?;
            return Ok(());
        });
        return join_all(tasks).await;
    }

    pub async fn send_message(
        &self,
        message: &Message,
        target: &u32,
    ) -> Result<(), Box<dyn Error>> {
        let conn = self.get_connection(target).await?;
        conn.write_message(message).await?;
        return Ok(());
    }

    async fn get_connection(&self, node_id: &u32) -> Result<Arc<Connection>, Box<dyn Error>> {
        let conns = self.connections.lock().await;
        return match conns.get(node_id) {
            None => Err(format!("Could not get connection for node {}", node_id).into()),
            Some(conn) => Ok(conn.clone()),
        };
    }

    async fn add_connection(&self, node_id: u32, conn: Connection) -> Result<(), Box<dyn Error>> {
        let mut conns = self.connections.lock().await;
        info!(target: "SESSION", "Connection established with node {}", node_id);
        conns.insert(node_id, Arc::new(conn));
        drop(conns);

        self.start_conn_worker(node_id).await?;
        self.event_bus.emit(Event::NewConnection(node_id))?;

        return Ok(());
    }

    async fn start_conn_worker(&self, node_id: u32) -> Result<(), Box<dyn Error>> {
        let conn = self.get_connection(&node_id).await?;
        let new_conn = conn.clone();
        tokio::spawn(async move {
            new_conn.run_worker().await;
        });

        return Ok(());
    }

    async fn remove_connection(&self, node_id: u32) {
        let mut conns = self.connections.lock().await;
        conns.remove(&node_id);
    }

    async fn send_conn_requests(&self) -> Result<(), Box<dyn Error>> {
        let nodes = self.config.get_nodes_to_connect().0;

        if nodes.is_empty() {
            info!(target: "SESSION", "This node isn't setup to make any connection requests, so it'll just listen.");
            return Ok(());
        }

        info!(target: "SESSION",
            "This node will connect to {} nodes: {:?}",
            nodes.len(),
            nodes
        );

        let tasks = nodes.iter().map(|node| self.connect_to(*node));
        join_all(tasks).await;
        return Ok(());
    }

    pub async fn connect_to(&self, node_id: u32) -> Result<(), Box<dyn Error>> {
        let stream = establish_stream(&self.config, node_id).await;
        let conn = Connection::new(stream, self.event_bus.clone());

        let msg = Message::new(&self.config, MessageBody::InitRequest);
        conn.write_message(&msg).await?;

        let msg = conn.read_message().await?;
        match msg.body {
            MessageBody::InitResponse => {
                self.add_connection(node_id, conn).await?;
                return Ok(());
            }
            _ => {
                return Err(format!("Node {}'s response wasn't Init Response.", node_id).into());
            }
        };
    }

    async fn listen_conn_requests(&self) -> Result<(), Box<dyn Error>> {
        let socket_addr = self.config.get_listen_address();
        let listener = TcpListener::bind(&socket_addr).await?;
        info!(target: "SESSION", "Started listing on: {}", socket_addr);

        loop {
            let (stream, incoming_addr) = listener.accept().await?;
            let res = self.handle_incoming_request(stream).await;
            if let Err(err) = res {
                error!(
                    target: "SESSION",
                    "Ignoring incoming request from {} because: {}",
                    incoming_addr, err
                );
            }
        }
    }

    async fn handle_incoming_request(&self, stream: TcpStream) -> Result<(), Box<dyn Error>> {
        let conn = Connection::new(stream, self.event_bus.clone());
        let msg = conn.read_message().await?;

        let is_init = match msg.body {
            MessageBody::InitRequest => true,
            _ => false,
        };
        if !is_init {
            return Err(String::from("Initial message was not a connection init request.").into());
        }

        let res_msg = Message::new(&self.config, MessageBody::InitResponse);
        conn.write_message(&res_msg).await?;

        self.add_connection(msg.sender, conn).await?;
        return Ok(());
    }
}

pub struct Connection {
    stream: Mutex<TcpStream>,
    event_bus: Arc<EventBus>,
    interrupt_bus: Sender<()>,
}

impl Connection {
    pub fn new(stream: TcpStream, event_bus: Arc<EventBus>) -> Self {
        return Self {
            stream: Mutex::new(stream),
            event_bus,
            interrupt_bus: tokio::sync::broadcast::channel(64).0,
        };
    }

    pub async fn write_message(&self, message: &Message) -> Result<(), Box<dyn Error>> {
        let bytes = bson::to_vec(&message)?;
        match self.interrupt_bus.send(()) {
            Err(err) => eprintln!("{}", err),
            Ok(_) => {}
        }
        let mut stream = self.get_stream_with_priority().await;
        stream.write_all(&bytes).await?;
        info!(target: "SESSION", "Sent message: {}", message);
        return Ok(());
    }

    async fn get_stream_with_priority(&self) -> MutexGuard<'_, TcpStream> {
        match self.stream.try_lock() {
            Ok(res) => return res,
            Err(_) => {
                self.interrupt_bus.send(()).unwrap();
                return self.stream.lock().await;
            }
        }
    }

    pub async fn read_message(&self) -> Result<Message, Box<dyn Error>> {
        let mut rx = self.interrupt_bus.subscribe();
        let mut stream = self.stream.lock().await;
        tokio::select! {
            res = Self::read_message_from_stream(&mut stream) => return res,
            Ok(()) = rx.recv() => return Err("Stopped listening to messages so we can send a message".into()),
        }
    }

    async fn read_message_from_stream(stream: &mut TcpStream) -> Result<Message, Box<dyn Error>> {
        let mut len_buf = [0u8; 4];
        stream.read_exact(&mut len_buf).await?;

        // BSON uses little endian
        let len = i32::from_le_bytes(len_buf);

        let mut doc_buf = vec![0u8; len as usize];
        doc_buf[0..4].copy_from_slice(&len_buf);
        stream.read_exact(&mut doc_buf[4..]).await?;

        let msg: Message = bson::from_reader(&doc_buf[..])?;
        info!(target: "SESSION", "Read message: {}", msg);
        return Ok(msg);
    }

    pub async fn run_worker(&self) {
        loop {
            info!(target: "SESSION", "Listening for more messages...");
            match self.read_message().await {
                Err(err) => {
                    let err_str = format!("Failed in read message: {}", err);
                    if err_str.contains("early eof") {
                        // TODO: Make this better for part 3
                        info!(target: "SESSION", "The network has been broken, shutting down. Graceful handling will happen in part 3...");
                        std::process::exit(1);
                    }
                    info!(target: "SESSION", "{}", err_str);
                }
                Ok(msg) => match self.event_bus.emit(Event::MessageReceived(msg)) {
                    Err(err) => {
                        info!(target: "SESSION", "Failed emit event for read message: {}", err);
                    }
                    Ok(_) => {}
                },
            }
        }
    }
}

async fn establish_stream(config: &Config, node: u32) -> TcpStream {
    let addr = config.resolve_route(node);
    let mut delay: u64 = 1;
    loop {
        if delay > 1 {
            info!(target: "SESSION", "Waiting {} seconds before trying again...", delay);
            sleep(Duration::from_secs(delay)).await;
        }
        delay *= 2;
        info!(target: "SESSION", "Attempting to connect to node {} ({})", node, addr);
        match TcpStream::connect(addr.clone()).await {
            Ok(stream) => return stream,
            Err(err) => {
                error!(target: "SESSION", "Failed to connect to node {} ({}). {}", node, addr, err);
            }
        }
    }
}
