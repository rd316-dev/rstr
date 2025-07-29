use bincode::{BorrowDecode, Decode, Encode};
use bytes::Bytes;
use num_derive::{FromPrimitive, ToPrimitive};

use crate::{binary::{serialization::{BinarySize, StaticBinarySize}}, meta::Meta};

pub const MAGIC_HEADER: u32 = 0x5353454D;

#[derive(Clone)]
pub struct BBytes(pub Bytes);

impl Encode for BBytes {
    fn encode<E: bincode::enc::Encoder>(&self, encoder: &mut E) -> Result<(), bincode::error::EncodeError> {
        let bytes = &self.0;
        bytes.encode(encoder)
    }
}

impl <Context> Decode<Context> for BBytes { 
    fn decode<D: bincode::de::Decoder<Context = Context>>(
        decoder: &mut D,
    ) -> Result<Self, bincode::error::DecodeError> {
        let bytes = Vec::<u8>::decode(decoder)?;

        Ok(BBytes(Bytes::from(bytes)))
    }
}

impl <'de, Context> BorrowDecode<'de, Context> for BBytes { 
    fn borrow_decode<D: bincode::de::BorrowDecoder<'de, Context = Context>>(
        decoder: &mut D,
    ) -> Result<Self, bincode::error::DecodeError> {
        let bytes = Vec::<u8>::borrow_decode(decoder)?;

        Ok(BBytes(Bytes::from(bytes)))
    }
}

#[repr(u32)]
#[derive(FromPrimitive, ToPrimitive)]
#[derive(Encode, Decode)]
pub enum MessageType {
    Unknown         = 0,
    PublishMeta     = 1,
    RemoveMeta      = 2,
    RequestMeta     = 3,
    TransmitMeta    = 4,
    NotifyUpdated   = 5,
    RequestChunk    = 6,
    TransmitChunk   = 7,
    StopChunk       = 8,
}

#[derive(Encode, Decode)]
pub struct BinaryMessage {
    magic: u32,
    payload: MessagePayload
}

impl BinaryMessage {
    pub fn new(payload: MessagePayload) -> BinaryMessage {
        BinaryMessage { magic: MAGIC_HEADER, payload }
    }

    pub fn payload(&self) -> &MessagePayload {
        &self.payload
    }

}

#[repr(u32)]
#[derive(Encode, Decode)]
pub enum MessagePayload {
    Unknown                             = 0,
    Error           (String)            = 1,
    Login           (LoginData)         = 2,
    LoginSuccess                        = 3,
    PublishMeta     (Meta)              = 4,
    RemoveMeta      (RemoveMetaData)    = 5,
    RequestMeta     (RequestMetaData)   = 6,
    TransmitMeta    (TransmitMetaData)  = 7,
    NotifyUpdated   (NotifyUpdatedData) = 8,
    RequestChunk    (RequestChunkData)  = 9,
    TransmitChunk   (TransmitChunkData) = 10,
    NotifySenderStatus(UserStatus)      = 11,
    StopChunk                           = 12,
}

#[derive(Encode, Decode, Clone)]
pub enum UserStatus {
    Connected,
    Disconnected
}

#[derive(Encode, Decode, Clone)]
pub enum LoginType {
    Receiver,
    Sender
}

#[derive(Encode, Decode, Clone)]
pub struct LoginData {
    pub login_type: LoginType,
    pub username: String,
    pub key: String
}

#[derive(Encode, Decode, Clone)]
pub struct RemoveMetaData {
    pub path: String
}

#[derive(Encode, Decode, Clone)]
pub struct RequestMetaData {
    pub after: i64
}

#[derive(Encode, Decode, Clone)]
pub struct TransmitMetaData {
    pub meta: Vec<Meta>
}

#[derive(Encode, Decode, Clone)]
pub struct NotifyUpdatedData {
    pub last_modified: i64
}

#[derive(Encode, Decode, Clone)]
pub struct RequestChunkData {
    pub path: String,
    pub index: u32,
    pub hash: u64//[u8; 20]
}

#[derive(Encode, Decode, Clone)]
pub struct TransmitChunkData {
    pub data: BBytes
}

#[derive(Encode, Decode, Clone)]
pub struct TransmitChunkHeader {
    pub hash: u64,//[u8; 20],
    pub offset: u64,
    pub size: u32
}

