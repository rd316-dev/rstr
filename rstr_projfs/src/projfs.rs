use std::{collections::HashMap, ops::DerefMut, path::PathBuf, str::FromStr, vec};

use rstr_core::meta::Meta;
use tokio::sync::mpsc;
use windows_projfs::{DirectoryEntry, DirectoryInfo, FileInfo, ProjectedFileSystemSource};

pub enum RemoteEntry {
    Directory(RemoteDirectory),
    File(RemoteFile)
}

pub struct RemoteDirectory {
    pub name: String,
    pub info: DirectoryInfo,
    pub children: Vec<RemoteEntry>
}

pub struct RemoteFile {
    pub name: String,
    pub info: FileInfo,
    pub meta: Meta
}

impl RemoteEntry {
    fn name(&self) -> &str {
        match self {
            RemoteEntry::Directory(remote_directory) => &remote_directory.name,
            RemoteEntry::File(remote_file) => &remote_file.name,
        }
    }

    fn info(&self) -> DirectoryEntry {
        match self {
            RemoteEntry::Directory(dir) => DirectoryEntry::Directory(dir.info.clone()),
            RemoteEntry::File(file) => DirectoryEntry::File(file.info.clone()),
        }
    }

    fn new_dir(name: &str) -> Self {
        RemoteEntry::Directory( RemoteDirectory {
            name: name.to_owned(), 
            info: DirectoryInfo { directory_name: name.to_owned(), ..Default::default() }, 
            children: vec![]
        })
    }

    fn new_file(name: &str, meta: &Meta) -> Self {
        RemoteEntry::File( RemoteFile { 
            name: name.to_owned(), 
            info: FileInfo {
                file_name: name.to_owned(), 
                file_size: meta.size, 
                file_attributes: 1, 
                creation_time: meta.file_modified as u64, 
                
                ..Default::default()
            },
            meta: meta.clone()
        })
    }

    fn find_child<'a>(&'a self, name: &str) -> Option<&'a Self> {
        match self {
            RemoteEntry::File(_) => None,
            RemoteEntry::Directory(remote_directory) => {
                remote_directory.children.iter().find(|c| c.name() == name)
            },
        }
    }

    fn find_child_mut<'a>(&'a mut self, name: &str) -> Option<&'a mut Self> {
        match self {
            RemoteEntry::File(_) => None,
            RemoteEntry::Directory(remote_directory) => {
                remote_directory.children.iter_mut().find(|c| c.name() == name)
            },
        }
    }

    fn traverse<'a>(&'a self, path: &str) -> Option<&'a Self> {
        let parts: Vec<&str> = path.split("/").collect();

        let mut current = self;
        for i in 0..parts.len() {
            let name = parts[i];

            current = current.find_child(name)?;
        }

        Some(current)
    }

    fn traverse_mut<'a>(&'a mut self, path: &str) -> Option<&'a mut Self> {
        let parts: Vec<&str> = path.split("/").collect();

        let mut current = self;
        for i in 0..parts.len() {
            let name = parts[i];

            current = current.find_child_mut(name)?;
        }

        Some(current)
    }

    fn add<'a>(&'a mut self, new_entry: RemoteEntry) -> Option<&'a mut Self> {
        match self {
            RemoteEntry::Directory(remote_directory) => {
                remote_directory.children.push(new_entry);
                remote_directory.children.last_mut()
            },
            RemoteEntry::File(_) => None,
        }
    }

    fn get_or_add<'a, F>(&'a mut self, name: &str, entry_creator: F) -> Option<&'a mut Self> 
    where F: FnOnce() -> RemoteEntry {
        match self {
            RemoteEntry::File(_) => None,
            RemoteEntry::Directory(remote_directory) => {
                for i in 0..remote_directory.children.len() {
                    let entry = &remote_directory.children[i];
                    if entry.name() == name {
                        return Some(remote_directory.children.get_mut(i).unwrap())
                    }
                }

                let new_entry = entry_creator();

                remote_directory.children.push(new_entry);
                remote_directory.children.last_mut()
            },
        }
    }

    fn add_at_path<'a>(&'a mut self, path: &str, new_entry: RemoteEntry) -> Option<&'a mut Self> {
        let parts: Vec<&str> = path.split("/").collect();

        let mut current = self;
        for i in 0..parts.len() {
            let name = parts[i];

            current = current.get_or_add(name, || RemoteEntry::new_dir(name) )?;
        }

        current.add(new_entry);

        None
    }
}

struct RemoteFileSystem {
    tree: RemoteEntry
}

impl RemoteFileSystem {
    pub fn create_from_entries(meta_entries: &[Meta]) -> Self {
        let mut root = RemoteEntry::new_dir("");

        for meta in meta_entries {
            let mut parts: Vec<&str> = meta.path.split("/").collect();
            let file_name = parts.remove(parts.len() - 1);

            let file = RemoteEntry::new_file(file_name, meta);

            let dir_path = parts.join("/");
            root.add_at_path(&dir_path, file);
        }

        RemoteFileSystem { tree: root }
    }
}

impl ProjectedFileSystemSource for RemoteFileSystem {
    fn list_directory(&self, path: &std::path::Path) -> Vec<windows_projfs::DirectoryEntry> {
        let absolute_path = &std::path::absolute(path).unwrap().to_str().unwrap().to_owned();
        println!("Requested listing of '{}'", absolute_path);

        match self.tree.traverse(&absolute_path) {
            Some(RemoteEntry::Directory(dir)) => {
                dir.children.iter().map(|d| d.info()).collect::<Vec<DirectoryEntry>>()
            }
            Some(RemoteEntry::File(_)) => vec![],
            None => vec![]
        }
    }

    fn stream_file_content(
        &self,
        path: &std::path::Path,
        byte_offset: usize,
        length: usize,
    ) -> std::io::Result<Box<dyn std::io::Read>> {
        Err(std::path::absolute(path).unwrap_err())
        //println!("Requested content of '{}'", absolute_path);
    }
}