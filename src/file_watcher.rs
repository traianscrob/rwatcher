use std::collections::HashMap;
use std::ffi::OsStr;
use std::fmt::Error;
use std::fs::Metadata;
use std::fs::{self, DirEntry};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Sender};
use std::sync::Mutex;
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime};

const ALL: &str = "*.*";

#[derive(Debug, Clone)]
pub enum OPERATIONS {
    ADD,
    CHANGE,
    DELETE,
}

#[derive(Debug, Clone)]
enum ChannelOperations {
    CONTINUE,
    EXIT,
}

#[derive(Debug, Clone)]
pub struct OnChangesArgs {
    operation: OPERATIONS,
    files: Vec<PathBuf>,
}

impl OnChangesArgs {
    fn new(operation: OPERATIONS, files: Vec<PathBuf>) -> Self {
        Self { operation, files }
    }

    pub fn operation(&self) -> OPERATIONS {
        self.operation.clone()
    }

    pub fn files(&self) -> &[PathBuf] {
        self.files.as_slice()
    }
}

#[derive(Debug, Clone)]
struct OperationMessage(OPERATIONS, HashMap<PathBuf, Metadata>, SystemTime);

#[derive(Debug, Clone)]
struct ChannelMessage(ChannelOperations, Option<OperationMessage>);

impl ChannelMessage {
    pub fn new(op: ChannelOperations, message: Option<OperationMessage>) -> Self {
        Self(op, message)
    }
}

impl OperationMessage {
    pub fn new(op: OPERATIONS, elems: HashMap<PathBuf, Metadata>, last_sync: SystemTime) -> Self {
        Self(op, elems, last_sync)
    }
}

#[derive(Debug, Clone)]
pub struct FileWatcherOptions {
    directory: &'static str,
    files_extensions: Option<&'static [&'static str]>,
    include_sub_folders: bool,
    refresh_rate_in_milliseconds: u64,
    on_changes: Option<fn(OnChangesArgs)>,
}

impl FileWatcherOptions {
    pub fn new(directory: &'static str) -> Self {
        Self {
            directory,
            files_extensions: None,
            include_sub_folders: false,
            refresh_rate_in_milliseconds: 250,
            on_changes: None,
        }
    }

    pub fn with_extensions(&mut self, extensions: &'static [&'static str]) -> &mut Self {
        self.files_extensions = Some(extensions);

        self
    }

    pub fn with_refresh_rate(&mut self, refresh_rate_in_milliseconds: u64) -> &mut Self {
        self.refresh_rate_in_milliseconds = refresh_rate_in_milliseconds;

        self
    }

    pub fn with_on_changes(&mut self, action: fn(OnChangesArgs)) -> &mut Self {
        self.on_changes = Some(action);

        self
    }
}

#[derive(Debug)]
pub struct FileWatcher {
    dir_path: PathBuf,
    files_extensions: Option<Vec<String>>,
    include_sub_folders: bool,
    is_started: bool,
    last_sync: Option<SystemTime>,
    refresh_rate_in_milliseconds: u64,
    main_thread: Option<JoinHandle<()>>,
    events_thread: Option<JoinHandle<()>>,
    on_changed: Option<fn(OnChangesArgs)>,
    include_all_files: bool,
    channel_sender: Option<Sender<ChannelMessage>>,
}

impl Drop for FileWatcher {
    fn drop(&mut self) {
        self.dir_path.clear();

        self.files_extensions = None;
        self.main_thread = None;
        self.events_thread = None;
        self.last_sync = None;
        self.on_changed = None;
        self.refresh_rate_in_milliseconds = 0;
        self.is_started = false;
        self.include_sub_folders = false;
        self.include_all_files = false;
        self.channel_sender = None;
    }
}

