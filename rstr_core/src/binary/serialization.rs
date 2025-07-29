use core::{array::TryFromSliceError, str::Utf8Error};
use std::{string::FromUtf8Error};

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::binary::{reader::BinaryReader, writer::BinaryWriter};

#[derive(Debug)]
pub enum DeserializationError {
    InvalidMagic,
    InvalidVersion,
    InvalidMessageType,
    UnknownError,
    StringError,
    LengthError,
    UnknownMessage
}

#[derive(Debug)]
pub enum SerializationError {}

impl From<Utf8Error> for DeserializationError {
    fn from(_: Utf8Error) -> DeserializationError {
        DeserializationError::StringError
    }
}

impl From<FromUtf8Error> for DeserializationError {
    fn from(_: FromUtf8Error) -> DeserializationError {
        DeserializationError::StringError
    }
}

impl From<TryFromSliceError> for DeserializationError {
    fn from(_: TryFromSliceError) -> DeserializationError {
        DeserializationError::UnknownError
    }
}

pub trait BinaryRead<'a, T: AsyncReadExt> where T: Unpin {
    fn read(reader: &mut BinaryReader<'a, T>) -> impl Future<Output = Result<Self, DeserializationError>> where Self: Sized;
}

pub trait BinaryWrite<T: AsyncWriteExt> where T: Unpin {
    fn write(&self, writer: &mut BinaryWriter<T>) -> impl Future<Output = Result<(), SerializationError>>;
}

pub trait BinarySize {
    fn binary_size(&self) -> usize;
}

pub trait StaticBinarySize {
    fn static_size() -> usize;
}


impl <'a, T: AsyncReadExt> BinaryRead<'a, T> for u32 where T: Unpin {
    async fn read(reader: &mut BinaryReader<'a, T>) -> Result<Self, DeserializationError> {
        Ok(reader.read_le_u32().await)
    }
}

impl <'a, T: AsyncReadExt> BinaryRead<'a, T> for u64 where T: Unpin {
    async fn read(reader: &mut BinaryReader<'a, T>) -> Result<Self, DeserializationError> {
        Ok(reader.read_le_u64().await)
    }
}

impl <'a, T: AsyncReadExt> BinaryRead<'a, T> for i32 where T: Unpin {
    async fn read(reader: &mut BinaryReader<'a, T>) -> Result<Self, DeserializationError> {
        Ok(reader.read_le_i32().await)
    }
}

impl <'a, T: AsyncReadExt> BinaryRead<'a, T> for i64 where T: Unpin {
    async fn read(reader: &mut BinaryReader<'a, T>) -> Result<Self, DeserializationError> {
        Ok(reader.read_le_i64().await)
    }
}


impl <T: AsyncWriteExt> BinaryWrite<T> for &str where T: Unpin {
    async fn write(&self, writer: &mut BinaryWriter<T>) -> Result<(), SerializationError> {
        let buf = self.as_bytes();
        writer.write_le_u32(buf.len() as u32).await;
        writer.write(buf).await;

        Ok(())
    }
}

impl <T: AsyncWriteExt> BinaryWrite<T> for String where T: Unpin {
    async fn write(&self, writer: &mut BinaryWriter<T>) -> Result<(), SerializationError> {
        let buf = self.as_bytes();
        writer.write_le_u32(buf.len() as u32).await;
        writer.write(buf).await;

        Ok(())
    }
}

impl <T: AsyncWriteExt> BinaryWrite<T> for u32 where T: Unpin {
    async fn write(&self, writer: &mut BinaryWriter<T>) -> Result<(), SerializationError> {
        writer.write_le_u32(*self).await;

        Ok(())
    }
}

impl <T: AsyncWriteExt> BinaryWrite<T> for u64 where T: Unpin {
    async fn write(&self, writer: &mut BinaryWriter<T>) -> Result<(), SerializationError> {
        writer.write_le_u64(*self).await;

        Ok(())
    }
}

impl <T: AsyncWriteExt> BinaryWrite<T> for i32 where T: Unpin {
    async fn write(&self, writer: &mut BinaryWriter<T>) -> Result<(), SerializationError> {
        writer.write_le_i32(*self).await;

        Ok(())
    }
}

impl <T: AsyncWriteExt> BinaryWrite<T> for i64 where T: Unpin {
    async fn write(&self, writer: &mut BinaryWriter<T>) -> Result<(), SerializationError> {
        writer.write_le_i64(*self).await;

        Ok(())
    }
}


impl <T: StaticBinarySize> BinarySize for T {
    fn binary_size(&self) -> usize {
        T::static_size()
    }
}

impl BinarySize for &str {
    fn binary_size(&self) -> usize {
        4 + self.len()
    }
}

impl BinarySize for String {
    fn binary_size(&self) -> usize {
        4 + self.len()
    }
}

impl <T> BinarySize for [T] where T: StaticBinarySize {
    fn binary_size(&self) -> usize {
        self.len() * T::static_size()
    }
}

impl <T> BinarySize for &[T] where T: StaticBinarySize {
    fn binary_size(&self) -> usize {
        self.len() * T::static_size()
    }
}


impl StaticBinarySize for bool { fn static_size() -> usize { 1 } }


impl StaticBinarySize for f32 { fn static_size() -> usize { 4 } }

impl StaticBinarySize for f64 { fn static_size() -> usize { 8 } }


impl StaticBinarySize for u8 { fn static_size() -> usize { 1 } }

impl StaticBinarySize for u16 { fn static_size() -> usize { 2 } }

impl StaticBinarySize for u32 { fn static_size() -> usize { 4 } }

impl StaticBinarySize for u64 { fn static_size() -> usize { 8 } }

impl StaticBinarySize for u128 { fn static_size() -> usize { 16 } }


impl StaticBinarySize for i8 { fn static_size() -> usize { 1 } }

impl StaticBinarySize for i16 { fn static_size() -> usize { 2 } }

impl StaticBinarySize for i32 { fn static_size() -> usize { 4 } }

impl StaticBinarySize for i64 { fn static_size() -> usize { 8 } }

impl StaticBinarySize for i128 { fn static_size() -> usize { 16 } }
