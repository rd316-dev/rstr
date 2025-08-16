use std::{collections::HashSet, io::SeekFrom, path::PathBuf, sync::Arc};

use bincode::config::{Fixint, LittleEndian, NoLimit};
use lexical_sort::lexical_cmp;
use rstr_core::{message::{BinaryMessage, LoginData, LoginType, MessagePayload, RequestChunkData, TransmitChunkData}, meta::{MetaIndex, MetaIndexEntry}};
use rstr_ui::{event_message::EventMessage, model::{DirectoryData, SenderFileData, TransferingFileData}};
use size::Size;
use tokio::{fs::File, io::{AsyncBufReadExt, AsyncSeekExt, BufReader}, sync::{mpsc, Mutex}};
use tokio_util::sync::CancellationToken;

use crate::client::{Client, ClientHandlerError, NewClientError};

pub struct Sender {
    data_dir: PathBuf,
    index: MetaIndex,
    bincode_config: bincode::config::Configuration<LittleEndian, Fixint, NoLimit>,

    authorized: bool,
    internal: mpsc::Sender<EventMessage>,
    transmission_tx: mpsc::Sender<BinaryMessage>,

    sender_cancellation_token: Option<CancellationToken>
}

impl Client for Sender {
    async fn new(
        data_dir: &PathBuf, 
        internal: mpsc::Sender<EventMessage>, 
        transmission_tx: mpsc::Sender<BinaryMessage>,
        bincode_config: &bincode::config::Configuration<LittleEndian, Fixint, NoLimit>
    ) -> Result<Self, NewClientError> {
        let index = MetaIndex::load(data_dir, bincode_config).await.map_err(|_| NewClientError::MetaReadError)?;

        let sender = Sender {
            data_dir: data_dir.to_owned(),
            index: index,
            bincode_config: *bincode_config,

            authorized: false,
            internal: internal,
            transmission_tx: transmission_tx,

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

    fn get_transmission_sender(&self) -> &mpsc::Sender<BinaryMessage> {
        &self.transmission_tx
    }

    fn check_authorized(&self) -> Result<(), ClientHandlerError> {
        if self.authorized {
            Ok(())
        } else {
            Err(ClientHandlerError::WrongState)
        }
    }
}

impl Sender {

    pub async fn create_meta(&mut self, local_path: &PathBuf, remote_path: &str) -> Result<(), ClientHandlerError> {
        self.check_authorized()?;

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

        Ok(())
    }

    pub async fn create_multiple_meta(&mut self, local_paths: &[PathBuf], remote_dir: &str) -> Result<(), ClientHandlerError> {
        self.check_authorized()?;

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

        Ok(())
    }

    pub async fn on_meta_created(&mut self, entry: MetaIndexEntry) -> Result<(), ClientHandlerError> {
        self.index.add_entry(entry);
        self.index.save(&self.data_dir, &self.bincode_config).await.unwrap();

        let (dirs, files) = self.convert_index();

        self.send_event(EventMessage::UpdateSenderFiles(dirs, files)).await.unwrap();

        Ok(())
    }

    pub async fn publish_meta(&mut self, remote_path: &str) -> Result<(), ClientHandlerError> {
        self.check_authorized()?;

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
                self.authorized = true;

                let (dirs, files) = self.convert_index();
                self.send_event(EventMessage::LoggedInAsSender(dirs, files)).await.unwrap();
            },
            MessagePayload::Ping => {},
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

        let event_tx = self.internal.clone();
        let sender = self.transmission_tx.clone();
        tokio::spawn(async move {
            let cancel_tx = event_tx.clone();
            let cancel_task = async move {
                cancel_tx.send(EventMessage::TransferStopped).await.unwrap();
                cancellation_token.cancelled().await
            };

            let transmit_task = async move {
                event_tx.send(EventMessage::TransferStarted(TransferingFileData {
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
                    
                    sender.send(BinaryMessage::new(payload)).await.unwrap();

                    buf_reader.consume(size);

                    count += size as u32;

                    if size <= 0 {
                        break;
                    }
                };

                event_tx.send(EventMessage::TransferFinished).await.unwrap();
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

}