impl FileWatcher {
    pub fn new_with_options(op: &FileWatcherOptions) -> Self {
        let mut result = Self::new(
            op.directory,
            op.files_extensions,
            op.include_sub_folders,
            op.refresh_rate_in_milliseconds,
        );

        if let Some(on_event) = op.on_changes {
            result.on_changes(on_event);
        }

        result
    }

    pub fn new(
        folder: &str,
        extensions: Option<&'static [&str]>,
        include_sub_folders: bool,
        refresh_rate_in_milliseconds: u64,
    ) -> Self {
        let dir_path = PathBuf::from(folder);
        if !dir_path.exists() {
            panic!("The directory '{folder}' does not exist")
        }

        let ext_result = Self::get_parsed_extensions(extensions);
        let result = Self {
            dir_path,
            files_extensions: ext_result.1,
            include_all_files: ext_result.0,
            include_sub_folders,
            is_started: false,
            last_sync: None,
            refresh_rate_in_milliseconds,
            main_thread: None,
            events_thread: None,
            on_changed: None,
            channel_sender: None,
        };

        result
    }

    pub fn watched_dir(&self) -> &str {
        &self.dir_path.as_os_str().to_str().unwrap()
    }

    pub fn extensions(&self) -> Option<&[String]> {
        return match &self.files_extensions {
            Some(exts) => Some(exts.as_slice()),
            None => None,
        };
    }

    pub fn on_changes(&mut self, action: fn(OnChangesArgs)) -> &Self {
        self.on_changed = Some(action);

        self
    }

