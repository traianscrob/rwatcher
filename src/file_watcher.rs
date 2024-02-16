use std::collections::HashSet;
use std::fmt::{Display, Error};
use std::fs::Metadata;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime};

use crate::search_dir::{File, SearchDir};

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy,PartialEq, Eq, Hash)]
    pub struct NotifyFilters : u8 {
        const Attributes = 0;
        const CreationTime = 1;
        const DirectoryName = 2;
        const FileName = 3;
        const LastAccess = 4;
        const LastWrite = 5;
        const Security = 6;
        const Size = 7;
    }
}

impl Display for NotifyFilters {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", *self)
    }
}

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
    files: HashSet<PathBuf>,
}

impl OnChangesArgs {
    fn new(operation: OPERATIONS, files: HashSet<PathBuf>) -> Self {
        Self { operation, files }
    }

    pub fn operation(&self) -> OPERATIONS {
        self.operation.clone()
    }

    pub fn files(&self) -> &HashSet<PathBuf> {
        &self.files
    }
}

#[derive(Debug, Clone)]
struct OperationMessage(OPERATIONS, HashSet<File>);

#[derive(Debug, Clone)]
struct ChannelMessage(ChannelOperations, Option<OperationMessage>);

impl ChannelMessage {
    pub fn new(op: ChannelOperations, message: Option<OperationMessage>) -> Self {
        Self(op, message)
    }
}

impl OperationMessage {
    pub fn new(op: OPERATIONS, elems: HashSet<File>) -> Self {
        Self(op, elems)
    }
}

#[derive(Debug, Clone)]
pub struct FileWatcherOptions {
    directory: &'static str,
    filter: Option<&'static str>,
    include_sub_folders: bool,
    refresh_rate_mils: u64,
    on_changes: Option<fn(OnChangesArgs)>,
    notify_filters: NotifyFilters,
    dir_depth: Option<u16>,
}

impl FileWatcherOptions {
    pub fn new(directory: &'static str) -> Self {
        Self {
            directory,
            filter: None,
            include_sub_folders: false,
            refresh_rate_mils: 250,
            on_changes: None,
            notify_filters: NotifyFilters::LastWrite,
            dir_depth: None,
        }
    }

    pub fn with_filter(&mut self, filter: &'static str) -> &mut Self {
        self.filter = Some(filter);

        self
    }

    pub fn with_refresh_rate(&mut self, refresh_rate_in_milliseconds: u64) -> &mut Self {
        self.refresh_rate_mils = refresh_rate_in_milliseconds;

        self
    }

    pub fn with_on_changes(&mut self, action: fn(OnChangesArgs)) -> &mut Self {
        self.on_changes = Some(action);

        self
    }
    pub fn with_notify_filters(&mut self, filters: NotifyFilters) -> &mut Self {
        self.notify_filters = filters;

        self
    }
    pub fn with_directory_depth(&mut self, depth: u16) -> &mut Self {
        self.dir_depth = Some(depth);

        self
    }
}

#[derive(Debug)]
pub struct FileWatcher {
    dir_path: PathBuf,
    filter: Option<&'static str>,
    is_started: bool,
    last_sync: Option<SystemTime>,
    refresh_rate_in_milliseconds: u64,
    main_thread: Option<JoinHandle<()>>,
    events_thread: Option<JoinHandle<()>>,
    on_changed: Option<fn(OnChangesArgs)>,
    channel_sender: Option<Sender<ChannelMessage>>,
    notify_filters: NotifyFilters,
    dir_depth: Option<u16>,
}

impl Drop for FileWatcher {
    fn drop(&mut self) {
        self.dir_path.clear();

        self.filter = None;
        self.main_thread = None;
        self.events_thread = None;
        self.last_sync = None;
        self.on_changed = None;
        self.refresh_rate_in_milliseconds = 0;
        self.is_started = false;
        self.channel_sender = None;
    }
}

impl FileWatcher {
    pub fn new_with_options(op: &FileWatcherOptions) -> Self {
        let mut result = Self::new(op.directory, op.filter, op.refresh_rate_mils, op.dir_depth);

        result.notify_filters = op.notify_filters;
        result.dir_depth = op.dir_depth;

        if let Some(on_event) = op.on_changes {
            result.on_changes(on_event);
        }

        result
    }

    pub fn new(
        dir: &str,
        filter: Option<&'static str>,
        refresh_rate_in_milliseconds: u64,
        dir_depth: Option<u16>,
    ) -> Self {
        let dir_path = PathBuf::from(dir);
        if !dir_path.exists() {
            panic!("The directory '{dir}' does not exist")
        }

        let result = Self {
            dir_path,
            filter,
            is_started: false,
            last_sync: None,
            refresh_rate_in_milliseconds,
            main_thread: None,
            events_thread: None,
            on_changed: None,
            channel_sender: None,
            notify_filters: NotifyFilters::LastWrite,
            dir_depth: None,
        };

        result
    }

