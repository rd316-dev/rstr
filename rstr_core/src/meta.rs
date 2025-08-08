use std::{collections::HashSet, hash::Hasher, path::{Path, PathBuf}, sync::Arc, time::UNIX_EPOCH};
use bincode::{config::{Fixint, LittleEndian, NoLimit}, Decode, Encode};
use bytes::Bytes;
//use num_traits::One;
//use sha1::{Digest, Sha1};
use tokio::{fs::File, io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, BufWriter}, sync::{mpsc, Mutex}};
use xxhash_rust::xxh3::Xxh3;

#[derive(Debug)]
pub enum MetaLoadError {
    IOError,
    DeserializationError
}

#[derive(Debug)]
pub enum MetaSaveError {
    IOError,
    FileCreationError,
    SerializationError
}

#[derive(Debug)]
pub enum CreateMetadataError {
    FileNotFound,
    IOError
}

#[derive(Debug)]
pub enum MetaIndexError {
    FileNotFound,
}

#[derive(Debug)]
pub enum MarkReceivedError {
    FileNotFound,
    ChunkNotFound
}

#[derive(Encode, Decode, Clone)]
pub struct Chunk {
    pub hash: u64,
    pub offset: u64,
    pub size: u32
}

#[derive(Encode, Decode, Clone)]
pub struct Meta {
    pub path: String,
    pub hash: u64,
    pub size: u64,
    pub file_modified: i64,
    pub meta_modified: i64,
    pub chunks: Vec<Chunk>
}

#[derive(Encode, Decode, Clone)]
pub struct MetaIndexEntry {
    pub local_path: PathBuf,
    pub received_chunks: HashSet<u32>,
    pub meta: Meta
}

#[derive(Encode, Decode, Clone)]
pub struct MetaIndex {
    entries: Vec<MetaIndexEntry>
}

impl From<std::io::Error> for CreateMetadataError {
    fn from(_: std::io::Error) -> Self {
        CreateMetadataError::IOError
    }
}

impl MetaIndex {
    pub async fn load(path: &PathBuf, bincode_config: &bincode::config::Configuration<LittleEndian, Fixint, NoLimit>) -> Result<Self, MetaLoadError> {
        let file = match File::open(path.join("index")).await {
            Ok(f) => f,
            Err(_) => { 
                return Ok(MetaIndex {entries: vec![]}) 
            }
        };

        let buf_reader = &mut BufReader::with_capacity(8192, file);

        let mut buf = vec![];
        buf_reader.read_to_end(&mut buf).await.map_err(|_| MetaLoadError::IOError)?;

        let (index, _): (MetaIndex, usize) = bincode::decode_from_slice(&buf, *bincode_config).map_err(|_| MetaLoadError::DeserializationError)?;

        Ok(index)
    }

    pub async fn save(&self, path: &PathBuf, bincode_config: &bincode::config::Configuration<LittleEndian, Fixint, NoLimit>) -> Result<(), MetaSaveError> {
        tokio::fs::create_dir_all(path).await.map_err(|_| MetaSaveError::FileCreationError)?;
        let file = File::create(path.join("index")).await.map_err(|_| MetaSaveError::FileCreationError)?;

        let buf_writer = &mut BufWriter::with_capacity(8192, file);
        let buf = bincode::encode_to_vec(&self, *bincode_config).map_err(|_| MetaSaveError::SerializationError)?;

        buf_writer.write_all(&buf).await.map_err(|_| MetaSaveError::IOError)?;
        buf_writer.flush().await.map_err(|_| MetaSaveError::IOError)?;

        Ok(())
    }

