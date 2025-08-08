use std::{collections::HashSet, hash::Hasher, io::SeekFrom, path::PathBuf, sync::Arc, time::{Duration, SystemTime}};

use bincode::config::{Fixint, LittleEndian, NoLimit};
use bytes::Bytes;
use lexical_sort::lexical_cmp;
use rstr_core::{binary::serialization::DeserializationError, message::{BBytes, BinaryMessage, LoginData, LoginType, MessagePayload, RequestChunkData, TransmitChunkData, TransmitMetaData, UserStatus}, meta::{MetaIndex, MetaIndexEntry}};
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

struct RequestingFile {
    remote_path: String,
    total_downloaded: u64,
    progress: f32,
    chunk_index: u32,
    current_offset: u64,
    chunk_hash_writer: Xxh3//CoreWrapper<Sha1Core>
}

#[derive(PartialEq, Eq)]
pub enum ReceiverState {
    Initializing,
    Idle,
    //RequestedMeta,
    ReceivingMeta,
    RequestedChunk
}

#[derive(PartialEq, Eq)]
pub enum SenderState {
    Initializing,
    Idle,
    //SendingMeta,
    //SendingChunk,
}

pub struct Receiver {
    data_dir: PathBuf,
    index: MetaIndex,
    bincode_config: bincode::config::Configuration<LittleEndian, Fixint, NoLimit>,

    state: ReceiverState,
    internal: mpsc::Sender<EventMessage>,

    //last_updated: i64,

    requesting_chunk: Option<RequestingFile>
}

pub struct Sender {
    data_dir: PathBuf,
    index: MetaIndex,
    bincode_config: bincode::config::Configuration<LittleEndian, Fixint, NoLimit>,

    state: SenderState,
    internal: mpsc::Sender<EventMessage>,

    sender_cancellation_token: Option<CancellationToken>
}

pub trait Client : Sized {
    async fn new(data_dir: &PathBuf, internal: mpsc::Sender<EventMessage>, bincode_config: &bincode::config::Configuration<LittleEndian, Fixint, NoLimit>) -> Result<Self, NewClientError>;
    async fn login(&mut self, username: &str, password: &str) -> Result<(), ClientHandlerError>;
    fn get_event_sender(&self) -> &mpsc::Sender<EventMessage>;
    fn get_bincode_config(&self) -> &bincode::config::Configuration<LittleEndian, Fixint, NoLimit>;

    async fn send_message(&self, payload: MessagePayload) -> Result<(), ClientHandlerError> {
        let message = BinaryMessage::new(payload);

        let encoded = bincode::encode_to_vec(message.clone(), self.get_bincode_config().clone()).unwrap();

        let mut string = "".to_owned();
        for e in encoded {
            string = format!("{} {:x} ", string, e);
        }

        println!("{}", string);

        self.send_event(EventMessage::SendMessage(message)).await
    }

    async fn send_event(&self, message: EventMessage) -> Result<(), ClientHandlerError> {
        self.get_event_sender().send(message)
            .await
            .map_err(|_| ClientHandlerError::IOError)
    }
}

impl Client for Receiver {
    async fn new(data_dir: &PathBuf, internal: mpsc::Sender<EventMessage>, bincode_config: &bincode::config::Configuration<LittleEndian, Fixint, NoLimit>) -> Result<Self, NewClientError> {
        let index = MetaIndex::load(data_dir, bincode_config).await.map_err(|_| NewClientError::MetaReadError)?;

        //let last_updated = index.get_last_updated();
        let receiver = Receiver {
            data_dir: data_dir.to_owned(),
            index: index,
            bincode_config: *bincode_config,

            state: ReceiverState::Initializing,
            internal: internal,

            //last_updated: last_updated,
            requesting_chunk: None
        };

        Ok(receiver)
    }

    async fn login(&mut self, username: &str, password: &str) -> Result<(), ClientHandlerError> {
        self.send_message(MessagePayload::Login(LoginData {
            login_type: LoginType::Receiver, username: username.to_string(), key: password.to_string() 
        })).await
    }