    pub fn watched_dir(&self) -> &str {
        &self.dir_path.as_os_str().to_str().unwrap()
    }

    pub fn filter(&self) -> Option<&str> {
        return match &self.filter {
            Some(f) => Some(f),
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

        let refresh_rate: u64 = self.refresh_rate_in_milliseconds;
        let func = self.on_changed;
        let dir_path = self.dir_path.clone();
        let filter_mutex = Mutex::new(self.filter.clone());
        let notify_filters_mutex = Arc::new(Mutex::new(self.notify_filters));

        //child thread for receiving changed files
        let child = thread::spawn(move || loop {
            match receiver_mutex.lock().unwrap().recv() {
                Ok(value) => match value.0 {
                    ChannelOperations::CONTINUE => {
                        if let Some(op) = value.1 {
                            match func {
                                Some(f) => {
                                    let operation = op.0;
                                    let res: HashSet<PathBuf> =
                                        op.1.into_iter().map(|f| f.path().clone()).collect();
                                    f(OnChangesArgs::new(operation, res));
                                }
                                _ => {}
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
        });

        let depth = self.dir_depth.clone();
        //main child thread
        let main = thread::spawn(move || {
            let mut all_files = HashSet::<File>::new();

            let filter = filter_mutex.lock().unwrap();
            let mut search_dir = SearchDir::new(dir_path.clone(), depth, *filter);

            let notify_filters = Arc::clone(&notify_filters_mutex);

            //load existing files
            for file in Self::get_files(&search_dir, *notify_filters.lock().unwrap()) {
                all_files.insert(file);
            }

            //check for directory changes
            search_dir.update_metadata();
            loop {
                //if there's no change in the directory do not get files
                if !search_dir.has_changes() {
                    thread::sleep(Duration::from_millis(refresh_rate));

                    continue;
                }

                let latest_files = Self::get_files(&search_dir, *notify_filters.lock().unwrap());
                let added_files: HashSet<File> = latest_files
                    .difference(&all_files)
                    .map(|fe| fe.clone())
                    .collect();

                let deleted_files: HashSet<File> = all_files
                    .difference(&latest_files)
                    .map(|fe| fe.clone())
                    .collect();

                let mut changed_files: HashSet<File> = HashSet::new();
                for file in latest_files.iter() {
                    let old_file = all_files.get(&file);
                    match old_file {
                        Some(fe) => {
                            // file was changed
                            if fe.last_modified().unwrap() != file.last_modified().unwrap() {
                                changed_files.insert(file.clone());

                                all_files.remove(&file);
                                all_files.insert(file.clone());
                            }
                        }
                        _ => (),
                    }
                }

                let local_sender = sender_mutex.lock().unwrap();
                if added_files.len() > 0 {
                    all_files = all_files.union(&added_files).map(|f| f.clone()).collect();

                    // trigger event for added files
                    if let Err(error) = local_sender.clone().send(ChannelMessage::new(
                        ChannelOperations::CONTINUE,
                        Some(OperationMessage::new(OPERATIONS::ADD, added_files)),
                    )) {
                        panic!("Error while sending{}", error);
                    };
                }

                // trigger event for changed files
                if changed_files.len() > 0 {
                    let _ = local_sender.clone().send(ChannelMessage::new(
                        ChannelOperations::CONTINUE,
                        Some(OperationMessage::new(OPERATIONS::CHANGE, changed_files)),
                    ));
                }

                if deleted_files.len() > 0 {
                    // trigger event for added files
                    let _ = local_sender.clone().send(ChannelMessage::new(
                        ChannelOperations::CONTINUE,
                        Some(OperationMessage::new(
                            OPERATIONS::DELETE,
                            deleted_files.clone(),
                        )),
                    ));

                    for f in deleted_files.iter() {
                        all_files.remove(&f);
                    }
                };

                drop(local_sender);

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

    fn get_files(search_dir: &SearchDir, notify_filters: NotifyFilters) -> HashSet<File> {
        let meta: &Metadata = search_dir.meta();
        let mut result: HashSet<File> = HashSet::new();

        for file in search_dir.get_files() {
            if Self::apply_notify_filters(&file, meta, notify_filters) {
                result.insert(file);
            }
        }

        result
    }

    fn apply_notify_filters(
        file: &File,
        old_meta: &Metadata,
        notify_filters: NotifyFilters,
    ) -> bool {
        let last_write = notify_filters.contains(NotifyFilters::LastWrite)
            && old_meta.modified().unwrap() != file.last_modified().unwrap();

        if last_write {
            return last_write;
        }

        let last_access = notify_filters.contains(NotifyFilters::LastAccess)
            && old_meta.accessed().unwrap() != file.last_modified().unwrap();

        if last_access {
            return last_access;
        }

        let creation_time = notify_filters.contains(NotifyFilters::CreationTime)
            && old_meta.created().unwrap() != file.created().unwrap();

        creation_time
    }
}
