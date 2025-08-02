
use std::{path::PathBuf, rc::Rc};

use rfd::FileDialog;
use slint::{ComponentHandle, Model, ModelExt, ModelRc, PlatformError, SharedString, ToSharedString, VecModel};
use tokio::sync::mpsc;

use crate::{event_message::EventMessage, model::{DirectoryData, MetaFileData, ReceiverFileData, SenderFileData}};

//use crate::{client::EventMessage, AppWindow, ConnectionStatus, CreateMetaDialog, CreateMetaMultipleDialog, Directory, DownloadingFile, MetaFile, ReceiverFile, SenderFile};

slint::include_modules!();

const VERSION: &str = env!("CARGO_PKG_VERSION");
pub struct GuiContext {
    pub app_window: AppWindow,
    meta_dialog: CreateMetaDialog,
    meta_multiple_dialog: CreateMetaMultipleDialog
}

pub enum ProcessingEvent {
    ConnectionStatusChanged(ConnectionStatus),
    LoggedInAsReceiver(Vec<DirectoryData>, Vec<ReceiverFileData>),
    LoggedInAsSender(Vec<DirectoryData>, Vec<SenderFileData>),
    MultipleMetadataCreationStarted(Vec<MetaFileData>),
    MetadataCreationStarted(MetaFileData),
    MetadataCreationFailed(String),
    MetadataCreationProgress(f32),
    MetadataCreated,
    UpdateReceiverFiles(Vec<DirectoryData>, Vec<ReceiverFileData>),
    UpdateSenderFiles(Vec<DirectoryData>, Vec<SenderFileData>),
    ReceiverDirectoryChanged(String),
    SenderDirectoryChanged(String),
    SenderConnected,
    SenderDisconnected,
    FilePartIoError,
    FileStartedDownloading(String, String, String, u64),
    FileDownloadProgress(f32),
    FileFinishedDownloading,
}

#[cfg(windows)]
fn open_author() {
    let _ = std::process::Command::new("cmd")
        .args(["/C", "start https://github.com/rd316-dev"])
        .spawn().unwrap();
}

#[cfg(unix)]
fn open_author() {
    let _ = std::process::Command::new("xdg-open")
        .args(["https://github.com/rd316-dev"])
        .spawn().unwrap();
}

