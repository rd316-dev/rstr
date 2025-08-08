use std::{path::PathBuf};

use log::{error, info};
use tokio::{net::{TcpListener}, sync::mpsc};

use crate::server::Server;

mod server;

#[cfg(unix)]
fn setup_logging() {
    use log::LevelFilter;
    use simple_logger::SimpleLogger;
    SimpleLogger::new().with_level(LevelFilter::Info).init().unwrap();

    info!("Setting up loggers for UNIX");
}

#[cfg(windows)]
fn setup_logging() {
    use log::LevelFilter;
    use simple_logger::SimpleLogger;
    SimpleLogger::new().with_level(LevelFilter::Info).init().unwrap();

    info!("Setting up loggers for Windows");
}

#[tokio::main]
async fn main() {
    setup_logging();

    let port = std::env::var("RSTR_SERVER_PORT").unwrap_or("37065".to_owned());
    let addr = "0.0.0.0:".to_string() + &port;

    let try_socket = TcpListener::bind(&addr).await;
    let listener = try_socket.expect("Failed to bind");
    info!("Listening on: {}", addr);

    let data_dir_path = std::env::var("RSTR_SERVER_DATA_DIR").unwrap_or("server".to_owned());
    let data_dir = PathBuf::from(data_dir_path);
    info!("Data directory: {}", std::path::absolute(&data_dir).unwrap().to_str().unwrap());

    let config = bincode::config::legacy();

    let (message_tx, message_rx) = mpsc::channel(1024*1024);

    let mut server = match Server::new(&data_dir, message_rx, &config).await {
        Ok(s) => s,
        Err(error) => {
            error!("Error occured while creating a server: {:?}", error);
            return;
        },
    };

    tokio::spawn(async move {
        server.handle_clients().await;
    });

    while let Ok((stream, addr)) = listener.accept().await {
        let config_copy = config.clone();

        tokio::spawn(Server::handle_connection(stream, addr, message_tx.clone(), config_copy));
    }
}