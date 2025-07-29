use tokio::io::AsyncWriteExt;

use crate::binary::serialization::{BinaryWrite, SerializationError};

pub struct BinaryWriter<T: AsyncWriteExt> where T: Unpin {
    internal: T,
    pos: usize,
}

impl <T: AsyncWriteExt> BinaryWriter<T> where T: Unpin {
    pub fn new(internal: T) -> BinaryWriter<T> {
        BinaryWriter { internal: internal, pos: 0 }
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    pub async fn flush(&mut self) {
        self.internal.flush().await.unwrap();
    }
    
    pub async fn write(&mut self, buffer: &[u8]) -> usize {
        self.internal.write_all(buffer).await.unwrap();
        self.pos += buffer.len();

        buffer.len()
    }

    pub async fn write_unsized_string(&mut self, string: &str) {
        self.write(string.as_bytes()).await;
    }

    pub async fn write_le_u32(&mut self, number: u32) {
        let buffer = [
            (number >> 24 & 0xFF) as u8,
            (number >> 16 & 0xFF) as u8,
            (number >> 8  & 0xFF) as u8,
            (number       & 0xFF) as u8,
        ];

        self.write(&buffer).await;
    }

    pub async fn write_le_i32(&mut self, number: i32) {
        self.write_le_u32(number as u32).await;
    }

    pub async fn write_le_u64(&mut self, number: u64) {
        let buffer = [
            (number >> 56 & 0xFF) as u8,
            (number >> 48 & 0xFF) as u8,
            (number >> 40 & 0xFF) as u8,
            (number >> 32 & 0xFF) as u8,
            (number >> 24 & 0xFF) as u8,
            (number >> 16 & 0xFF) as u8,
            (number >> 8  & 0xFF) as u8,
            (number       & 0xFF) as u8,
        ];

        self.write(&buffer).await;
    }

    pub async fn write_le_i64(&mut self, number: i64) {
        self.write_le_u64(number as u64).await;
    }

    pub async fn write_typed_array<I>(&mut self, array: &[I]) -> Result<(), SerializationError>
    where I: BinaryWrite<T> {
        self.write_le_u32(array.len() as u32).await;

        for o in array {
            o.write(self).await?;
        }

        Ok(())
    }

}