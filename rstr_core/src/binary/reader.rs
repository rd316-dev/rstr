use std::{string::FromUtf8Error};

use tokio::io::{AsyncReadExt};

use crate::binary::serialization::{BinaryRead, DeserializationError};

pub struct BinaryReader<'a, T: AsyncReadExt> where T: Unpin {
    internal: &'a mut T,
    pos: usize
}

impl <'a, T: AsyncReadExt> BinaryReader<'a, T> where T: Unpin {
    pub fn new(internal: &'a mut T) -> Self {
        BinaryReader { internal: internal, pos: 0 }
    }

    pub fn seek(&mut self, length: usize) {
        return self.pos += length;
    }

    pub async fn read(&mut self, count: usize) -> Vec<u8> {
        let mut buffer = vec![0; count];
        self.internal.read_exact(&mut buffer).await.unwrap();

        self.pos += count as usize;

        return buffer;
    }

    pub async fn read_le_u32(&mut self) -> u32 {
        let data = self.read(4).await;

        let value = 
            ((data[0] as u32) << 24) |
            ((data[1] as u32) << 16) |
            ((data[2] as u32) <<  8) |
            ((data[3] as u32));

        return value;
    }

    pub async fn read_le_i32(&mut self) -> i32 {
        self.read_le_u32().await as i32
    }

    pub async fn read_le_u64(&mut self) -> u64 {
        let data = self.read(8).await;

        let value = 
            ((data[0]     as u64) << 56) |
            ((data[1] as u64) << 48) |
            ((data[2] as u64) << 40) |
            ((data[3] as u64) << 32) |
            ((data[4] as u64) << 24) |
            ((data[5] as u64) << 16) |
            ((data[6] as u64) <<  8) |
            ((data[7] as u64));

        return value;
    }

    pub async fn read_le_i64(&mut self) -> i64 {
        self.read_le_u64().await as i64
    }

    pub async fn read_sized_str(&mut self) -> Result<String, FromUtf8Error> {
        let size = self.read_le_u32().await;
        let data = self.read(size as usize).await;

        return String::from_utf8(data);
    }

    pub async fn read_unsized_str(&mut self, len: u32) -> Result<String, FromUtf8Error> {
        let data = self.read(len as usize).await;

        return String::from_utf8(data);
    }

    pub async fn read_typed_array<I>(&mut self) -> Result<Vec<I>, DeserializationError> where I: BinaryRead<'a, T> {
        let count = self.read_le_u32().await;

        let mut vector: Vec<I> = vec![];

        for _ in 0..count {
            vector.push( I::read(self).await? );
        }

        return Ok(vector);
    }
}