impl GuiContext {
    pub fn setup_ui(event_tx: mpsc::Sender<EventMessage>, processing_tx: mpsc::Sender<ProcessingEvent>) -> Result<GuiContext, Box<dyn std::error::Error>> {
        let context = GuiContext { 
            app_window: AppWindow::new()?,
            meta_dialog: CreateMetaDialog::new()?,
            meta_multiple_dialog: CreateMetaMultipleDialog::new()?
        };

        let multiple_meta_paths: Rc<VecModel<SharedString>> = Rc::new(VecModel::default());
        let event_meta_paths = multiple_meta_paths.clone();

        context.app_window.set_version(("v".to_owned() + &VERSION).into());

        context.meta_multiple_dialog.set_local_paths(ModelRc::new(multiple_meta_paths));

        let tx = event_tx.clone();
        context.app_window.on_pressed_receiver(move || {
            let tx = tx.clone();
            slint::spawn_local(async move {
                tx.send(EventMessage::LogInAsReceiver).await.unwrap();
            }).unwrap();
        });

        let tx = event_tx.clone();
        context.app_window.on_pressed_sender(move || {
            let tx = tx.clone();
            slint::spawn_local(async move {
                tx.send(EventMessage::LogInAsSender).await.unwrap();
            }).unwrap();
        });

        context.app_window.on_author_clicked(move || {
            open_author();
        });

        let tx = processing_tx.clone();
        context.app_window.on_sender_dir_changed(move |dir| {
            let tx = tx.clone();
            slint::spawn_local(async move {
                tx.send(ProcessingEvent::SenderDirectoryChanged(dir.to_string())).await.unwrap();
            }).unwrap();
        });

        let tx = processing_tx.clone();
        context.app_window.on_receiver_dir_changed(move |dir| {
            let tx = tx.clone();
            slint::spawn_local(async move {
                tx.send(ProcessingEvent::ReceiverDirectoryChanged(dir.to_string())).await.unwrap();
            }).unwrap();
        });

        let tx = event_tx.clone();
        context.app_window.on_download(move |remote_path, remote_name| {
            let result = FileDialog::new()
                .set_file_name(remote_name)
                .set_directory("/")
                .save_file();

            let local_path = match result {
                None => return,
                Some(f) => f
            };

            let tx = tx.clone();
            slint::spawn_local(async move {
                tx.send(EventMessage::FileRequested(remote_path.to_string(), local_path)).await.unwrap();
            }).unwrap();
        });

        let tx = event_tx.clone();
        context.app_window.on_resume(move |remote_path| {
            let tx = tx.clone();
            slint::spawn_local(async move {
                tx.send(EventMessage::FileResumed(remote_path.to_string())).await.unwrap();
            }).unwrap();
        });

        let tx = event_tx.clone();
        context.app_window.on_stop_downloading(move || {
            let tx = tx.clone();
            slint::spawn_local(async move {
                tx.send(EventMessage::DownloadStopped).await.unwrap();
            }).unwrap();
        });

        let dialog = context.meta_dialog.as_weak();
        context.app_window.on_upload_clicked(move || {
            dialog.unwrap().window().show().unwrap();
        });

        let dialog = context.meta_multiple_dialog.as_weak();
        context.app_window.on_upload_multiple_clicked(move || {
            dialog.unwrap().window().show().unwrap();
        });

        let dialog = context.meta_dialog.as_weak();
        context.meta_dialog.on_cancel_clicked(move || {
            dialog.unwrap().window().hide().unwrap();
        });

        let dialog = context.meta_dialog.as_weak();
        let tx = event_tx.clone();
        context.meta_dialog.on_ok_clicked(move |local_path, remote_path| {
            let tx = tx.clone();

            slint::spawn_local(async move {
                tx.send(EventMessage::UploadMeta(PathBuf::from(local_path.to_string()), remote_path.to_string())).await.unwrap();
            }).unwrap();

            dialog.unwrap().window().hide().unwrap();
        });

        let dialog = context.meta_dialog.as_weak();
        context.meta_dialog.on_on_browse_clicked(move || {
            let result = FileDialog::new()
                .set_directory("/")
                .pick_file();

            let local_path = match result {
                None => return,
                Some(f) => f
            };

            let filename = local_path.file_name().unwrap().to_str().unwrap();

            dialog.unwrap().invoke_set_local_path(
                local_path.to_str().unwrap().to_shared_string(), 
                filename.to_shared_string()
            );
        });

        //let dialog = create_meta_multiple_dialog.as_weak();
        let meta_paths = event_meta_paths.clone();
        context.meta_multiple_dialog.on_add_files_clicked(move || {
            let result = FileDialog::new()
                .set_directory("/")
                .pick_files();

            let local_paths = match result {
                None => return,
                Some(f) => f
            };

            let local_paths: Vec<SharedString> = local_paths.iter().map(|p| p.to_str().unwrap().into()).collect();

            meta_paths.set_vec(local_paths);

            //dialog.unwrap().set_local_paths(ModelRc::from(local_paths.as_slice()));
        });

        let dialog = context.meta_multiple_dialog.as_weak();
        let meta_paths = event_meta_paths.clone();
        context.meta_multiple_dialog.on_cancel_clicked(move || {
            dialog.unwrap().window().hide().unwrap();
            meta_paths.clear();
        });

        let dialog = context.meta_multiple_dialog.as_weak();
        let meta_paths = event_meta_paths.clone();
        let tx = event_tx.clone();
        context.meta_multiple_dialog.on_ok_clicked(move || {
            let dialog = dialog.unwrap();
            let local_paths = meta_paths.iter().map(|f| {
                PathBuf::from(f.to_string())
            }).collect();

            let remote_dir = dialog.get_remote_dir().to_string();

            let tx = tx.clone();
            slint::spawn_local(async move {
                tx.send(EventMessage::UploadMultipleMeta(remote_dir, local_paths)).await.unwrap();
            }).unwrap();

            dialog.hide().unwrap();
            meta_paths.clear();
        });

        return Ok(context);
    }

    pub fn run(&self) -> Result<(), PlatformError> {
        return self.app_window.run()
    }

    pub fn process_gui_events(&self, rx: mpsc::Receiver<ProcessingEvent>) {
        let app_window = self.app_window.clone_strong();
        let _ = slint::spawn_local(async move {
            return GuiContext::process_gui_events_internal(app_window, rx).await;
        });
    }

