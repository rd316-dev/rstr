use std::{fmt::Display, fs::{File, OpenOptions}, io::{BufRead, BufReader, Read}, path::{self, PathBuf}};

#[derive(Debug, Clone)]
pub struct ClientConfig {
    pub url: String,
    pub receiver_username: String,
    pub receiver_password: String,
    pub sender_username: String,
    pub sender_password: String
}

impl ClientConfig {
    pub fn load(path: &PathBuf) -> ClientConfig {
        let file_path = path::absolute(path.join("config")).unwrap();
        println!("Opening config file at {:?}", &file_path);

        let mut url = None;
        let mut receiver_username = None;
        let mut receiver_password = None;
        let mut sender_username = None;
        let mut sender_password = None;

        match File::open(&file_path) {
            Ok(file) => {
                let buf = BufReader::new(file);
                for l in buf.lines() {
                    let line = match l {
                        Ok(l) => l,
                        Err(_) => break,
                    };

                    let line = line.trim();

                    if line.starts_with("#") {
                        continue;
                    }

                    if line.len() == 0 {
                        continue;
                    }

                    let colon = match line.find(":") {
                        Some(c) => c,
                        None => continue,
                    };

                    let property_name = line[0..colon].trim().to_lowercase().to_owned();
                    let property_value = line[(colon + 1)..].trim().to_owned();

                    match property_name.as_str() {
                        "url" => { url = Some(property_value) },
                        "receiver_username" => { receiver_username = Some(property_value) },
                        "receiver_password" => { receiver_password = Some(property_value) },
                        "sender_username" => { sender_username = Some(property_value) },
                        "sender_password" => { sender_password = Some(property_value) },
                        _ => {}
                    }
                }
            },
            Err(_) => {
                let prefix = file_path.parent().unwrap();
                std::fs::create_dir_all(prefix).expect("Unable to create a file tree for the configuration file");
                File::create_new(&file_path).expect("Unable to create a configuration file");
            }
        };

        return ClientConfig {
            url: url.unwrap_or("ws://127.0.0.1:37065".to_owned()),
            receiver_username: receiver_username.unwrap_or("receiver".to_owned()),
            receiver_password: receiver_password.unwrap_or("badfea6f-4732-4fc6-acf8-796cc45cc0fa".to_owned()),
            sender_username: sender_username.unwrap_or("sender".to_owned()),
            sender_password: sender_password.unwrap_or("5c6547d2-e2b6-448c-b5b9-e84e939b460a".to_owned()),
        }
    }
}