    fn get_event_sender(&self) -> &mpsc::Sender<EventMessage> {
        &self.internal
    }

    fn get_bincode_config(&self) -> &bincode::config::Configuration<LittleEndian, Fixint, NoLimit> {
        &self.bincode_config
    }
}

impl Client for Sender {
    async fn new(data_dir: &PathBuf, internal: mpsc::Sender<EventMessage>, bincode_config: &bincode::config::Configuration<LittleEndian, Fixint, NoLimit>) -> Result<Self, NewClientError> {
        let index = MetaIndex::load(data_dir, bincode_config).await.map_err(|_| NewClientError::MetaReadError)?;

        let sender = Sender {
            data_dir: data_dir.to_owned(),
            index: index,
            bincode_config: *bincode_config,

            state: SenderState::Initializing,
            internal: internal,
            sender_cancellation_token: None,
        };

        Ok(sender)
    }
    async fn login(&mut self, username: &str, password: &str) -> Result<(), ClientHandlerError> {
        self.send_message(MessagePayload::Login(LoginData {
            login_type: LoginType::Sender, username: username.to_string(), key: password.to_string() 
        })).await
    }

    fn get_event_sender(&self) -> &mpsc::Sender<EventMessage> {
        &self.internal
    }

    fn get_bincode_config(&self) -> &bincode::config::Configuration<LittleEndian, Fixint, NoLimit> {
        &self.bincode_config
    }
}

impl Receiver {
    /*pub async fn request_new_meta(&mut self) -> Result<(), ClientHandlerError> {
        self.request_meta(self.index.get_last_updated()).await
    }*/

    /*pub async fn request_meta(&mut self, after: i64) -> Result<(), ClientHandlerError> {
        self.check_state(ReceiverState::Idle).map_err(|_| ClientHandlerError::WrongState)?;

        let data = RequestMetaData {
            after: after
        };

        self.send_message(MessagePayload::RequestMeta(data)).await
    }*/

    pub async fn request_file(&mut self, remote_path: &str, local_path: &PathBuf) -> Result<(), ClientHandlerError> {
        self.check_state(ReceiverState::Idle)?;

        let size = self.index.find_entry(remote_path).ok_or(ClientHandlerError::FileNotFound(remote_path.to_owned()))?.meta.size;
        self.index.change_local(remote_path, local_path).unwrap();
        self.index.save(&self.data_dir, &self.bincode_config).await.unwrap();

        self.request_chunk(remote_path, 0).await?;

        let file_name = remote_path.split("/").last().unwrap();
        let formatted_size = Size::from_bytes(size).format().to_string();

        self.send_event(EventMessage::FileStartedDownloading(remote_path.to_owned(), file_name.to_owned(), formatted_size, size)).await?;

        Ok(())
    }

    pub async fn resume_file(&mut self, remote_path: &str) -> Result<(), ClientHandlerError> {
        self.check_state(ReceiverState::Idle)?;

        let entry = self.index.find_entry(remote_path).ok_or(ClientHandlerError::FileNotFound(remote_path.to_owned()))?;
        let size = entry.meta.size;

        let first_not_downloaded = Receiver::find_first_undownloaded(&entry).ok_or(ClientHandlerError::FileIsDownloaded)?;

        self.request_chunk(remote_path, first_not_downloaded).await?;

        let file_name = remote_path.split("/").last().unwrap();
        let formatted_size = Size::from_bytes(size).format().to_string();

        self.send_event(EventMessage::FileStartedDownloading(remote_path.to_owned(), file_name.to_owned(), formatted_size, size)).await?;

        Ok(())
    }

