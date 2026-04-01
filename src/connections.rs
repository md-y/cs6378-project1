use std::{collections::HashMap, error::Error, fmt, io::ErrorKind, sync::Arc, time::Duration};

use bson;
use futures::future::join_all;
use log::{error, info};
use tokio::{
    io::{self, AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{Mutex, MutexGuard, broadcast::Sender},
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

    pub async fn has_connection(&self, node_id: &u32) -> bool {
        let conns = self.connections.lock().await;
        return conns.contains_key(node_id);
    }

    pub async fn get_connection(&self, node_id: &u32) -> Result<Arc<Connection>, Box<dyn Error>> {
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
        let stream = Self::establish_stream(&self.config, node_id).await;
        let addr = self.config.resolve_route(node_id);
        let conn = Connection::new(stream, addr, self.event_bus.clone());

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
            let res = self.handle_incoming_request(stream, incoming_addr.to_string()).await;
            if let Err(err) = res {
                error!(
                    target: "SESSION",
                    "Ignoring incoming request from {} because: {}",
                    incoming_addr, err
                );
            }
        }
    }

    async fn handle_incoming_request(&self, stream: TcpStream, addr: String) -> Result<(), Box<dyn Error>> {
        let conn = Connection::new(stream, addr, self.event_bus.clone());
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

    pub async fn kill_connection(&self, node_id: &u32) -> Result<(), Box<dyn Error>> {
        let mut conns = self.connections.lock().await;
        match conns.remove(node_id) {
            Some(conn) => drop(conn),
            None => return Err(format!("Could not find connection with {}", node_id).into()),
        }
        info!(target: "SESSION",
            "Killed connection with {}",
            node_id
        );
        return Ok(());
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
}

pub struct Connection {
    stream: Mutex<TcpStream>,
    event_bus: Arc<EventBus>,
    interrupt_bus: Sender<ConnectionInterruptEvent>,
    addr: String,
}

impl Connection {
    pub fn new(stream: TcpStream, addr: String, event_bus: Arc<EventBus>) -> Self {
        return Self {
            stream: Mutex::new(stream),
            addr,
            event_bus,
            interrupt_bus: tokio::sync::broadcast::channel(64).0,
        };
    }

    pub async fn kill_worker(&self) -> Result<(), Box<dyn Error>> {
        self.interrupt_bus.send(ConnectionInterruptEvent::Kill)?;
        return Ok(());
    }

    pub async fn write_message(&self, message: &Message) -> Result<(), Box<dyn Error>> {
        let bytes = bson::to_vec(&message)?;
        let mut stream = self.get_stream_with_priority().await;
        stream.write_all(&bytes).await?;
        info!(target: "SESSION", "Sent message to {}: {}", self.addr, message);
        return Ok(());
    }

    async fn get_stream_with_priority(&self) -> MutexGuard<'_, TcpStream> {
        match self.stream.try_lock() {
            Ok(res) => return res,
            Err(_) => {
                self.interrupt_bus
                    .send(ConnectionInterruptEvent::AllowWrite)
                    .unwrap();
                return self.stream.lock().await;
            }
        }
    }

    pub async fn read_message(&self) -> Result<Message, ConnectionError> {
        let mut rx = self.interrupt_bus.subscribe();
        let mut stream = self.stream.lock().await;
        tokio::select! {
            res = self.read_message_from_stream(&mut stream) => return res,
            Ok(ev) = rx.recv() => return Err(ev.into()),
        }
    }

    async fn read_message_from_stream(&self, stream: &mut TcpStream) -> Result<Message, ConnectionError> {
        let mut len_buf = [0u8; 4];
        stream.read_exact(&mut len_buf).await?;

        // BSON uses little endian
        let len = i32::from_le_bytes(len_buf);

        let mut doc_buf = vec![0u8; len as usize];
        doc_buf[0..4].copy_from_slice(&len_buf);
        stream.read_exact(&mut doc_buf[4..]).await?;

        let msg: Message = bson::from_reader(&doc_buf[..])?;
        info!(target: "SESSION", "Read message from {}: {}", self.addr, msg);
        return Ok(msg);
    }

    pub async fn run_worker(&self) {
        info!(target: "SESSION", "Started worker thread for connection to {}", self.addr);
        loop {
            
            match self.read_message().await {
                Ok(msg) => match self.event_bus.emit(Event::MessageReceived(msg)) {
                    Err(err) => {
                        info!(target: "SESSION", "Failed emit event for read message from {}. Effectively discarding it. {}", self.addr, err);
                    }
                    Ok(_) => {}
                },
                Err(err) => match err {
                    ConnectionError::Interrupt(ev) => match ev {
                        ConnectionInterruptEvent::Kill => {
                            info!(target: "SESSION", "Stopped connection worker for connection to {}", self.addr)
                        }

                        ConnectionInterruptEvent::AllowWrite => {
                            info!(target: "SESSION", "Yielding connection worker to allow message write to {}", self.addr)
                        }
                    },
                    ConnectionError::Deserialization(err) => {
                        info!(target: "SESSION", "Failed to deserialize message from {}: {}", self.addr, err)
                    }
                    ConnectionError::Read(io_err) => match io_err.kind() {
                        ErrorKind::UnexpectedEof => {
                            info!(target: "SESSION", "The connection to {} has been closed, shutting down. Graceful handling will happen in part 3...", self.addr);
                            std::process::exit(1);
                        }
                        _ => info!(target: "SESSION", "Failed to read message from {}: {}", self.addr, io_err),
                    },
                },
            }
        }
    }
}

#[derive(Clone, Debug)]
pub enum ConnectionInterruptEvent {
    AllowWrite,
    Kill,
}

#[derive(Debug)]
pub enum ConnectionError {
    Read(io::Error),
    Deserialization(bson::de::Error),
    Interrupt(ConnectionInterruptEvent),
}

impl From<io::Error> for ConnectionError {
    fn from(err: io::Error) -> Self {
        ConnectionError::Read(err)
    }
}

impl From<bson::de::Error> for ConnectionError {
    fn from(err: bson::de::Error) -> Self {
        ConnectionError::Deserialization(err)
    }
}

impl From<ConnectionInterruptEvent> for ConnectionError {
    fn from(ev: ConnectionInterruptEvent) -> Self {
        ConnectionError::Interrupt(ev)
    }
}

impl fmt::Display for ConnectionError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            _ => write!(f, "{}", self),
        }
    }
}

impl std::error::Error for ConnectionError {}
