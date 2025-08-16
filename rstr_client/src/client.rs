use std::{collections::HashSet, hash::Hasher, io::SeekFrom, path::PathBuf, sync::Arc, time::{Duration, SystemTime}};

use bincode::config::{Fixint, LittleEndian, NoLimit};
use lexical_sort::lexical_cmp;
use rstr_core::{binary::serialization::DeserializationError, message::{BinaryMessage, LoginData, LoginType, MessagePayload, RequestChunkData, TransmitChunkData, TransmitMetaData, UserStatus}, meta::{MetaIndex, MetaIndexEntry}};
use rstr_ui::{event_message::EventMessage, model::{DirectoryData, ReceiverFileData, ReceiverFileStatusData, SenderFileData, TransferingFileData}};
use size::Size;
use tokio::{fs::{File, OpenOptions}, io::{AsyncBufReadExt, AsyncSeekExt, AsyncWriteExt, BufReader, BufWriter}, sync::{mpsc, Mutex}};
use tokio_util::sync::CancellationToken;
use xxhash_rust::xxh3::Xxh3;

#[derive(Debug)]
pub enum NewClientError {
    MetaReadError
}

#[derive(Debug)]
pub enum ClientHandlerError {
    NotSupported,
    WrongState,
    SerializationError,
    DeserializationError,
    IOError,
    InvalidHash(u64, u64),
    FileNotFound(String),
    ChunkNotFound,
    FileIsDownloaded,
    FileNotRequested
}

impl From<DeserializationError> for ClientHandlerError {
    fn from(_: DeserializationError) -> Self {
        ClientHandlerError::DeserializationError
    }
}

impl From<std::io::Error> for ClientHandlerError {
    fn from(_: std::io::Error) -> Self {
        ClientHandlerError::IOError
    }
}

pub trait Client : Sized {
    async fn new(
        data_dir: &PathBuf, 
        internal: mpsc::Sender<EventMessage>, 
        transmission_tx: mpsc::Sender<BinaryMessage>,
        bincode_config: &bincode::config::Configuration<LittleEndian, Fixint, NoLimit>
    ) -> Result<Self, NewClientError>;

    async fn login(&mut self, username: &str, password: &str) -> Result<(), ClientHandlerError>;
    fn get_event_sender(&self) -> &mpsc::Sender<EventMessage>;
    fn get_transmission_sender(&self) -> &mpsc::Sender<BinaryMessage>;
    fn check_authorized(&self) -> Result<(), ClientHandlerError>;

    async fn send_message(&self, payload: MessagePayload) -> Result<(), ClientHandlerError> {
        let message = BinaryMessage::new(payload);
        self.get_transmission_sender().send(message).await.map_err(|_| ClientHandlerError::IOError)
    }

    async fn send_event(&self, message: EventMessage) -> Result<(), ClientHandlerError> {
        self.get_event_sender().send(message)
            .await
            .map_err(|_| ClientHandlerError::IOError)
    }
}