    async fn request_chunk(&mut self, remote_path: &str, chunk_index: u32) -> Result<(), ClientHandlerError> {
        let entry = self.index.find_entry(remote_path).ok_or(ClientHandlerError::FileNotFound(remote_path.to_owned()))?;
        let chunk = entry.meta.chunks.get(chunk_index as usize).ok_or(ClientHandlerError::ChunkNotFound)?;

        self.send_message(MessagePayload::RequestChunk(RequestChunkData { 
            path: remote_path.to_owned(), index: chunk_index, hash: chunk.hash 
        })).await?;

        let total_downloaded = Receiver::calculate_downloaded(entry);

        self.requesting_chunk = Some(RequestingFile {
            remote_path: remote_path.to_owned(), 
            total_downloaded: total_downloaded,
            progress: 0.0f32,
            chunk_index: chunk_index, 
            current_offset: chunk.offset, 
            chunk_hash_writer: Xxh3::new()
        });

        self.state = ReceiverState::RequestedChunk;

        Ok(())
    }

    pub async fn stop_transmition(&mut self) -> Result<(), ClientHandlerError> {
        self.check_state(ReceiverState::RequestedChunk).map_err(|_| ClientHandlerError::WrongState)?;
        self.send_message(MessagePayload::StopChunk).await?;
        self.state = ReceiverState::Idle;

        let (dirs, files) = self.convert_index();
        self.send_event(EventMessage::UpdateReceiverFiles(dirs, files)).await?;

        Ok(())
    }

    pub async fn process_message(&mut self, message: &BinaryMessage) -> Result<(), ClientHandlerError> {
        match message.payload() {
            /*MessagePayload::NotifyUpdated(data) => {
                self.check_states(&[ReceiverState::Idle, ReceiverState::Initializing])?;
                self.process_notify_updated(&data).await?;
            },*/
            MessagePayload::TransmitMeta(data) => {
                self.check_state(ReceiverState::Idle)?;
                self.process_transmit_meta(&data).await?;
            },
            MessagePayload::TransmitChunk(data) => {
                self.check_state(ReceiverState::RequestedChunk)?;
                self.process_transmit_chunk(&data).await?;
            },
            MessagePayload::LoginSuccess => {
                self.check_state(ReceiverState::Initializing)?;
                self.state = ReceiverState::Idle;

                let (dirs, files) = self.convert_index();

                self.send_event(EventMessage::LoggedInAsReceiver(dirs, files)).await?;
            },
            MessagePayload::NotifySenderStatus(status) => {
                match status {
                    UserStatus::Connected => self.send_event(EventMessage::SenderConnected).await,
                    UserStatus::Disconnected => self.send_event(EventMessage::SenderDisconnected).await,
                }?
            }
            _ => { return Err(ClientHandlerError::NotSupported); },
        }

        Ok(())
    }

    /*async fn process_notify_updated(&mut self, data: &NotifyUpdatedData) -> Result<(), ClientHandlerError> {
        if data.last_modified > self.last_updated {
            self.request_meta(self.last_updated).await?;
        }

        Ok(())
    }*/

    async fn process_transmit_meta(&mut self, data: &TransmitMetaData) -> Result<(), ClientHandlerError> {
        self.state = ReceiverState::ReceivingMeta;

        for m in &data.meta {
            self.index.add_for_meta(m);
        }

        self.index.save(&self.data_dir, &self.bincode_config).await.map_err(|_| ClientHandlerError::IOError)?;

        let (dirs, files) = self.convert_index();
        self.send_event(EventMessage::UpdateReceiverFiles(dirs, files)).await?;

        self.state = ReceiverState::Idle;

        Ok(())
    }

