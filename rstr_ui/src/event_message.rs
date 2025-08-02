use std::path::PathBuf;

use rstr_core::{message::BinaryMessage, meta::MetaIndexEntry};

use crate::model::{DirectoryData, ReceiverFileData, SenderFileData, TransferingFileData};

pub enum EventMessage {
    ProcessMessage(BinaryMessage),
    SendMessage(BinaryMessage),
    UploadMeta(PathBuf, String),
    UploadMultipleMeta(String, Vec<PathBuf>),
    LogInAsReceiver,
    LogInAsSender,
    LoggedInAsReceiver(Vec<DirectoryData>, Vec<ReceiverFileData>),
    LoggedInAsSender(Vec<DirectoryData>, Vec<SenderFileData>),
    UpdateReceiverFiles(Vec<DirectoryData>, Vec<ReceiverFileData>),
    UpdateSenderFiles(Vec<DirectoryData>, Vec<SenderFileData>),
    MetaCreationError,
    MetaProgressReport(f32),
    MetaCreated(MetaIndexEntry),
    SenderConnected,
    SenderDisconnected,
    FileRequested(String, PathBuf),
    FileResumed(String),
    DownloadStopped,
    FilePartIoError,
    FileStartedDownloading(String, String, String, u64),
    FileDownloadProgress(f32),
    FileFinishedDownloading(String),
    TransferStarted(TransferingFileData),
    TransferFinished,
    TransferStopped,
    Quit
}