use std::{collections::HashMap, net::SocketAddr, path::PathBuf, sync::Arc};

use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use log::{error, info, warn};
use rstr_core::{message::{BBytes, BinaryMessage, LoginData, LoginType, MessagePayload, NotifyUpdatedData, RemoveMetaData, RequestChunkData, RequestMetaData, TransmitChunkData, TransmitMetaData, UserStatus}, meta::{Meta, MetaIndex}};
use tokio::{net::TcpStream, sync::{mpsc, Mutex}};
use tokio_tungstenite::tungstenite::{self, Message};
use tokio_util::{sync::CancellationToken};


#[derive(Debug)]
pub enum NewServerError {
    MetaReadError
}

#[derive(Debug)]
pub enum HandlerError {
    TungsteniteError(tungstenite::error::Error),
    DecodeError(bincode::error::DecodeError),
    EncodeError(bincode::error::EncodeError),
    ServerHandlerError,
    IoError,
    Unsupported,
    ClientNotFound,
    ClientOccupied,
    InvalidToken,
    UnknownMessage
}

pub enum ServerMessage {
    ClientConnected( Client ),
    Message( MessageFromClient ),
    ClientDisconnected( SocketAddr )
}

pub struct MessageFromClient {
    addr: SocketAddr,
    message: BinaryMessage
}

#[derive(Clone, PartialEq)]
pub enum ClientType {
    Unidentified,
    Receiver,
    Sender
}

#[derive(Clone)]
pub struct Client {
    addr: SocketAddr,
    client_type: ClientType,
    outgoing: mpsc::Sender<BinaryMessage>,
    cancellation_token: CancellationToken,
    //incoming: mpsc::Receiver<BinaryMessage>
}

pub struct Server {
    data_dir: PathBuf,
    message_rx: mpsc::Receiver<ServerMessage>,
    clients: HashMap<SocketAddr, Arc<Mutex<Client>>>,
    bincode_config: bincode::config::Configuration,
    index: MetaIndex,

    receiver_token: String,
    sender_token: String
}

impl Server {
    pub async fn new(
        data_dir: &PathBuf, 
        message_rx: mpsc::Receiver<ServerMessage>, 
        bincode_config: &bincode::config::Configuration
    ) -> Result<Self, NewServerError> {
        let index = MetaIndex::load(data_dir, bincode_config).await.map_err(|_| NewServerError::MetaReadError)?;

        let receiver_token = match std::env::var("RSTR_RECEIVER_TOKEN") {
            Ok(token) => token,
            Err(_) => "h87s8ghegh48ghs4gs84hg8s4h8".to_owned()
        };

        let sender_token = match std::env::var("RSTR_SENDER_TOKEN") {
            Ok(token) => token,
            Err(_) => "vn753498573q0v5983n5789qyunasp8fy3j".to_owned()
        };

        let server = Server {
            data_dir: data_dir.to_owned(),
            message_rx: message_rx,
            clients: HashMap::new(),
            bincode_config: *bincode_config,
            index: index,
            receiver_token: receiver_token,
            sender_token: sender_token
        };

        Ok(server)
    }

    pub async fn handle_clients(&mut self) {
        loop {
            let server_message = match self.message_rx.recv().await {
                Some(message) => message,
                None => return
            };

            match server_message {
                ServerMessage::ClientConnected(client) => {
                    let addr = client.addr;
                    let client = Arc::new(Mutex::new(client));

                    self.clients.insert(addr, client);
                },
                ServerMessage::Message(message_from_client) => {
                    let addr = message_from_client.addr;
                    let client = match self.clients.get(&addr) {
                        Some(client) => client,
                        None => {
                            warn!("Unable to find a client with address {}", addr);
                            continue;
                        }
                    }.lock().await;

                    let client_copy = client.clone();
                    drop(client);

                    let binary_message = message_from_client.message;

                    match self.handle_message(&client_copy, binary_message.payload()).await {
                        Err(error) => {
                            error!("Error occured while processing a message from a client: {:?}", error);
                            client_copy.cancellation_token.cancel();
                        },
                        _ => {}
                    }
                },
                ServerMessage::ClientDisconnected(addr) => {
                    let client = match self.clients.get(&addr) {
                        Some(client) => client,
                        None => {
                            warn!("Unable to find a client with address {}", addr);
                            continue;
                        }
                    }.lock().await;

                    let client_type = client.client_type.clone();
                    drop(client);

                    if client_type == ClientType::Sender {
                        let _ = self.notify_sender_status(UserStatus::Disconnected).await;
                    }

                    info!("Client with address {} disconnected", addr);
                    
                    self.clients.remove(&addr);   
                }
            }
        }
    }

