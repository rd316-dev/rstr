use std::{collections::HashSet, hash::Hasher, io::SeekFrom, path::PathBuf, time::{Duration, SystemTime}};

use bincode::config::{Fixint, LittleEndian, NoLimit};
use lexical_sort::lexical_cmp;
use rstr_core::{message::{BinaryMessage, LoginData, LoginType, MessagePayload, RequestChunkData, TransmitChunkData, TransmitMetaData, UserStatus}, meta::{MetaIndex, MetaIndexEntry}};
use rstr_ui::{event_message::EventMessage, model::{DirectoryData, ReceiverFileData, ReceiverFileStatusData}};
use size::Size;
use tokio::{fs::OpenOptions, io::{AsyncSeekExt, AsyncWriteExt, BufWriter}, sync::mpsc};
use xxhash_rust::xxh3::Xxh3;

use crate::client::{Client, ClientHandlerError, NewClientError};

pub struct Receiver {
    data_dir: PathBuf,
    index: MetaIndex,
    bincode_config: bincode::config::Configuration<LittleEndian, Fixint, NoLimit>,

    authorized: bool,
    internal: mpsc::Sender<EventMessage>,
    transmission_tx: mpsc::Sender<BinaryMessage>,

    requesting_chunk: Option<RequestingFile>
}

struct RequestingFile {
    remote_path: String,
    total_downloaded: u64,
    progress: f32,
    chunk_index: u32,
    current_offset: u64,
    chunk_hash_writer: Xxh3
}

impl Client for Receiver {
    async fn new(
        data_dir: &PathBuf, 
        internal: mpsc::Sender<EventMessage>, 
        transmission_tx: mpsc::Sender<BinaryMessage>,
        bincode_config: &bincode::config::Configuration<LittleEndian, Fixint, NoLimit>
    ) -> Result<Self, NewClientError> {
        let index = MetaIndex::load(data_dir, bincode_config).await.map_err(|_| NewClientError::MetaReadError)?;

        let receiver = Receiver {
            data_dir: data_dir.to_owned(),
            index: index,
            bincode_config: *bincode_config,

            authorized: false,
            internal: internal,
            transmission_tx: transmission_tx,

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

impl Receiver {

    pub async fn request_file(&mut self, remote_path: &str, local_path: &PathBuf) -> Result<(), ClientHandlerError> {
        self.check_authorized()?;

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
        self.check_authorized()?;

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
        self.check_authorized()?;

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

        Ok(())
    }

    pub async fn stop_transmition(&mut self) -> Result<(), ClientHandlerError> {
        self.check_authorized()?;
        self.send_message(MessagePayload::StopChunk).await?;

        let (dirs, files) = self.convert_index();
        self.send_event(EventMessage::UpdateReceiverFiles(dirs, files)).await?;

        Ok(())
    }

    pub async fn process_message(&mut self, message: &BinaryMessage) -> Result<(), ClientHandlerError> {
        match message.payload() {
            MessagePayload::TransmitMeta(data) => {
                self.process_transmit_meta(&data).await?;
            },
            MessagePayload::TransmitChunk(data) => {
                self.process_transmit_chunk(&data).await?;
            },
            MessagePayload::LoginSuccess => {
                self.authorized = true;
                let (dirs, files) = self.convert_index();

                self.send_event(EventMessage::LoggedInAsReceiver(dirs, files)).await?;
            },
            MessagePayload::NotifySenderStatus(status) => {
                match status {
                    UserStatus::Connected => self.send_event(EventMessage::SenderConnected).await,
                    UserStatus::Disconnected => self.send_event(EventMessage::SenderDisconnected).await,
                }?
            }
            MessagePayload::Ping => {},
            _ => { return Err(ClientHandlerError::NotSupported); },
        }

        Ok(())
    }

    async fn process_transmit_meta(&mut self, data: &TransmitMetaData) -> Result<(), ClientHandlerError> {
        for m in &data.meta {
            self.index.add_for_meta(m);
        }

        self.index.save(&self.data_dir, &self.bincode_config).await.map_err(|_| ClientHandlerError::IOError)?;

        let (dirs, files) = self.convert_index();
        self.send_event(EventMessage::UpdateReceiverFiles(dirs, files)).await?;

        Ok(())
    }

    async fn process_transmit_chunk(&mut self, data: &TransmitChunkData) -> Result<(), ClientHandlerError> {
        let data = &data.data;
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
}