    async fn process_transmit_chunk(&mut self, data: &TransmitChunkData) -> Result<(), ClientHandlerError> {
        match self.check_state(ReceiverState::RequestedChunk) {
            Err(_) => return Ok(()),
            _ => ()
        }

        let data = &data.data; // uhhh
        let index = &mut self.index;

        let requesting_chunk = self.requesting_chunk.as_mut().ok_or(ClientHandlerError::FileNotRequested)?;

        let remote_path = requesting_chunk.remote_path.clone();
        let entry = index.find_entry(&remote_path).unwrap();
        let meta = &entry.meta;

        let mut chunk_hash: Xxh3 = requesting_chunk.chunk_hash_writer.clone();
        let offset = requesting_chunk.current_offset;

        let total_size = meta.size;
        let file_modified = meta.file_modified;
        let local_path = entry.local_path.clone();

        let buffer: &[u8] = &data;

        let hashing_task = async move {
            chunk_hash.write(buffer);

            chunk_hash
        };

        let writing_task = async move {
            let mut file = OpenOptions::new()
                .create(true)
                .write(true)
                .open(local_path)
                .await.map_err(|_| ClientHandlerError::IOError)?;

            file.set_len(total_size).await.map_err(|_| ClientHandlerError::IOError)?;
            let mut file_writer = BufWriter::new(&mut file);
            let mut written = 0;

            file_writer.seek(SeekFrom::Start(offset)).await.map_err(|_| ClientHandlerError::IOError).map_err(|_| ClientHandlerError::IOError)?;
            let length = buffer.len();

            file_writer.write_all(buffer).await.map_err(|_| ClientHandlerError::IOError)?;

            written += length;

            file_writer.flush().await.map_err(|_| ClientHandlerError::IOError)?;
            let std_file = file.into_std().await;

            let system_time = SystemTime::UNIX_EPOCH + Duration::from_millis(file_modified as u64);
            
            std_file.set_modified(system_time).map_err(|_| ClientHandlerError::IOError)?;
            std_file.sync_all().map_err(|_| ClientHandlerError::IOError)?;

            Ok::<usize, ClientHandlerError>(written)
        };

        let (chunk_hash, writing_result) = tokio::join!(hashing_task, writing_task);

        let written = writing_result?;

        let chunk_index = requesting_chunk.chunk_index;
        let chunk = meta.chunks.get(chunk_index as usize).unwrap();

        requesting_chunk.current_offset += written as u64;
        requesting_chunk.total_downloaded += written as u64;
        requesting_chunk.chunk_hash_writer = chunk_hash;

        let last_progress = requesting_chunk.progress;
        let new_progress = (requesting_chunk.total_downloaded as f64 / total_size as f64) as f32;

        if requesting_chunk.current_offset < chunk.offset + chunk.size as u64 {
            if new_progress - last_progress > 0.01f32 {
                requesting_chunk.progress = new_progress;
                self.send_event(EventMessage::FileDownloadProgress(new_progress)).await?;
            }

            return Ok(());
        }

        let result_hash: u64 = requesting_chunk.chunk_hash_writer.clone().digest();

        if chunk.hash != result_hash {
            return Err(ClientHandlerError::InvalidHash(chunk.hash, result_hash))
        }

        self.index.mark_received(&remote_path, chunk_index).unwrap();
        self.index.save(&self.data_dir, &self.bincode_config).await.map_err(|_| ClientHandlerError::IOError)?;

        self.requesting_chunk = None;

        let entry = self.index.find_entry(&remote_path).unwrap().clone();

        match Receiver::find_first_undownloaded(&entry) {
            Some(first_undownloaded) => {
                self.request_chunk(&remote_path, first_undownloaded).await?;
            },
            None => {
                // downloading process is finished
                self.state = ReceiverState::Idle;
                self.send_event(EventMessage::FileFinishedDownloading(remote_path)).await?;

                let (dirs, files) = self.convert_index();
                self.send_event(EventMessage::UpdateReceiverFiles(dirs, files)).await?;
            },
        };

        Ok(())
    }