    pub async fn handle_connection(
        raw_stream: TcpStream, 
        addr: SocketAddr, 
        message_tx: mpsc::Sender<ServerMessage>,
        bincode_config: bincode::config::Configuration,
    ) {
        info!("Incoming TCP connection from: {}", addr);

        let ws_stream = match tokio_tungstenite::accept_async(raw_stream).await {
            Ok(ws_stream) => ws_stream,
            Err(_) => {
                warn!("WebSocket handshake failed");
                return;
            },
        };

        let (mut outgoing, mut incoming) = ws_stream.split();
        let (outgoing_tx, mut outgoing_rx) = mpsc::channel(1024*1024);

        let cancellation_token = CancellationToken::new();
        let client = Client { addr, client_type: ClientType::Unidentified, outgoing: outgoing_tx, cancellation_token: cancellation_token.clone() };

        let task_message_tx = message_tx.clone();

        match &message_tx.send(ServerMessage::ClientConnected(client)).await {
            Ok(_) => {},
            Err(_) => {
                error!("Unable to notify the server of a new client. Quitting");
                return;
            },
        }

        let config = bincode_config.clone();

        let incoming_task = async move {
            loop {
                let message = match incoming.next().await {
                    None => { return Ok(()); }
                    Some(m) => m
                };

                let data = match message {
                    Ok(message) => { message.into_data() },
                    Err(error) => {
                        return Err(HandlerError::TungsteniteError(error));
                    }
                };

                let (binary_message, _): (BinaryMessage, usize) = match bincode::decode_from_slice(&data, config) {
                    Ok((b, size)) => (b, size),
                    Err(error) => {
                        return Err(HandlerError::DecodeError(error));
                    },
                };

                let boxed_message = ServerMessage::Message(MessageFromClient { addr, message: binary_message });

                task_message_tx.send(boxed_message).await.unwrap();
            }
        };

        let outgoing_task = async move {
            loop {
                let binary_message = match outgoing_rx.recv().await {
                    Some(binary_message) => binary_message,
                    None => {
                        info!("No more messages from the client to process");
                        break;
                    }
                };

                let vec = bincode::encode_to_vec(binary_message, config).unwrap();
                let bytes = BBytes(Bytes::from(vec));

                let message = Message::binary(bytes.0);

                match outgoing.send(message).await {
                    Err(error) => { return Err(HandlerError::TungsteniteError(error));}
                    _ => {}
                }
            }

            return Ok(())
        };

        tokio::select!{
            it = incoming_task => {
                match it {
                    Err(err) => {
                        error!("Error occurred in incomming task for address {}: {:?}", addr, err);
                    },
                    _ => {
                        warn!("Incoming task is cancelled for address {}", addr);
                    }
                }
            },
            ot = outgoing_task => {
                match ot {
                    Err(err) => {
                        error!("Error occurred in outgoing task for address {}: {:?}", addr, err);
                    },
                    _ => {
                        warn!("Outgoing task is cancelled for address {}", addr);
                    }
                }
            },
            _ = cancellation_token.cancelled() => {
                error!("Connection closed by cancellation token for address {}", addr);
            }
        };

        match message_tx.send(ServerMessage::ClientDisconnected(addr)).await {
            Ok(_) => {},
            Err(_) => {
                error!("Unable to notify the server of a disconnected client for address {}", addr);
            },
        }
    }