    pub fn start(&mut self) -> Result<bool, std::io::Error> {
        if self.is_started {
            return Ok(false);
        }

        // communication channel
        let (sender, receiver) = channel::<ChannelMessage>();
        let sender_mutex = Mutex::new(sender.clone());
        let receiver_mutex = Mutex::new(receiver);

        let folder = self.dir_path.clone();
        let refresh_rate = self.refresh_rate_in_milliseconds;
        let func = self.on_changed;
        let dir_path = self.dir_path.clone();
        let include_all_files = self.include_all_files.clone();
        let extensions_mutex = Mutex::new(self.files_extensions.clone());

        //child thread for receiving changed files
        let child = thread::spawn(move || loop {
            let local_receiver = receiver_mutex.lock().unwrap();

            match local_receiver.recv() {
                Ok(value) => match value.0 {
                    ChannelOperations::CONTINUE => {
                        if let Some(op) = value.1 {
                            match op.0 {
                                OPERATIONS::ADD => match func {
                                    Some(f) => {
                                        f(OnChangesArgs::new(
                                            OPERATIONS::ADD,
                                            op.1.into_keys().collect(),
                                        ));
                                    }
                                    _ => {}
                                },
                                OPERATIONS::CHANGE => match func {
                                    Some(f) => {
                                        f(OnChangesArgs::new(
                                            OPERATIONS::CHANGE,
                                            op.1.into_keys().collect(),
                                        ));
                                    }
                                    _ => {}
                                },
                                OPERATIONS::DELETE => match func {
                                    Some(f) => {
                                        f(OnChangesArgs::new(
                                            OPERATIONS::DELETE,
                                            op.1.into_keys().collect(),
                                        ));
                                    }
                                    _ => {}
                                },
                            }
                        }
                    }
                    ChannelOperations::EXIT => {
                        break;
                    }
                },
                Err(error) => {
                    panic!("{}", error)
                }
            }

            drop(local_receiver);
        });

        //main child thread
        let main = thread::spawn(move || {
            let mut all_files = HashMap::<PathBuf, Metadata>::new();

            let extensions = match !include_all_files {
                true => extensions_mutex.lock().unwrap().to_owned(),
                _ => None,
            };

            //load existing files
            if let Some(files) = Self::get_files(&dir_path, include_all_files, &extensions) {
                for entry in files {
                    all_files.insert(entry.0, entry.1);
                }
            }

            //check for directory changes
            let mut dir_meta = fs::metadata(folder.clone()).unwrap();
            loop {
                //if there's no change in the directory do not get files
                let new_meta = fs::metadata(folder.clone()).unwrap();
                if dir_meta.modified().unwrap() == new_meta.modified().unwrap() {
                    thread::sleep(Duration::from_millis(refresh_rate));

                    continue;
                }

                dir_meta = new_meta;

                match Self::get_files(&dir_path, include_all_files, &extensions) {
                    Some(latest_files) => {
                        let added_files = Self::get_added_files(latest_files.clone(), &all_files);
                        let changed_files =
                            Self::get_changed_files(latest_files.clone(), &all_files);
                        let deleted_files =
                            Self::get_deleted_files(latest_files.clone(), &all_files);

                        let local_sender = sender_mutex.lock().unwrap();

                        if added_files.len() > 0 {
                            // update list of all files
                            all_files.extend(added_files.clone());

                            // trigger event for added files
                            if let Err(error) = local_sender.clone().send(ChannelMessage::new(
                                ChannelOperations::CONTINUE,
                                Some(OperationMessage::new(
                                    OPERATIONS::ADD,
                                    added_files.clone(),
                                    SystemTime::now(),
                                )),
                            )) {
                                panic!("Error while sending{}", error);
                            };
                        }

                        if changed_files.len() > 0 {
                            // update list of all files
                            for elem in changed_files.clone().into_iter() {
                                all_files.entry(elem.0).and_modify(|e| *e = elem.1);
                            }

                            // trigger event for added files
                            let _ = local_sender.clone().send(ChannelMessage::new(
                                ChannelOperations::CONTINUE,
                                Some(OperationMessage::new(
                                    OPERATIONS::CHANGE,
                                    changed_files,
                                    SystemTime::now(),
                                )),
                            ));
                        }

                        if deleted_files.len() > 0 {
                            // update list of all files
                            for elem in deleted_files.iter() {
                                all_files.remove(elem.0);
                            }

                            // trigger event for added files
                            let _ = local_sender.clone().send(ChannelMessage::new(
                                ChannelOperations::CONTINUE,
                                Some(OperationMessage::new(
                                    OPERATIONS::DELETE,
                                    deleted_files,
                                    SystemTime::now(),
                                )),
                            ));
                        };

                        drop(local_sender);
                    }
                    None => {}
                }

                thread::sleep(Duration::from_millis(refresh_rate));
            }
        });

        self.main_thread = Some(main);
        self.events_thread = Some(child);
        self.channel_sender = Some(sender.clone());

        self.is_started = !self.is_started;

        Ok(true)
    }

    pub fn stop(&mut self) -> Result<bool, Error> {
        if !self.is_started {
            return Ok(false);
        }

        //send an exit message for the child thread handling events
        let _ = self
            .channel_sender
            .as_ref()
            .unwrap()
            .send(ChannelMessage::new(ChannelOperations::EXIT, None));

        self.events_thread = None;

        self.main_thread = None;
        self.events_thread = None;
        self.is_started = false;

        Ok(true)
    }

    fn get_parsed_extensions(extensions: Option<&[&str]>) -> (bool, Option<Vec<String>>) {
        if let Some(exts) = extensions {
            return match exts.into_iter().any(|e| e.eq(&ALL)) {
                true => (true, Some(vec![ALL.to_string()])),
                false => {
                    let exts: Vec<String> = extensions
                        .unwrap()
                        .iter()
                        .map(|e| e.split("*.").last().unwrap_or_default().to_string())
                        .collect();

                    match !exts
                        .iter()
                        .any(|e| e.chars().any(|c| !(c.is_alphabetic() || c.is_numeric())))
                    {
                        false => {
                            panic!(
                                "An extension should be of the following format: '*.*, *.extension, file_name.extension'"
                            );
                        }
                        true => (false, Some(exts)),
                    }
                }
            };
        }

        (true, None)
    }

