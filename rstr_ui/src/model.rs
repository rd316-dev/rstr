use crate::gui::{Directory, DownloadingFile, MetaFile, ReceiverFile, ReceiverFileStatus, SenderFile, TransferingFile};

pub struct DirectoryData {
    pub display_name: String,
    pub actual_name: String    
}

impl Into<Directory> for &DirectoryData {
    fn into(self) -> Directory {
        Directory { display_name: self.display_name.clone().into(), actual_name: self.actual_name.clone().into() }
    }
}

pub enum ReceiverFileStatusData {
    NotDownloaded,
    Downloading,
    Downloaded,
}

impl Into<ReceiverFileStatus> for &ReceiverFileStatusData {
    fn into(self) -> ReceiverFileStatus {
        match self {
            ReceiverFileStatusData::NotDownloaded => ReceiverFileStatus::NotDownloaded,
            ReceiverFileStatusData::Downloading => ReceiverFileStatus::Downloading,
            ReceiverFileStatusData::Downloaded => ReceiverFileStatus::Downloaded,
        }
    }
}

pub struct ReceiverFileData {
    pub name: String,
    pub dir: String,
    pub status: ReceiverFileStatusData,
    pub formatted_size: String,
    pub remote_path: String,
    pub local_path: String,
    pub downloaded_progress: f32
}

impl Into<ReceiverFile> for &ReceiverFileData {
    fn into(self) -> ReceiverFile {
        ReceiverFile { 
            name: self.name.clone().into(),
            dir: self.dir.clone().into(),
            status: (&self.status).into(),
            formatted_size: self.formatted_size.clone().into(),
            remote_path: self.remote_path.clone().into(),
            local_path: self.local_path.clone().into(),
            downloaded_progress: self.downloaded_progress
        }
    }
}

pub struct SenderFileData {
    pub name: String,
    pub dir: String,
    pub formatted_size: String,
    pub remote_path: String,
    pub local_path: String,
    pub meta_creation_progress: f32
}

impl Into<SenderFile> for &SenderFileData {
    fn into(self) -> SenderFile {
        SenderFile { 
            name: self.name.clone().into(),
            dir: self.dir.clone().into(),
            formatted_size: self.formatted_size.clone().into(),
            remote_path: self.remote_path.clone().into(),
            local_path: self.local_path.clone().into(),
            meta_creation_progress: self.meta_creation_progress
        }
    }
}

pub struct MetaFileData {
    pub name: String,
    pub formatted_size: String
}

impl Into<MetaFile> for &MetaFileData {
    fn into(self) -> MetaFile {
        MetaFile { 
            name: self.name.clone().into(),
            formatted_size: self.formatted_size.clone().into()
        }
    }
}

pub struct DownloadingFileData {
    pub remote_path: String,
    pub file_name: String,
    pub formatted_size: String
}

impl Into<DownloadingFile> for &DownloadingFileData {
    fn into(self) -> DownloadingFile {
        DownloadingFile { 
            remote_path: self.remote_path.clone().into(),
            file_name: self.file_name.clone().into(),
            formatted_size: self.formatted_size.clone().into(),
        }
    }
}

pub struct TransferingFileData {
    pub file_name: String,
    pub chunk_index: i32,
    pub total_chunks: i32
}

impl Into<TransferingFile> for &TransferingFileData {
    fn into(self) -> TransferingFile {
        TransferingFile {
            file_name: self.file_name.clone().into(),
            chunk_index: self.chunk_index,
            total_chunks: self.total_chunks
        }
    }
}