/*
impl <'a, T: AsyncReadExt> BinaryRead<'a, T> for RequestMetaData where T: Unpin {
    async fn read(reader: &mut BinaryReader<'a, T>) -> Result<Self, DeserializationError> where Self: Sized {
        let after = reader.read_le_i64().await;

        Ok(RequestMetaData{ after })
    }
}

impl <'a, T: AsyncReadExt> BinaryRead<'a, T> for RequestChunkData where T: Unpin {
    async fn read(reader: &mut BinaryReader<'a, T>) -> Result<Self, DeserializationError> {
        let path: String        = reader.read_sized_str().await?.to_string();
        let filename: String    = reader.read_sized_str().await?.to_string();
        let index: u32          = reader.read_le_u32().await;
        let hash: [u8; 20]      = reader.read(20).await.try_into().unwrap();

        return Ok( RequestChunkData {path, filename, index, hash} )
    }
}

impl <'a, T: AsyncReadExt> BinaryRead<'a, T> for TransmitChunkHeader where T: Unpin {
    async fn read(reader: &mut BinaryReader<'a, T>) -> Result<Self, DeserializationError> {
        let hash: [u8; 20]  = reader.read(20).await.try_into().unwrap();
        let offset: u64     = reader.read_le_u64().await;
        let size: u32       = reader.read_le_u32().await;

        return Ok( TransmitChunkHeader {hash, offset, size} )
    }
}

impl <'a, T: AsyncReadExt> BinaryRead<'a, T> for MessageHeader where T: Unpin {
    async fn read(reader: &mut BinaryReader<'a, T>) -> Result<Self, DeserializationError> {
        let magic = reader.read_le_u32().await;
        if magic != 0x5353454D { // MESS in ASCII
            return Err(DeserializationError::InvalidMagic);
        }

        let version = reader.read_le_u32().await;
        if version != 1 {
            return Err(DeserializationError::InvalidVersion);
        }

        let message_type: MessageType = match MessageType::from_u32(reader.read_le_u32().await) {
            Some(t) => { t },
            None => { return Err(DeserializationError::InvalidMessageType); }
        };

        let size = reader.read_le_u32().await;

        Ok(MessageHeader { message_type, size })
    }
}


impl <T: AsyncWriteExt> BinaryWrite<T> for MessageHeader where T: Unpin {
    async fn write(&self, writer: &mut BinaryWriter<T>) -> Result<(), SerializationError> {
        writer.write_le_u32(0x5353454D).await;
        writer.write_le_u32(1).await;
        writer.write_le_u32(self.message_type.to_u32().unwrap()).await;
        writer.write_le_u32(self.size).await;

        Ok(())
    }
}

impl <T: AsyncWriteExt> BinaryWrite<T> for RequestMetaData where T: Unpin {
    async fn write(&self, writer: &mut BinaryWriter<T>) -> Result<(), SerializationError> {
        writer.write_le_i64(self.after).await;
        Ok(())
    }
}

impl <T: AsyncWriteExt> BinaryWrite<T> for RequestChunkData where T: Unpin {
    async fn write(&self, writer: &mut BinaryWriter<T>) -> Result<(), SerializationError> {
        self.path.write(writer).await?;
        self.filename.write(writer).await?;
        writer.write_le_u32(self.index).await;
        writer.write(&self.hash).await;

        Ok(())
    }
}

impl <T: AsyncWriteExt> BinaryWrite<T> for TransmitChunkHeader where T: Unpin {
    async fn write(&self, writer: &mut BinaryWriter<T>) -> Result<(), SerializationError> {
        writer.write(&self.hash).await;
        writer.write_le_u64(self.offset).await;
        writer.write_le_u32(self.size).await;

        Ok(())
    }
}*/


impl StaticBinarySize for RequestMetaData {
    fn static_size() -> usize {
        u64::static_size()
    }
}

impl BinarySize for RequestChunkData {
    fn binary_size(&self) -> usize {
        self.path.binary_size() + 
        self.hash.binary_size() +
        self.index.binary_size()
    }
}

impl StaticBinarySize for TransmitChunkHeader {
    fn static_size() -> usize {
        20 + 8 + 4 
    }
}
