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
pub(crate) struct SearchDir {
    dir_path: PathBuf,
    depth: Option<u16>,
    extensions: Option<Vec<&'static str>>,
    file_names: Option<Vec<&'static str>>,
    include_all_files: bool,
    meta: Metadata,
}

#[derive(Debug, Clone)]
pub struct File {
    path: PathBuf,
    meta: Metadata,
}

impl Eq for File {}

impl PartialEq for File {
    fn eq(&self, other: &Self) -> bool {
        self.path.to_str().unwrap() == other.path.to_str().unwrap()
    }
}

impl Hash for File {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.path.hash(state);
    }
}

impl Display for File {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.path.to_str().unwrap())
    }
}

impl File {
    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    pub fn name(&self) -> &str {
        match self.path.as_os_str().to_str() {
            None => EMPTY_STRING,
            Some(value) => value,
        }
    }

    pub fn extension(&self) -> &str {
        match self.path.extension().and_then(OsStr::to_str) {
            Some(ext) => ext,
            _ => EMPTY_STRING,
        }
    }

    pub fn last_modified(&self) -> Option<SystemTime> {
        match self.meta.modified() {
            Ok(value) => Some(value),
            _ => None,
        }
    }

    pub fn last_accessed(&self) -> Option<SystemTime> {
        match self.meta.accessed() {
            Ok(value) => Some(value),
            _ => None,
        }
    }

    pub fn created(&self) -> Option<SystemTime> {
        match self.meta.created() {
            Ok(value) => Some(value),
            _ => None,
        }
    }
}

impl SearchDir {
    pub fn new(dir_path: PathBuf, depth: Option<u16>, filter: Option<&'static str>) -> Self {
        let path = Path::new(&dir_path);
        if !path.exists() || !path.is_dir() {
            panic!("Directory '{:?}' does not exist", dir_path.clone());
        }

        let mut file_names: Option<Vec<&str>> = None;
        let mut extensions: Option<Vec<&str>> = None;
        let mut include_all_files = false;

        if let Some(f) = filter {
            if !f.is_empty() {
                let split_extensions: Vec<&'static str> = f.split(FILTER_SEPARATORS).collect();
                include_all_files = split_extensions
                    .clone()
                    .into_iter()
                    .any(|e| e.eq(ALL_FILES_FILTER));

                if !include_all_files {
                    let filter_regex = Regex::new(VALID_FILTER_REGEX_PATH).unwrap();
                    let mut exs: Vec<&'static str> = vec![];
                    let mut files: Vec<&'static str> = vec![];

                    for elem in split_extensions.iter() {
                        if !filter_regex.is_match(elem) {
                            panic!("The filter should contain valid file extensions separated by {:?}! i.e: *.*, *.ext, file_name.ext", FILTER_SEPARATORS);
                        }

                        //if we have entry like *.ext
                        if elem.starts_with("*") {
                            let splits: Vec<&'static str> = elem.split(POINT_CHAR).collect();
                            exs.push(splits[1]);
                        } else {
                            files.push(&elem);
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
        }
    }

    pub fn meta(&self) -> &Metadata {
        &self.meta
    }

    pub fn update_metadata(&mut self) {
        self.meta = fs::metadata(self.dir_path.as_path()).unwrap();
    }

    pub fn last_modified(&self) -> Result<SystemTime, io::Error> {
        self.meta.modified()
    }

    pub fn has_changes(&self) -> bool {
        self.last_modified().unwrap()
            != fs::metadata(self.dir_path.as_path())
                .unwrap()
                .modified()
                .unwrap()
    }

    pub fn get_files(&self) -> HashSet<File> {
        let mut result: HashSet<File> = HashSet::new();

        let rec_limit = match self.depth {
            Some(value) => value,
            _ => 128,
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

    fn get_files_internal(
        dir: &PathBuf,
        depth: u16,
        extensions: &Option<Vec<&str>>,
        file_names: &Option<Vec<&str>>,
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
                    let file_name = path_buf.file_name().unwrap().to_str().unwrap();
                    return names.as_slice().contains(&file_name);
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
                    result.insert(File {
                        path: file.path(),
                        meta: file.metadata().unwrap(),
                    });
                }
            }
        }
    }
}