    fn convert_index(&self) -> (Vec<DirectoryData>, Vec<ReceiverFileData>) {
        let mut dirs: HashSet<String> = HashSet::new();
        let mut files: Vec<ReceiverFileData> = vec![];

        dirs.insert("".to_owned());

        self.index.iter().for_each(|e| {
            let formatted_size = Size::from_bytes(e.meta.size).format().to_string();

            let remote_path = e.meta.path.to_owned();
            let mut tokens: Vec<&str> = remote_path.split("/").collect();

            let filename = tokens.last().unwrap().to_owned();
            let directory_path: String;

            if tokens.len() <= 1 {
                directory_path = "".to_owned();
            } else {
                tokens.truncate(tokens.len() - 1);
                directory_path = tokens.join("/");
            }

            let chunks_total = e.meta.chunks.len();
            let chunks_left = chunks_total - e.received_chunks.len();

            let status = if chunks_left == chunks_total {
                ReceiverFileStatusData::NotDownloaded
            } else if chunks_left > 0 {
                ReceiverFileStatusData::Downloading
            } else {
                ReceiverFileStatusData::Downloaded
            };

            let file = ReceiverFileData {
                name: filename.to_owned(),
                dir: directory_path.clone(),
                status: status,
                remote_path: remote_path,
                local_path: e.local_path.to_str().unwrap().to_owned(),
                formatted_size: formatted_size,
                downloaded_progress: 0.0f32,
            };

            dirs.insert(directory_path);
            files.push(file);
        });
        
        let mut dirs: Vec<DirectoryData> = dirs.iter().map(|d| DirectoryData{ display_name: ("/".to_owned() + &d).into(), actual_name: d.into() }).collect();
        dirs.sort_by(|d1, d2| lexical_cmp(d1.actual_name.as_str(), d2.actual_name.as_str()));

        println!("Dirs:");
        dirs.iter().for_each( |d| println!("{} | {}", d.display_name, d.actual_name));

        (dirs, files)
    }

    fn calculate_downloaded(entry: &MetaIndexEntry) -> u64 {
        let downloaded_chunks = &entry.received_chunks;

        let mut downloaded_size: u64 = 0;

        downloaded_chunks.iter().for_each(|c| {
            downloaded_size += entry.meta.chunks[*c as usize].size as u64;
        });

        downloaded_size
    }

    fn find_first_undownloaded(entry: &MetaIndexEntry) -> Option<u32> {
        let chunk_count = entry.meta.chunks.len();
        let downloaded_chunks = &entry.received_chunks;

        for i in 0..chunk_count as u32 {
            if !downloaded_chunks.contains(&i) {
                return Some(i);
            }
        }

        return None
    }

    fn check_state(&self, state: ReceiverState) -> Result<(), ClientHandlerError> {
        if self.state == state {
            Ok(())
        } else {
            Err(ClientHandlerError::WrongState)
        }
    }

    fn check_states(&self, states: &[ReceiverState]) -> Result<(), ClientHandlerError> {
        for s in states {
            if self.state == *s {
                return Ok(())
            }
        }

        Err(ClientHandlerError::WrongState)
    }
}

impl Sender {

    pub async fn create_meta(&mut self, local_path: &PathBuf, remote_path: &str) {
        let local_path = local_path.clone();
        let remote_path = Sender::normalize_path(remote_path);

        let event_tx = self.internal.clone();

        tokio::spawn(async move {
            let progress_mut = Arc::new(Mutex::new(0.0f32));

            let meta_local_path = local_path.clone();
            let meta_remote_path = remote_path.clone();

            let progress_clone = progress_mut.clone();
            let meta_creating_task = async move {
                MetaIndex::create_for_file(meta_local_path, meta_remote_path, progress_clone).await
            };

            let progress_event_tx = event_tx.clone();
            let progress_tracking_task = async move {
                let mut last_progress = 0.0f32;
                loop {
                    let progress = *progress_mut.lock().await;

                    if progress - last_progress >= 0.01 {
                        progress_event_tx.send(EventMessage::MetaProgressReport(progress)).await.unwrap();
                        last_progress = progress;
                    }
                }
            };

            tokio::select! {
                _ = progress_tracking_task => {},
                meta = meta_creating_task => {
                    match meta {
                        Ok(meta) => {
                            let entry = MetaIndexEntry {
                                local_path: local_path.to_owned(), 
                                received_chunks: HashSet::new(),
                                meta: meta
                            };
                            
                            event_tx.send(EventMessage::MetaCreated(entry)).await.unwrap();
                        }
                        Err(_) => {
                            event_tx.send(EventMessage::MetaCreationError).await.unwrap();
                        }
                    }
                }
            }
        });
    }