    async fn notify_sender_status(&mut self, status: UserStatus) -> Result<(), HandlerError> {
        let receiver = self.get_by_type(&ClientType::Receiver).await?;
        receiver.outgoing
            .send(BinaryMessage::new(MessagePayload::NotifySenderStatus(status)))
            .await.map_err(|_| HandlerError::IoError)?;

        Ok(())
    }

    async fn handle_message(&mut self, client: &Client, message: &MessagePayload) -> Result<(), HandlerError> {
        return match message {
            MessagePayload::Unknown => Err(HandlerError::UnknownMessage),
            MessagePayload::Error(data) => { 
                warn!("Client sent a error: {}", data); Ok(())
            },
            MessagePayload::Login(data) => self.handle_login(client, data).await,
            MessagePayload::PublishMeta(data) => self.handle_publish_meta(client, data).await,
            MessagePayload::RemoveMeta(data) => self.handle_remove_meta(client, data).await,
            MessagePayload::RequestMeta(data) => self.handle_request_meta(client, data).await,
            MessagePayload::RequestChunk(data) => self.handle_request_chunk(client, data).await,
            MessagePayload::TransmitChunk(data) => self.handle_transmit_chunk(client, data).await,
            MessagePayload::StopChunk => self.handle_stop_chunk(client).await,

            MessagePayload::NotifySenderStatus(_) => Err(HandlerError::Unsupported),
            MessagePayload::NotifyUpdated(_) => Err(HandlerError::Unsupported),
            MessagePayload::TransmitMeta(_) => Err(HandlerError::Unsupported),
            MessagePayload::LoginSuccess => Err(HandlerError::Unsupported)
        };
    }

    async fn handle_login(&mut self, client: &Client, data: &LoginData) -> Result<(), HandlerError> {
        Server::unidentified(client)?;

        info!("Client {} is logging in", client.addr);

        let matched_type = match data.login_type {
            LoginType::Receiver => {
                if data.key != self.receiver_token {
                    return Err(HandlerError::InvalidToken)
                }

                ClientType::Receiver
            },
            LoginType::Sender => {
                if data.key != self.sender_token {
                    return Err(HandlerError::InvalidToken)
                }
                
                ClientType::Sender
            }
        };

        match self.get_by_type(&matched_type).await {
            Ok(_) => { return Err(HandlerError::ClientOccupied) },
            _ => {}
        }

        let addr = client.addr;
        let mut client = self.clients.get(&addr).unwrap().lock().await;
        client.client_type = matched_type.clone();

        client.outgoing.send(BinaryMessage::new(MessagePayload::LoginSuccess)).await.map_err(|_| HandlerError::IoError)?;

        drop(client);

        match matched_type {
            ClientType::Receiver => {
                info!("A receiver at {} has connected", addr);

                if self.exists_by_type(&ClientType::Sender).await {
                    self.notify_sender_status(UserStatus::Connected).await?;
                }
            },
            ClientType::Sender => {
                info!("A sender at {} has connected", addr)
            },
            _ => {}
        }

        if matched_type == ClientType::Sender {
            let _ = self.notify_sender_status(UserStatus::Connected).await;
        }

        Ok(())
    }

    async fn handle_publish_meta(&mut self, client: &Client, meta: &Meta) -> Result<(), HandlerError> {
        Server::check_type(client, ClientType::Sender)?;

        let modified = std::time::UNIX_EPOCH.elapsed().unwrap().as_millis() as i64;

        let meta = Meta {
            path: meta.path.clone(),
            hash: meta.hash,
            size: meta.size,
            file_modified: meta.file_modified,
            meta_modified: modified,
            chunks: meta.chunks.clone(),
        };

        self.index.add_for_meta(&meta);
        self.index.save(&self.data_dir, &self.bincode_config).await.unwrap();

        info!("Metadata published for file {}", meta.path);

        match self.get_by_type(&ClientType::Receiver).await {
            Ok(receiver) => {
                receiver.outgoing.send(BinaryMessage::new(MessagePayload::NotifyUpdated(NotifyUpdatedData { last_modified: modified } ))).await.unwrap();
            },
            _ => {}
        }

        Ok(())
    }