    async fn process_gui_events_internal(app_window: AppWindow, mut rx: mpsc::Receiver<ProcessingEvent>) {
        let directories: Rc<VecModel<Directory>> = Rc::new(VecModel::default());

        let receiver_files: Rc<VecModel<ReceiverFile>> = Rc::new(VecModel::default());
        let sender_files: Rc<VecModel<SenderFile>> = Rc::new(VecModel::default());

        // by default we don't filter any files
        let filtered_receiver_files = receiver_files.clone().filter(|_| true);
        let filtered_sender_files = sender_files.clone().filter(|_| true);

        app_window.invoke_set_directories(ModelRc::from(directories.clone()));

        app_window.invoke_set_receiver_files(ModelRc::new(filtered_receiver_files));
        app_window.invoke_set_sender_files(ModelRc::new(filtered_sender_files));

        loop {
            let event = match rx.recv().await {
                Some(event) => event,
                None => break,
            };

            match event {
                ProcessingEvent::ConnectionStatusChanged(status) => {
                    app_window.set_connection_status(status);
                },
                ProcessingEvent::LoggedInAsReceiver(dirs, files) => {
                    directories.set_vec(dirs.iter().map(|d| d.into()).collect::<Vec<Directory>>());
                    receiver_files.set_vec(files.iter().map(|f| f.into()).collect::<Vec<ReceiverFile>>());

                    app_window.invoke_open_receiver();
                },
                ProcessingEvent::LoggedInAsSender(dirs, files) => {
                    directories.set_vec(dirs.iter().map(|d| d.into()).collect::<Vec<Directory>>());
                    sender_files.set_vec(files.iter().map(|f| f.into()).collect::<Vec<SenderFile>>());

                    app_window.invoke_open_sender();
                },
                ProcessingEvent::MetadataCreationStarted(meta_file) => {
                    app_window.invoke_metadata_creation_started((&meta_file).into());
                },
                ProcessingEvent::MultipleMetadataCreationStarted(files) => {
                    app_window.invoke_multiple_metadata_creation_started(ModelRc::from(
                        files.iter().map(|f| f.into()).collect::<Vec<MetaFile>>().as_slice()
                    ));
                },
                ProcessingEvent::MetadataCreationFailed(error) => {
                    app_window.invoke_set_metadata_creation_error(error.into());
                },
                ProcessingEvent::MetadataCreationProgress(progress) => {
                    app_window.invoke_sender_set_progress(progress);
                },
                ProcessingEvent::MetadataCreated => {
                    app_window.invoke_metadata_created();
                },
                ProcessingEvent::UpdateReceiverFiles(dirs, files) => {
                    directories.set_vec(dirs.iter().map(|d| d.into()).collect::<Vec<Directory>>());
                    receiver_files.set_vec(files.iter().map(|f| f.into()).collect::<Vec<ReceiverFile>>());
                },
                ProcessingEvent::UpdateSenderFiles(dirs, files) => {
                    directories.set_vec(dirs.iter().map(|d| d.into()).collect::<Vec<Directory>>());
                    sender_files.set_vec(files.iter().map(|f| f.into()).collect::<Vec<SenderFile>>());
                },
                ProcessingEvent::ReceiverDirectoryChanged(dir) => {
                    let filtered_receiver_files = receiver_files.clone().filter(move |f| f.dir == dir);
                    app_window.invoke_set_receiver_files(ModelRc::new(filtered_receiver_files));
                },
                ProcessingEvent::SenderDirectoryChanged(dir) => {
                    let filtered_sender_files = sender_files.clone().filter(move |f| f.dir == dir);
                    app_window.invoke_set_sender_files(ModelRc::new(filtered_sender_files));
                },
                ProcessingEvent::SenderConnected => {
                    app_window.set_sender_connected(true);
                },
                ProcessingEvent::SenderDisconnected => {
                    app_window.set_sender_connected(false);
                },
                ProcessingEvent::FilePartIoError => {

                },
                ProcessingEvent::FileStartedDownloading(remote_path, file_name, formatted_size, _) => {
                    app_window.invoke_set_downloading_file(DownloadingFile { 
                        remote_path: remote_path.into(), 
                        file_name: file_name.into(), 
                        formatted_size: formatted_size.into() 
                    });
                },
                ProcessingEvent::FileDownloadProgress(progress) => {
                    app_window.invoke_set_downloading_progress(progress);
                },
                ProcessingEvent::FileFinishedDownloading => {
                    app_window.invoke_set_downloading_file(DownloadingFile::default());
                }
            }
        }
    }
}