    pub async fn create_multiple_meta(&mut self, local_paths: &[PathBuf], remote_dir: &str) {
        let local_paths = local_paths.to_owned();
        let remote_dir = Sender::normalize_path(remote_dir) + "/";

        let event_tx = self.internal.clone();

        tokio::spawn(async move {
            let progress_mutex = Arc::new(Mutex::new(0.0f32));

            let task_progress_mutex = progress_mutex.clone();
            let task_event_tx = event_tx.clone();
            let meta_creating_task = async move {
                for local_path in local_paths {
                    let finish_progress_mutex = task_progress_mutex.clone();
                    let task_progress_mutex = task_progress_mutex.clone();

                    let file_name = local_path.file_name().unwrap().to_str().unwrap();
                    let remote_path = remote_dir.clone() + file_name;

                    let result = MetaIndex::create_for_file(
                        local_path.to_owned(), remote_path, task_progress_mutex
                    ).await;

                    match result {
                        Ok(meta) => {
                            let entry = MetaIndexEntry {
                                local_path: local_path.to_owned(), 
                                received_chunks: HashSet::new(),
                                meta: meta
                            };

                            task_event_tx.send(EventMessage::MetaCreated(entry)).await.unwrap();
                            *finish_progress_mutex.lock().await = 0.0f32;
                        }
                        Err(_) => {
                            task_event_tx.send(EventMessage::MetaCreationError).await.unwrap();
                        }
                    };
                }
            };

            let task_event_tx = event_tx.clone();
            let progress_tracking_task = async move {
                let mut last_progress = 0.0f32;
                loop {
                    let progress = *progress_mutex.lock().await;

                    if progress - last_progress >= 0.01 || progress == 0.0f32 {
                        task_event_tx.send(EventMessage::MetaProgressReport(progress)).await.unwrap();
                        last_progress = progress;
                    }
                }
            };

            tokio::select! {
                _ = progress_tracking_task => {},
                _ = meta_creating_task => {}
            }
        });
    }

    pub async fn on_meta_created(&mut self, entry: MetaIndexEntry) -> Result<(), ClientHandlerError> {
        self.index.add_entry(entry);
        self.index.save(&self.data_dir, &self.bincode_config).await.unwrap();

        let (dirs, files) = self.convert_index();

        self.send_event(EventMessage::UpdateSenderFiles(dirs, files)).await.unwrap();

        Ok(())
    }

    pub async fn publish_meta(&mut self, remote_path: &str) -> Result<(), ClientHandlerError> {
        let entry = self.index.find_entry(remote_path).ok_or(ClientHandlerError::FileNotFound(remote_path.to_owned()))?;
        let meta = &entry.meta;

        self.send_message(MessagePayload::PublishMeta(meta.clone())).await
    }

    pub async fn process_message(&mut self, message: &BinaryMessage) -> Result<(), ClientHandlerError> {
        match message.payload() {
            MessagePayload::RequestChunk(data) => {
                self.process_request_chunk(&data)?;
            },
            MessagePayload::StopChunk => {
                let token = self.sender_cancellation_token.clone().ok_or(ClientHandlerError::WrongState)?;
                token.cancel();
            },
            MessagePayload::LoginSuccess => {
                self.state = SenderState::Idle;

                let (dirs, files) = self.convert_index();
                self.send_event(EventMessage::LoggedInAsSender(dirs, files)).await.unwrap();
            },
            _ => {
                return Err(ClientHandlerError::NotSupported)
            }
        };

        Ok(())
    }