    async fn handle_remove_meta(&mut self, client: &Client, data: &RemoveMetaData) -> Result<(), HandlerError> {
        Server::check_type(client, ClientType::Sender)?;

        self.index.remove(&data.path);
        self.index.save(&self.data_dir, &self.bincode_config).await.map_err(|_| HandlerError::ServerHandlerError)?;

        info!("Metadata removed for file {}", data.path);

        Ok(())
    }

    async fn handle_request_meta(&mut self, client: &Client, data: &RequestMetaData) -> Result<(), HandlerError> {
        Server::identified(client)?;

        let entries = self.index.get_modified_after(data.after);

        let meta: Vec<Meta> = entries.iter().map(|e| e.meta.clone()).collect();
        let message = BinaryMessage::new(MessagePayload::TransmitMeta(TransmitMetaData { meta }));

        client.outgoing.send(message).await.map_err(|_| HandlerError::IoError)?;

        Ok(())
    }

    async fn handle_request_chunk(&mut self, client: &Client, data: &RequestChunkData) -> Result<(), HandlerError> {
        Server::check_type(client, ClientType::Receiver)?;

        let receiver = self.get_by_type(&ClientType::Sender).await?;

        receiver.outgoing
            .send(BinaryMessage::new(MessagePayload::RequestChunk(data.clone())))
            .await.map_err(|_| HandlerError::IoError)?;

        Ok(())
    }

    async fn handle_transmit_chunk(&mut self, client: &Client, data: &TransmitChunkData) -> Result<(), HandlerError> {
        Server::check_type(client, ClientType::Sender)?;

        let receiver = match self.get_by_type(&ClientType::Receiver).await {
            Ok(receiver) => receiver,
            Err(_) => {
                return client.outgoing.send(BinaryMessage::new(MessagePayload::StopChunk)).await.map_err(|_| HandlerError::IoError);
            },
        };

        receiver.outgoing
            .send(BinaryMessage::new(MessagePayload::TransmitChunk(data.clone())))
            .await.map_err(|_| HandlerError::IoError)?;

        Ok(())
    }

    async fn handle_stop_chunk(&mut self, client: &Client) -> Result<(), HandlerError> {
        Server::check_type(client, ClientType::Receiver)?;

        let receiver = self.get_by_type(&ClientType::Sender).await?;
        receiver.outgoing
            .send(BinaryMessage::new(MessagePayload::StopChunk))
            .await.map_err(|_| HandlerError::IoError)?;

        Ok(())
    }

    async fn get_by_type(&mut self, client_type: &ClientType) -> Result<tokio::sync::MutexGuard<'_, Client>, HandlerError> {
        for (_, c) in &self.clients {
            let current: tokio::sync::MutexGuard<'_, Client> = c.lock().await;
            if &current.client_type == client_type {
                return Ok(current)
            }
        }

        Err(HandlerError::ClientNotFound)
    }

    async fn exists_by_type(&self, client_type: &ClientType) -> bool {
        for (_, c) in &self.clients {
            let current: tokio::sync::MutexGuard<'_, Client> = c.lock().await;
            if &current.client_type == client_type {
                return true
            }
        }

        false
    }

    fn check_type(client: &Client, t: ClientType) -> Result<(), HandlerError> {
        if client.client_type == t {
            Ok(())
        } else {
            Err(HandlerError::Unsupported)
        }
    }

    fn identified(client: &Client) -> Result<(), HandlerError> {
        if client.client_type != ClientType::Unidentified {
            Ok(())
        } else {
            Err(HandlerError::Unsupported)
        }
    }

    fn unidentified(client: &Client) -> Result<(), HandlerError> {
        if client.client_type == ClientType::Unidentified {
            Ok(())
        } else {
            Err(HandlerError::Unsupported)
        }
    }

}