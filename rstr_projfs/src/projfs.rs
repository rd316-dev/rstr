use std::{collections::HashMap, path::PathBuf, str::FromStr};

use rstr_core::meta::Meta;
use tokio::sync::mpsc;
use windows_projfs::{DirectoryEntry, DirectoryInfo, FileInfo, ProjectedFileSystemSource};

pub struct DirNode {
    info: DirectoryInfo,
    files: Vec<FileInfo>,
    dirs: Vec<DirectoryInfo>
}

pub fn create_dir_map(meta_entries: &[Meta]) -> HashMap<String, DirNode> {
    let mut map: HashMap<String, DirNode> = HashMap::new();

    {
        let root_info = DirectoryInfo {
            directory_name: "".to_owned(),
            
            .. Default::default()
        };

        let root = DirNode { info: root_info, files: vec![], dirs: vec![] };
        map.insert("".to_owned(), root);
    }

    for e in meta_entries {
        let mut parts: Vec<&str> = e.path.split("/").collect();
        let file_name = parts.remove(parts.len() - 1).to_owned();

        {
            let parts = parts.clone();

            let mut root_path = "".to_owned();
            for dir_name in parts {
                let current_path = root_path.clone() + (if root_path.len() == 1 { "" } else { "/"} ) + dir_name;
                match map.get(&current_path) {
                    Some(_) => continue,
                    _ => {}
                }

                let dir_info = DirectoryInfo {
                    directory_name: dir_name.to_owned(),

                    .. Default::default()
                };

                let root_node: &mut DirNode = map.get_mut(&root_path).unwrap();
                root_node.dirs.push(dir_info.clone());

                let node = DirNode {
                    info: dir_info,
                    files: vec![],
                    dirs: vec![]
                };

                map.insert(current_path.clone(), node);

                root_path = current_path;
            }
        }

        let dir_name = parts.last().unwrap().to_owned();

        let path = parts.join("/");

        let file_info = FileInfo {
            file_name: file_name,
            file_size: e.size,
            file_attributes: 0,
            creation_time: e.file_modified as u64,
            last_access_time: e.file_modified as u64,
            last_write_time: e.file_modified as u64,
        };

        match map.get_mut(&path) {
            Some(entry) => {
                entry.files.push(file_info);
            },
            None => { // this branch is pretty much useless
                let dir_info = DirectoryInfo {
                    directory_name: dir_name.to_owned(),

                    .. Default::default()
                };

                let node = DirNode {
                    info: dir_info,
                    files: vec![file_info],
                    dirs: vec![]
                };

                map.insert(path, node);
            }
        }
    }

    return map;
}

/*struct RemoteFileSystem {
    //file_paths: Vec<String>,
    root_node: DirNode,
    dir_map: HashMap<String, DirNode>
    //tx: mpsc::Sender<EventMessage>
}

impl RemoteFileSystem {
    fn set_file_paths(paths: &[&str]) {
        
    }
}

impl ProjectedFileSystemSource for RemoteFileSystem {
    fn list_directory(&self, path: &std::path::Path) -> Vec<windows_projfs::DirectoryEntry> {
        path.to_str();

        vec![]
    }

    fn stream_file_content(
        &self,
        path: &std::path::Path,
        byte_offset: usize,
        length: usize,
    ) -> std::io::Result<Box<dyn std::io::Read>> {
        
    }
}*/