    pub fn iter(&self) -> std::slice::Iter<'_, MetaIndexEntry> {
        self.entries.iter()
    }

    pub fn get_last_updated(&self) -> i64 {
        match self.entries.iter().max_by_key(|e| e.meta.meta_modified) {
            Some(s) => s.meta.meta_modified,
            None => i64::MIN,
        }
    }

    pub fn get_modified_after(&self, after: i64) -> Vec<&MetaIndexEntry> {
        self.entries.iter().filter(|e| e.meta.meta_modified > after).collect()
    }

    // prevent invalid invariants by making the mutable version private
    fn find_entry_mut(&mut self, remote_path: &str) -> Option<&mut MetaIndexEntry> {
       self.entries.iter_mut().find(|e| e.meta.path == remote_path)
    }

    pub fn find_entry(&self, remote_path: &str) -> Option<&MetaIndexEntry> {
        self.entries.iter().find(|e| e.meta.path == remote_path)
    }

    pub fn mark_received(&mut self, remote_path: &str, chunk_index: u32) -> Result<(), MarkReceivedError> {
        let entry = self.find_entry_mut(remote_path).ok_or(MarkReceivedError::FileNotFound)?;

        if entry.meta.chunks.len() <= chunk_index as usize {
            return Err(MarkReceivedError::ChunkNotFound)
        }

        entry.received_chunks.insert(chunk_index);

        Ok(())
    }

    pub fn change_local(&mut self, remote_path: &str, local_path: &PathBuf) -> Result<(), MetaIndexError> {
        let entry = self.find_entry_mut(remote_path).ok_or(MetaIndexError::FileNotFound)?;

        entry.local_path = local_path.to_owned();
        entry.received_chunks = HashSet::new();

        Ok(())
    }

    pub fn change_remote(&mut self, old_remote_path: &str, new_remote_path: &str) -> Result<(), MetaIndexError> {
        let entry = self.find_entry_mut(old_remote_path).ok_or(MetaIndexError::FileNotFound)?;

        entry.meta.path = new_remote_path.to_owned();

        Ok(())
    }

    pub fn remove(&mut self, remote_path: &str) {
        for i in 0..self.entries.len() {
            if self.entries[i].meta.path == remote_path {
                self.entries.remove(i);
                break;
            }
        }
    }

    pub fn add_entry(&mut self, entry: MetaIndexEntry) {
        let remote_path = &entry.meta.path;
        let existing_entry: Option<usize> =  {
            let mut existing: Option<usize> = None;
            for i in 0..self.entries.len() {
                if &self.entries[i].meta.path == remote_path {
                    existing = Some(i);
                    break;
                }
            }

            existing
        };

        if existing_entry.is_some() {
            self.entries[existing_entry.unwrap()] = entry;
        } else {
            self.entries.push(entry);
        }
    }

    pub fn add_for_meta(&mut self, meta: &Meta) {
        let entry = MetaIndexEntry {
            local_path: PathBuf::new(),
            received_chunks: HashSet::new(),
            meta: meta.clone()
        };

        self.add_entry(entry);
    }

    pub async fn create_for_file(local_path: PathBuf, remote_path: String, progress_mut: Arc<Mutex<f32>>) -> Result<Meta, CreateMetadataError> {
        let file_path = Path::new(&local_path);
        let file = File::open(file_path).await.map_err(|_| CreateMetadataError::FileNotFound)?;

        let file_modified = file
            .metadata().await.unwrap()
            .modified().unwrap()
            .duration_since(UNIX_EPOCH).unwrap()
            .as_millis() as i64;

        let total_file_size = file.metadata().await.unwrap().len();

        let (tx, mut rx) = mpsc::channel(4 * 1024 * 1024);

        let mut file_hash: Xxh3 = Xxh3::new(); //Sha1::new();
        let mut chunk_hash: Xxh3 = Xxh3::new(); //Sha1::new();

        let mut chunk_offset = 0;

        let mut file_size = 0;
        let mut chunk_size = 0;

        let mut progress: f32;

        let mut chunks: Vec<Chunk> = vec![];
        let meta: Meta;

        let progress_clone = progress_mut.clone();

        let read_thread = tokio::spawn(async move {
            let mut reader = BufReader::with_capacity(64 * 1024, file);

            loop {
                let bytes = Bytes::copy_from_slice(reader.fill_buf().await.unwrap());
                let length = bytes.len();
                reader.consume(length);

                tx.send(bytes).await.unwrap();

                if length <= 0 {
                    return;
                }
            };
        });

        loop {
            let mut progress_guard = progress_clone.lock().await;

            let rx_result = &rx.recv().await;
            let length = match rx_result {
                Some(data) => {
                    let buffer: &[u8] = &data;
                    let length = buffer.len();

                    file_hash.write(buffer);
                    chunk_hash.write(buffer);

                    chunk_size += length;
                    file_size += length;

                    progress = (file_size as f64 / total_file_size as f64) as f32;
                    *progress_guard = progress;

                    if chunk_size < 16 * 1024 * 1024 {
                        continue;
                    }

                    length
                }
                None => { 0 }
            };

            if chunk_size > 0 {
                let hash = chunk_hash.digest();
                chunk_hash.reset();
                chunks.push( Chunk { hash, offset: chunk_offset, size: chunk_size as u32 } );

                chunk_offset = file_size as u64;
                chunk_size = 0;
            }

            if length <= 0 {
                let hash = file_hash.digest();//file_hash.finalize().try_into().unwrap();

                meta = Meta{ 
                    path: remote_path.to_owned(),
                    hash, 
                    size: file_size as u64, 
                    file_modified: file_modified, 
                    meta_modified: std::time::UNIX_EPOCH.elapsed().unwrap().as_millis() as i64,
                    chunks  
                };

                break;
            }
        }

        read_thread.await.unwrap();

        *progress_mut.lock().await = 1.0f32;

        return Ok(meta);
    }
}
