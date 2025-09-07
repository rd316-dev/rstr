use std::{any::Any, os::windows::io::AsHandle, sync::Arc, thread::{self, JoinHandle}};

use bytes::Bytes;
use rstr_core::meta::Meta;

use crate::lru::Lru;

struct LoadingThread {
    index: u32,
    handle: JoinHandle<Bytes>
}

/*impl LoadingThread {
    fn new<F>(index: u32, on_load: F) 
    where F: Fn(u32) -> Bytes {
        let handle = thread::spawn(|| {
            on_load(index)
        });
    }
}*/

struct RemoteChunk {
    offset: u64,
    length: u64,
    position: u64
}

struct SparseRemoteFile<F: Fn(u32) -> Bytes> {
    meta: Meta,
    cache: Lru<u32, RemoteChunk>,
    on_load: F,
    loading_thread: Option<LoadingThread>
}

impl <F: Fn(u32) -> Bytes> SparseRemoteFile<F> {
    fn new(meta: &Meta, on_load: F) -> Self
    where F : Fn(&str, i32) -> Bytes {
        let max_chunks = std::cmp::min(meta.chunks.len(), 10);

        let cache = Lru::new(max_chunks);
        SparseRemoteFile { meta: meta.clone(), cache: cache, on_load: on_load, loading_thread: None }
    }

    fn get_chunk(&mut self, index: u32) -> Option<Bytes> {
        let data = match self.loading_thread.take() {
            Some(thread) if thread.index == index => {
                thread.handle.join().ok()
            },
            Some(thread) => {
                // todo: add thread interrupt
                Some((self.on_load)(index))
            },
            None => {
                Some((self.on_load)(index))
            },
        };

        None
    }

}