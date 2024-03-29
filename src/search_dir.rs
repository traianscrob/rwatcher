#![recursion_limit = "255"]

use core::panic;
use regex::Regex;
use std::collections::HashSet;
use std::ffi::OsStr;
use std::fmt::Display;
use std::fs::{self, Metadata};
use std::hash::Hash;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const EMPTY_STRING: &str = "";
const POINT_CHAR: char = '.';
const ALL_FILES_FILTER: &str = "*.*";
const FILTER_SEPARATORS: &[char] = &[';', ','];
const VALID_FILTER_REGEX_PATH: &str =
    r"^\*\.\*$|^\*\.([a-zA-Z0-9])+$|^([a-zA-Z0-9])+\.([a-zA-Z0-9])+$";

#[derive(Debug, Clone)]
pub struct SearchDir {
    dir_path: PathBuf,
    depth: Option<u8>,
    extensions: Option<Vec<String>>,
    file_names: Option<Vec<String>>,
    include_all_files: bool,
    last_synced: Option<SystemTime>,
    meta: Metadata,
}

#[derive(Debug, Clone)]
pub struct File {
    name: String,
    last_modified: Option<SystemTime>,
    last_accessed: Option<SystemTime>,
    created: SystemTime,
}

impl Eq for File {}

impl PartialEq for File {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl Hash for File {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.name.hash(state);
    }
}

impl Display for File {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

impl File {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn last_modified(&self) -> Option<SystemTime> {
        self.last_modified
    }

    pub fn last_accessed(&self) -> Option<SystemTime> {
        self.last_accessed
    }

    pub fn created(&self) -> SystemTime {
        self.created
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RenamedFileEntry(String, String);
impl RenamedFileEntry {
    pub fn new(name: &str, old_name: &str) -> Self {
        Self(name.to_string(), old_name.to_string())
    }

    pub fn name(&self) -> &str {
        &self.0
    }

    pub fn old_name(&self) -> &str {
        &self.1
    }
}

impl SearchDir {
    pub fn new(dir_path: PathBuf, depth: Option<u8>, filter: Option<String>) -> Self {
        let path = Path::new(&dir_path);
        if !path.exists() || !path.is_dir() {
            panic!("Directory '{:?}' does not exist", dir_path.clone());
        }

        let mut file_names: Option<Vec<String>> = None;
        let mut extensions: Option<Vec<String>> = None;
        let mut include_all_files = false;

        if let Some(file) = filter {
            if !file.is_empty() {
                let split_extensions: Vec<String> = file
                    .split(FILTER_SEPARATORS)
                    .map(|f| f.to_string())
                    .collect();
                include_all_files = split_extensions
                    .clone()
                    .into_iter()
                    .any(|e| e.eq(ALL_FILES_FILTER));

                if !include_all_files {
                    let filter_regex = Regex::new(VALID_FILTER_REGEX_PATH).unwrap();
                    let mut exs: Vec<String> = vec![];
                    let mut files: Vec<String> = vec![];

                    for elem in split_extensions {
                        if !filter_regex.is_match(elem.as_str()) {
                            panic!("The filter should contain valid file extensions separated by {:?}! i.e: *.*, *.ext, file_name.ext", FILTER_SEPARATORS);
                        }

                        //if we have entry like *.ext
                        if elem.starts_with("*") {
                            let splits: Vec<&str> = elem.split(POINT_CHAR).collect();
                            exs.push(splits[1].to_string());
                        } else {
                            files.push(elem.to_string());
                        }
                    }

                    extensions = Some(exs);
                    file_names = Some(files);
                }
            }
        }

        Self {
            dir_path: dir_path.clone(),
            depth,
            meta: path.metadata().unwrap(),
            extensions,
            file_names,
            include_all_files,
            last_synced: None,
        }
    }

    pub fn metadata(&self) -> &Metadata {
        &self.meta
    }

    pub fn sync_metadata(&mut self) {
        self.meta = fs::metadata(self.dir_path.as_path()).unwrap();
    }

    pub fn last_modified(&self) -> Result<SystemTime, io::Error> {
        self.meta.modified()
    }

    pub fn has_changed(&self) -> bool {
        self.last_modified().unwrap()
            != fs::metadata(self.dir_path.as_path())
                .unwrap()
                .modified()
                .unwrap()
    }

    pub fn get_files(&self) -> HashSet<File> {
        let mut result: HashSet<File> = HashSet::new();

        let rec_limit: u8 = match self.depth {
            Some(value) => value,
            _ => u8::MAX - 1,
        };

        Self::get_files_internal(
            &self.dir_path,
            rec_limit + 1,
            &self.extensions,
            &self.file_names,
            &mut result,
        );

        result
    }

    pub fn get_all_files(dir_path: &str) -> HashSet<File> {
        let path = Self::validate_dir_path(dir_path);

        let mut result: HashSet<File> = HashSet::new();
        let rec_limit: u8 = u8::MAX - 1;

        Self::get_files_internal(&path, rec_limit + 1, &None, &None, &mut result);

        result
    }

    fn validate_dir_path(dir_path: &str) -> PathBuf {
        if dir_path.is_empty() {
            panic!("The directory path cannot be empty!")
        }

        let path = Path::new(dir_path);
        if !path.exists() || !path.is_dir() {
            panic!("Directory '{:?}' does not exist", dir_path);
        }

        path.to_path_buf()
    }

    fn get_files_internal(
        dir: &PathBuf,
        depth: u8,
        extensions: &Option<Vec<String>>,
        file_names: &Option<Vec<String>>,
        result: &mut HashSet<File>,
    ) {
        if depth == 0 {
            return;
        }

        if let Ok(read_dir) = fs::read_dir(dir) {
            for dir_entry in read_dir.filter(|f| {
                let entry = f.as_ref().unwrap();
                let path_buf = entry.path().clone();
                let file_type = entry.file_type().unwrap();

                if !file_type.is_file() {
                    return true;
                }

                if let Some(exts) = extensions {
                    let file_ext = path_buf.as_path().extension().and_then(OsStr::to_str);
                    if let Some(extension) = file_ext {
                        return exts.iter().any(|e| e.contains(extension));
                    }
                }

                if let Some(names) = file_names {
                    let file_name = path_buf.file_name().unwrap().to_str().unwrap().to_string();
                    return names.contains(&file_name);
                }

                true
            }) {
                let file = dir_entry.unwrap();

                if file.file_type().unwrap().is_dir() {
                    Self::get_files_internal(
                        &file.path(),
                        depth - 1,
                        extensions,
                        file_names,
                        result,
                    );
                } else {
                    let meta = file.metadata().unwrap();
                    result.insert(File {
                        name: String::from(file.path().to_str().unwrap()),
                        created: meta.created().unwrap(),
                        last_modified: Some(meta.modified().unwrap()),
                        last_accessed: Some(meta.accessed().unwrap()),
                    });
                }
            }
        }
    }
}
