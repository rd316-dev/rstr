#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod client;

use std::{env, error::Error, path::PathBuf, time::Duration};

use bytes::{Bytes};
use futures::{SinkExt, StreamExt};
use rstr_core::message::BinaryMessage;
use rstr_ui::{event_message::EventMessage, gui::{ConnectionStatus, GuiContext, ProcessingEvent}, model::MetaFileData};
use size::Size;
use tokio::{fs::File, sync::mpsc};
use tokio_tungstenite::{connect_async, tungstenite::{self, Message}};

use crate::client::{Receiver, Sender};

//use crate::{client::{EventMessage, Receiver, Sender}, gui::{process_gui_event, setup_ui, ProcessingEvent}};

#[derive(Debug)]
enum HandlerError {
    TungsteniteError(tungstenite::error::Error),
    DecodeError(bincode::error::DecodeError),
    //EncodeError(bincode::error::EncodeError),
    //ClientHandlerError(ClientHandlerError),
    //UnableToConnect,
    ConnectionInterrupted
}

enum ClientType {
    None,
    Receiver(Receiver),
    Sender(Sender)
}


async fn logic(
    processing_tx: mpsc::Sender<ProcessingEvent>, 
    event_tx: mpsc::Sender<EventMessage>, 
    mut event_rx: mpsc::Receiver<EventMessage>
) -> Result<(), HandlerError> {
    let config = bincode::config::standard();

    let mut args = env::args();

    args.next();
    let host = args.next();

    let url = match host {
        Some(host) => { host },
        None => "ws://127.0.0.1:37065".to_owned()
    };

    let ws_stream;

    loop {
        processing_tx.send(ProcessingEvent::ConnectionStatusChanged(ConnectionStatus::Connecting)).await.unwrap();

        match connect_async(&url).await {
            Ok(s) => {
                ws_stream = s.0;
                break;
            },
            Err(err) => {
                println!("{:?}", err);
                processing_tx.send(ProcessingEvent::ConnectionStatusChanged(ConnectionStatus::Disconnected)).await.unwrap();
                tokio::time::sleep(Duration::from_millis(3000)).await;
            },
        }
    }

    processing_tx.send(ProcessingEvent::ConnectionStatusChanged(ConnectionStatus::Connected)).await.unwrap();

    //let (event_tx, mut event_rx) = tokio::sync::mpsc::channel(131072); // 2^17, 128KiB
    //let (message_tx, mut message_rx) = tokio::sync::mpsc::channel(131072); // 2^17, 128KiB
    let (mut write, mut read) = ws_stream.split();

    let read_event_tx = event_tx.clone();
    let read_task = async move {
        let result: Result<(), HandlerError>;
        loop {
            let message = match read.next().await {
                None => { 
                    result = Err(HandlerError::ConnectionInterrupted);
                    break; 
                }
                Some(m) => m
            };

            match message {
                Ok(message) => {
                    let data = message.into_data();
                    let (binary_message, _): (BinaryMessage, usize) = match bincode::decode_from_slice(&data, config) {
                        Ok((b, size)) => (b, size),
                        Err(error) => { 
                            result = Err(HandlerError::DecodeError(error));
                            break;
                        },
                    };

                    read_event_tx.send(EventMessage::ProcessMessage(binary_message)).await.unwrap();
                },
                Err(error) => { 
                    result = Err(HandlerError::TungsteniteError(error)); 
                    break;
                }
            }
        }
        return result;
    };

    let event_processing_tx = processing_tx.clone();
    let event_processor_task = async move {
        let mut client = ClientType::None;

        loop {
            let message = match event_rx.recv().await {
                None => {
                    println!("Unable to get more event messages. Quitting");
                    break;
                },
                Some(m) => m
            };

            match message {
                EventMessage::LogInAsReceiver => {
                    let mut receiver = Receiver::new(&PathBuf::from("receiver"), event_tx.clone(), &config).await.unwrap();

                    match receiver.login("receiver", "h87s8ghegh48ghs4gs84hg8s4h8").await {
                        Ok(_) => {
                            println!("Sent a login request")
                        },
                        Err(e) => {
                            println!("Error occured while logging in: {:?}", e);
                            break;
                        },
                    }

                    client = ClientType::Receiver(receiver);
                },
                EventMessage::LogInAsSender => {
                    let mut sender = Sender::new(&PathBuf::from("sender"),event_tx.clone(), &config).await.unwrap();

                    match sender.login("sender", "vn753498573q0v5983n5789qyunasp8fy3j").await {
                        Ok(_) => {
                            println!("Sent a login request")
                        },
                        Err(e) => {
                            println!("Error occured while logging in: {:?}", e);
                                break;
                        },
                    }

                    client = ClientType::Sender(sender);
                },
                EventMessage::LoggedInAsReceiver(dirs, files) => {
                    event_processing_tx.send(ProcessingEvent::LoggedInAsReceiver(dirs, files)).await.unwrap();

                    match &mut client {
                        ClientType::Receiver(receiver) => {
                            //receiver.logged_in();
                            receiver.request_new_meta().await.unwrap();
                        },
                        _ => {}
                    };
                },
                EventMessage::LoggedInAsSender(dirs, files) => {
                    event_processing_tx.send(ProcessingEvent::LoggedInAsSender(dirs, files)).await.unwrap();
                },
                EventMessage::SendMessage(message) => {
                    match bincode::encode_to_vec(message, config) {
                        Ok(encoded) => {
                            write.send(Message::binary(Bytes::from(encoded))).await.unwrap();
                        },
                        Err(error) => { 
                            println!("Error occurred while encoding a message: {:?}", error);
                        },
                    }
                },
                EventMessage::ProcessMessage(binary_message) => {
                    match &mut client {
                        ClientType::None => {},
                        ClientType::Receiver(receiver) => {
                            match receiver.process_message(&binary_message).await {
                                Ok(_) => {},
                                Err(e) => {
                                    println!("Error occured while processing a message: {:?}", e);
                                }
                            }
                        },
                        ClientType::Sender(sender) => {
                            match sender.process_message(&binary_message).await {
                                Ok(_) => {},
                                Err(e) => {
                                    println!("Error occured while processing a message: {:?}", e);
                                }
                            }
                        },
                    };
                },
                EventMessage::UploadMeta(local_path, remote_path) => {
                    match &mut client {
                        ClientType::Sender(sender) => 'exec: {
                            let file_path = PathBuf::from(&local_path);
                            let file = match File::open(file_path).await {
                                Ok(f) => f,
                                Err(_) => {
                                    event_processing_tx.send(
                                        ProcessingEvent::MetadataCreationFailed(format!("File `{}` cannot be opened", local_path.to_str().unwrap()))
                                    ).await.unwrap();
                                    break 'exec;
                                }
                            };

                            let metadata = file.metadata().await.unwrap();
                            let size = metadata.len();

                            let file_name = local_path.file_name().unwrap().to_str().unwrap().to_owned();
                            let formatted_size = Size::from_bytes(size).format().to_string();

                            println!("Generating metadata...");
                            event_processing_tx.send(
                                ProcessingEvent::MetadataCreationStarted(MetaFileData { name: file_name, formatted_size: formatted_size })
                            ).await.unwrap();

                            sender.create_meta(&local_path, &remote_path).await;
                        },
                        _ => {}
                    };
                },
                EventMessage::UploadMultipleMeta(remote_dir, local_paths) => {
                    match &mut client {
                        ClientType::Sender(sender) => {
                            let mut meta_files: Vec<MetaFileData> = vec![];

                            for local_path in local_paths.clone() {
                                let file_name = local_path.file_name().unwrap().to_str().unwrap().to_owned();
                                let file = match File::open(local_path.clone()).await {
                                    Ok(f) => f,
                                    Err(_) => {
                                        event_processing_tx.send(
                                            ProcessingEvent::MetadataCreationFailed(format!("File `{}` cannot be opened", local_path.to_str().unwrap()))
                                        ).await.unwrap();

                                        continue;
                                    }
                                };

                                let metadata = file.metadata().await.unwrap();
                                let size = metadata.len();

                                let formatted_size = Size::from_bytes(size).format().to_string();

                                meta_files.push(MetaFileData { name: file_name, formatted_size: formatted_size });
                            }

                            println!("Generating metadata...");
                            event_processing_tx.send(
                                ProcessingEvent::MultipleMetadataCreationStarted(meta_files)
                            ).await.unwrap();

                            sender.create_multiple_meta(&local_paths, &remote_dir).await;
                        },
                        _ => {}
                    };
                },
                EventMessage::MetaCreationError => {
                    println!("A error has occured while creating metadata");
                },
                EventMessage::MetaProgressReport(progress) => {
                    event_processing_tx.send(
                        ProcessingEvent::MetadataCreationProgress(progress)
                    ).await.unwrap();
                },
                EventMessage::MetaCreated(entry) => {
                    match &mut client {
                        ClientType::Sender(sender) => {
                            let remote_path = entry.meta.path.clone();
                            sender.on_meta_created(entry).await.unwrap();

                            println!("Metadata is generated. Publishing...");
                            match sender.publish_meta(&remote_path).await {
                                Ok(_) => {
                                    println!("Published metadata");
                                    event_processing_tx.send(ProcessingEvent::MetadataCreated).await.unwrap();
                                },
                                _ => { println!("Unexpected error while publishing meta")}
                            };
                        },
                        _ => {}
                    };
                },
                EventMessage::UpdateReceiverFiles(dirs, files) => {
                    event_processing_tx.send(ProcessingEvent::UpdateReceiverFiles(dirs, files)).await.unwrap();
                },
                EventMessage::UpdateSenderFiles(dirs, files) => {
                    event_processing_tx.send(ProcessingEvent::UpdateSenderFiles(dirs, files)).await.unwrap();
                },
                EventMessage::SenderConnected => {
                    event_processing_tx.send(ProcessingEvent::SenderConnected).await.unwrap();
                },
                EventMessage::SenderDisconnected => {
                    event_processing_tx.send(ProcessingEvent::SenderDisconnected).await.unwrap();
                },
                EventMessage::FileRequested(remote_path, local_path) => {
                    match &mut client {
                        ClientType::Receiver(receiver) => {
                            receiver.request_file(&remote_path, &local_path).await.unwrap();
                        },
                        _ => {}
                    };
                },
                EventMessage::FileResumed(remote_path) => {
                    match &mut client {
                        ClientType::Receiver(receiver) => {
                            receiver.resume_file(&remote_path).await.unwrap();
                        },
                        _ => {}
                    };
                },
                EventMessage::DownloadStopped => {
                    match &mut client {
                        ClientType::Receiver(receiver) => {
                            receiver.stop_transmition().await.unwrap();
                        },
                        _ => {}
                    };
                },
                EventMessage::FilePartIoError => {
                    event_processing_tx.send(ProcessingEvent::FilePartIoError).await.unwrap();
                },
                EventMessage::FileStartedDownloading(remote_path, file_name, formatted_size, total_size) => {
                    event_processing_tx.send(ProcessingEvent::FileStartedDownloading(remote_path, file_name, formatted_size, total_size)).await.unwrap();
                },
                EventMessage::FileDownloadProgress(progress) => {
                    event_processing_tx.send(ProcessingEvent::FileDownloadProgress(progress)).await.unwrap();
                },
                EventMessage::FileFinishedDownloading(_) => {
                    event_processing_tx.send(ProcessingEvent::FileFinishedDownloading).await.unwrap();
                },
                EventMessage::TransferStarted(file) => {

                },
                EventMessage::TransferStopped => {

                },
                EventMessage::TransferFinished => {

                },
                EventMessage::Quit => {
                    println!("Quitting");
                    break;
                }
            }
        }
    };

    tokio::select! {
        rt = read_task => {
            match rt {
                Err(err) => println!("Error occurred in read task: {:?}", err),
                _ => {
                    println!("Read task is cancelled")
                }
            }
        },
        _ = event_processor_task => {
            println!("Event processing task is cancelled")
            /*match et {
                Err(err) => println!("Error occurred in event processor task: {:?}", err),
                _ => {}
            }*/
        }
    };

    processing_tx.send(ProcessingEvent::ConnectionStatusChanged(ConnectionStatus::Disconnected)).await.unwrap();

    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let (processing_tx, processing_rx) = mpsc::channel(1024 * 1024);
    let (event_tx, event_rx) = mpsc::channel(16 * 1024 * 1024);
    
    let context = GuiContext::setup_ui(event_tx.clone(), processing_tx.clone())?;

    context.process_gui_events(processing_rx);

    std::thread::spawn(move || {
        let tokio_rt = tokio::runtime::Runtime::new().unwrap();
        tokio_rt.block_on(async move {
            return logic(processing_tx, event_tx, event_rx).await
        }).unwrap();
    });

    context.run()?;

    Ok(())
}