    fn convert_index(&self) -> (Vec<DirectoryData>, Vec<SenderFileData>) {
        let mut dirs: HashSet<String> = HashSet::new();
        let mut files: Vec<SenderFileData> = vec![];

        dirs.insert("".to_owned());

        self.index.iter().for_each(|e| {
            let formatted_size = Size::from_bytes(e.meta.size).format().to_string();

            let remote_path = e.meta.path.to_owned();
            let mut tokens: Vec<&str> = remote_path.split("/").collect();

            let filename = tokens.last().cloned().unwrap();
            let directory_path: String;

            if tokens.len() <= 1 {
                directory_path = "".to_owned();
            } else {
                tokens.truncate(tokens.len() - 1);
                directory_path = tokens.join("/");
            }

            let file = SenderFileData {
                name: filename.to_owned(),
                dir: directory_path.clone(),
                remote_path: remote_path,
                local_path: e.local_path.to_str().unwrap().to_owned(),
                formatted_size: formatted_size,
                meta_creation_progress: 0.0f32,
            };

            dirs.insert(directory_path);
            files.push(file);
        });
        
        let mut dirs: Vec<DirectoryData> = dirs.iter().map(|d| DirectoryData{ display_name: ("/".to_owned() + &d), actual_name: d.to_string() }).collect();
        dirs.sort_by(|d1, d2| lexical_cmp(d1.actual_name.as_str(), d2.actual_name.as_str()));

        (dirs, files)
    }

    fn process_request_chunk(&mut self, data: &RequestChunkData) -> Result<(), ClientHandlerError> {
        let remote_path = &data.path;
        let file_name = remote_path.split("/").last().unwrap().to_owned();
        let entry = self.index.find_entry(remote_path).ok_or(ClientHandlerError::FileNotFound(remote_path.to_owned()))?;
        let meta = &entry.meta;

        let local_filepath = entry.local_path.clone();
        let chunk_index = data.index;
        let total_chunks = meta.chunks.len().clone();
        let chunk = meta.chunks.get(chunk_index.clone() as usize).ok_or(ClientHandlerError::ChunkNotFound)?;

        if chunk.hash != data.hash {
            return Err(ClientHandlerError::InvalidHash(chunk.hash, data.hash));
        }

        let cancellation_token = CancellationToken::new();
        self.sender_cancellation_token = Some(cancellation_token.clone());

        let chunk_offset = chunk.offset.clone();
        let chunk_size = chunk.size.clone();
        let chunk_hash = chunk.hash;

        let sender = self.internal.clone();
        tokio::spawn(async move {
            let cancel_sender = sender.clone();
            let cancel_task = async move {
                cancel_sender.send(EventMessage::TransferStopped).await.unwrap();
                cancellation_token.cancelled().await
            };

            let transmit_task = async move {
                sender.send(EventMessage::TransferStarted(TransferingFileData {
                    file_name: file_name,
                    chunk_index: chunk_index as i32,
                    total_chunks: total_chunks as i32,
                })).await.unwrap();

                let file = File::open(&local_filepath).await.unwrap();
                let mut buf_reader = BufReader::with_capacity(65536, file);
                buf_reader.seek(SeekFrom::Start(chunk_offset)).await.unwrap();

                let mut count = 0;

                while count < chunk_size {
                    let read_buf = buf_reader.fill_buf().await.unwrap().to_owned();

                    let size = read_buf.len();
                    let boundary = std::cmp::min(chunk_size - count, size as u32);
                    let outcoming_buf = read_buf[0..boundary as usize].to_vec();

                    let payload = MessagePayload::TransmitChunk(TransmitChunkData{
                        hash: chunk_hash, offset: chunk_offset + count as u64, data: outcoming_buf
                    });
                    
                    sender.send(EventMessage::SendMessage(BinaryMessage::new(payload))).await.unwrap();

                    buf_reader.consume(size);

                    count += size as u32;

                    if size <= 0 {
                        break;
                    }
                };

                sender.send(EventMessage::TransferFinished).await.unwrap();
            };

            tokio::select! {
                _ = cancel_task => {},
                _ = transmit_task => {}
            };
        });

        Ok(())
    }

    fn normalize_path(path: &str) -> String {
        let iter = path.split("/").into_iter();
        let filtered: Vec<&str> = iter.filter(|p| !p.trim().is_empty()).collect();

        filtered.join("/")
    }

    /*fn check_state(&self, state: SenderState) -> Result<(), ClientHandlerError> {
        if self.state == state {
            Ok(())
        } else {
            Err(ClientHandlerError::WrongState)
        }
    }*/

}