    fn filter_func(entry: &DirEntry, extensions: &[&str]) -> bool {
        let file_type = entry.file_type().unwrap();

        match file_type.is_file() {
            true => {
                let path = entry.path();
                let file_name = path.file_name().unwrap();

                let extension = Path::new(file_name)
                    .extension()
                    .and_then(OsStr::to_str)
                    .unwrap_or_default();

                for ext in extensions {
                    if extension.ends_with(ext) {
                        return true;
                    }
                }
            }
            false => return false,
        }

        false
    }

    fn get_files(
        dir_path: &PathBuf,
        include_all_files: bool,
        file_extensions: &Option<Vec<String>>,
    ) -> Option<HashMap<PathBuf, Metadata>> {
        return match fs::read_dir(dir_path) {
            Ok(dir) => {
                let latest_files: HashMap<PathBuf, Metadata> = dir
                    .filter(|f| f.is_ok())
                    .filter(|f| {
                        include_all_files
                            || file_extensions.as_ref().unwrap().iter().any(|x| {
                                let path = f.as_ref().unwrap().path();
                                let extension = Path::new(path.as_os_str())
                                    .extension()
                                    .and_then(OsStr::to_str);
                                return match extension {
                                    Some(e) => e.contains(x),
                                    _ => false,
                                };
                            })
                    })
                    .into_iter()
                    .map(|f| {
                        let file = f.unwrap();

                        (file.path(), file.metadata().unwrap())
                    })
                    .collect();

                Some(latest_files)
            }
            _ => None,
        };
    }

    fn get_changed_files(
        latest_files: HashMap<PathBuf, Metadata>,
        all_files: &HashMap<PathBuf, Metadata>,
    ) -> HashMap<PathBuf, Metadata> {
        let result: HashMap<PathBuf, Metadata> = latest_files
            .into_iter()
            .filter(|f| {
                return match all_files.get(&f.0) {
                    Some(fe) => fe.modified().unwrap() != f.1.modified().unwrap(),
                    _ => false,
                };
            })
            .collect();

        result
    }

    fn get_added_files(
        latest_files: HashMap<PathBuf, Metadata>,
        all_files: &HashMap<PathBuf, Metadata>,
    ) -> HashMap<PathBuf, Metadata> {
        let result: HashMap<PathBuf, Metadata> = latest_files
            .into_iter()
            .filter(|f| {
                let file = &f.0;
                let file_name = file.clone();

                let old_file = all_files.get(&file_name);
                return match old_file {
                    None => true,
                    _ => false,
                };
            })
            .collect();

        result
    }

    fn get_deleted_files(
        latest_files: HashMap<PathBuf, Metadata>,
        all_files: &HashMap<PathBuf, Metadata>,
    ) -> HashMap<PathBuf, Metadata> {
        let result: HashMap<PathBuf, Metadata> = all_files
            .iter()
            .filter(|f| {
                let file = f.0;
                let file_name = file;

                let old_file = latest_files.get(file_name);
                if let None = old_file {
                    return true;
                }

                return false;
            })
            .map(|f| (f.0.clone(), f.1.clone()))
            .collect();

        result
    }

    fn folder_content_changed(folder: &PathBuf, meta: &Metadata) -> bool {
        let new_meta = fs::metadata(folder).unwrap();
        if meta.modified().unwrap() != new_meta.modified().unwrap() {
            return true;
        }

        false
    }

    fn filter_file(
        f: &Result<DirEntry, io::Error>,
        include_all_files: bool,
        extensions_to_check: &Option<&[String]>,
    ) -> bool {
        if !f.is_ok() {
            return false;
        }

        include_all_files
            || extensions_to_check.as_ref().unwrap().iter().any(|x| {
                Path::new(f.as_ref().unwrap().path().as_os_str())
                    .extension()
                    .and_then(OsStr::to_str)
                    .unwrap()
                    .contains(x)
            })
    }
}
