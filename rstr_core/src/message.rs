use bincode::{BorrowDecode, Decode, Encode};
use bytes::Bytes;

use crate::{binary::{serialization::BinarySize}, meta::Meta};

pub const MAGIC_HEADER: u32 = 0xDCBA0001; // last 4 hexadecimal digits describe the protocol version

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

#[derive(Encode, Decode, Clone)]
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
#[derive(Encode, Decode, Clone)]
pub enum MessagePayload {
    Unknown                             = 0,
    Error           (String)            = 1,
    Login           (LoginData)         = 2,
    LoginSuccess                        = 3,
    PublishMeta     (Meta)              = 4,
    RemoveMeta      (RemoveMetaData)    = 5,
    /* Deprecated */ _RequestMeta       = 6,
    TransmitMeta    (TransmitMetaData)  = 7,
    /* Deprecated */ _NotifyUpdated     = 8,
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
pub struct TransmitMetaData {
    pub meta: Vec<Meta>
}

#[derive(Encode, Decode, Clone)]
pub struct RequestChunkData {
    pub path: String,
    pub index: u32,
    pub hash: u64//[u8; 20]
}

#[derive(Encode, Decode, Clone)]
pub struct TransmitChunkData {
    pub hash: u64,
    pub offset: u64,
    pub data: Vec<u8>
}

impl BinarySize for RequestChunkData {
    fn binary_size(&self) -> usize {
        self.path.binary_size() + 
        self.hash.binary_size() +
        self.index.binary_size()
    }
}
