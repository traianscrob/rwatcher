mod events;
pub mod search_dir;

use std::collections::HashSet;
use std::fmt::{Debug, Display, Error};
use std::fs::Metadata;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime};

use search_dir::RenamedFileEntry;

use crate::events::{
    OnChangedEventArgs, OnCreatedEventArgs, OnDeletedEventArgs, OnRenamedEventArgs,
};
use crate::search_dir::{File, SearchDir};

#[derive(Debug, Clone)]
pub enum OPERATION {
    CREATE(HashSet<File>),
    CHANGE(HashSet<File>),
    DELETE(HashSet<File>),
    RENAME(HashSet<RenamedFileEntry>),
    ERROR(Error),
}

//enums
#[derive(Debug, Clone)]
enum ChannelOperation {
    CONTINUE(OPERATION),
    EXIT,
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
pub struct FileWatcherOptions {
    dir: String,
    filter: Option<String>,
    refresh_rate_mils: u64,
    on_created: Option<fn(OnCreatedEventArgs)>,
    on_deleted: Option<fn(OnDeletedEventArgs)>,
    on_changed: Option<fn(OnChangedEventArgs)>,
    on_renamed: Option<fn(OnRenamedEventArgs)>,
    notify_filters: NotifyFilters,
    dir_depth: Option<u8>,
}

impl FileWatcherOptions {
    pub fn new(directory: &str) -> Self {
        Self {
            dir: directory.to_string(),
            filter: None,
            refresh_rate_mils: 250,
            on_changed: None,
            on_created: None,
            on_deleted: None,
            on_renamed: None,
            dir_depth: None,
            notify_filters: NotifyFilters::LastWrite,
        }
    }

    pub fn with_filter(&mut self, filter: &str) -> &mut Self {
        self.filter = Some(filter.to_string());

        self
    }

    pub fn with_refresh_rate(&mut self, refresh_rate_in_milliseconds: u64) -> &mut Self {
        self.refresh_rate_mils = refresh_rate_in_milliseconds;

        self
    }

    pub fn with_on_changed(&mut self, action: fn(OnChangedEventArgs)) -> &mut Self {
        self.on_changed = Some(action);

        self
    }

    pub fn with_on_created(&mut self, event: fn(OnCreatedEventArgs)) -> &mut Self {
        self.on_created = Some(event);

        self
    }

    pub fn with_on_deleted(&mut self, event: fn(OnDeletedEventArgs)) -> &mut Self {
        self.on_deleted = Some(event);

        self
    }

    pub fn with_on_renamed(&mut self, event: fn(OnRenamedEventArgs)) -> &mut Self {
        self.on_renamed = Some(event);

        self
    }

    pub fn with_notify_filters(&mut self, filters: NotifyFilters) -> &mut Self {
        self.notify_filters = filters;

        self
    }
    pub fn with_directory_depth(&mut self, depth: u8) -> &mut Self {
        self.dir_depth = Some(depth);

        self
    }
}

#[derive(Debug)]
pub struct FileWatcher {
    dir_path: PathBuf,
    filter: Option<String>,
    last_sync: Option<SystemTime>,
    refresh_rate_in_milliseconds: u64,
    main_thread: Option<JoinHandle<()>>,
    events_thread: Option<JoinHandle<()>>,
    on_created: Option<fn(OnCreatedEventArgs)>,
    on_deleted: Option<fn(OnDeletedEventArgs)>,
    on_changed: Option<fn(OnChangedEventArgs)>,
    on_renamed: Option<fn(OnRenamedEventArgs)>,
    channel_sender: Option<Sender<ChannelOperation>>,
    notify_filters: NotifyFilters,
    dir_depth: Option<u8>,
    is_started: Arc<Mutex<bool>>,
}

impl Drop for FileWatcher {
    fn drop(&mut self) {
        if *self.is_started.lock().unwrap() {
            let _ = self.stop();
        }

        self.dir_path.clear();

        self.filter = None;
        self.last_sync = None;
        self.on_changed = None;
        self.channel_sender = None;
        self.refresh_rate_in_milliseconds = 0;
        self.main_thread = None;
        self.events_thread = None;
        self.on_created = None;
        self.on_deleted = None;
        self.on_renamed = None;

        drop(self.is_started.lock());
    }
}

impl FileWatcher {
    pub fn new_with_options(op: &FileWatcherOptions) -> Self {
        let mut result = Self::new(
            op.dir.as_str(),
            op.filter.clone(),
            op.refresh_rate_mils,
            op.dir_depth,
        );

        result.notify_filters = op.notify_filters;
        result.dir_depth = op.dir_depth;

        if let Some(on_event) = op.on_created {
            result.on_created(on_event);
        }

        if let Some(on_event) = op.on_deleted {
            result.on_deleted(on_event);
        }

        if let Some(on_event) = op.on_changed {
            result.on_changed(on_event);
        }

        if let Some(on_event) = op.on_renamed {
            result.on_renamed(on_event);
        }

        result
    }

    pub fn new(
        dir: &str,
        filter: Option<String>,
        refresh_rate_in_milliseconds: u64,
        dir_depth: Option<u8>,
    ) -> Self {
        let dir_path = PathBuf::from(dir);
        if !dir_path.exists() {
            panic!("The directory '{dir}' does not exist!")
        }

        let result = Self {
            dir_path,
            filter,
            last_sync: None,
            refresh_rate_in_milliseconds,
            main_thread: None,
            events_thread: None,
            on_changed: None,
            on_created: None,
            on_deleted: None,
            on_renamed: None,
            channel_sender: None,
            notify_filters: NotifyFilters::LastWrite,
            dir_depth: dir_depth,
            is_started: Arc::new(Mutex::new(false)),
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

    pub fn on_created(&mut self, action: fn(OnCreatedEventArgs)) -> &Self {
        self.on_created = Some(action);

        self
    }

    pub fn on_changed(&mut self, action: fn(OnChangedEventArgs)) -> &Self {
        self.on_changed = Some(action);

        self
    }

    pub fn on_deleted(&mut self, action: fn(OnDeletedEventArgs)) -> &Self {
        self.on_deleted = Some(action);

        self
    }

    pub fn on_renamed(&mut self, action: fn(OnRenamedEventArgs)) -> &Self {
        self.on_renamed = Some(action);

        self
    }

    pub fn start(&mut self) -> Result<bool, std::io::Error> {
        if *self.is_started.lock().unwrap() {
            return Ok(false);
        }

        // communication channel
        let (sender, receiver) = channel::<ChannelOperation>();
        let sender_mutex = Mutex::new(sender.clone());
        let receiver_mutex = Mutex::new(receiver);
        let filter_mutex = Mutex::new(self.filter.clone());
        let notify_filters_mutex = Arc::new(Mutex::new(self.notify_filters));

        let refresh_rate: u64 = self.refresh_rate_in_milliseconds;
        let on_created = self.on_created;
        let on_deleted = self.on_deleted;
        let on_changed = self.on_changed;
        let on_renamed = self.on_renamed;
        let dir_path = self.dir_path.clone();

        //child thread for receiving changed files
        let child = thread::spawn(move || loop {
            match receiver_mutex.lock().unwrap().recv() {
                Ok(value) => match value {
                    ChannelOperation::CONTINUE(op) => match op {
                        OPERATION::CREATE(data) => {
                            if let Some(func) = on_created {
                                func(OnCreatedEventArgs::new(data.into_iter().collect()));
                            }
                        }
                        OPERATION::CHANGE(data) => {
                            if let Some(func) = on_changed {
                                func(OnChangedEventArgs::new(data.into_iter().collect()));
                            }
                        }
                        OPERATION::DELETE(data) => {
                            if let Some(func) = on_deleted {
                                func(OnDeletedEventArgs::new(data.into_iter().collect()));
                            }
                        }
                        OPERATION::RENAME(data) => {
                            if let Some(func) = on_renamed {
                                func(OnRenamedEventArgs::new(data.into_iter().collect()));
                            }
                        }
                        OPERATION::ERROR(_) => todo!(),
                    },
                    ChannelOperation::EXIT => {
                        break;
                    }
                },
                Err(error) => {
                    println!("{}", error);
                    break;
                }
            }
        });

        let depth = self.dir_depth.clone();

        *self.is_started.lock().unwrap() = true;
        let is_started = self.is_started.clone();

        //main thread for checking for changes in the directory
        let main =
            thread::spawn(move || {
                let mut all_files = HashSet::<File>::new();

                let filter_mutex = filter_mutex.lock().unwrap();
                let mut search_dir =
                    SearchDir::new(dir_path.clone(), depth, (*filter_mutex).clone());

                let notify_filters = Arc::clone(&notify_filters_mutex);

                //load existing files
                for file in Self::get_files(&search_dir, *notify_filters.lock().unwrap()) {
                    all_files.insert(file);
                }

                //check for directory changes
                search_dir.sync_metadata();
                loop {
                    //check if the main thread should stop
                    if !*is_started.lock().unwrap() {
                        //send an exit message for the child thread handling events
                        let _ = sender_mutex.lock().unwrap().send(ChannelOperation::EXIT);
                        break;
                    }

                    //if there's no change in the directory do not get files
                    if !search_dir.has_changed() {
                        thread::sleep(Duration::from_millis(refresh_rate));

                        continue;
                    }

                    let latest_files =
                        Self::get_files(&search_dir, *notify_filters.lock().unwrap());

                    let mut created_files: HashSet<File> = latest_files
                        .difference(&all_files)
                        .map(|fe| fe.clone())
                        .collect();

                    let mut deleted_files: HashSet<File> = all_files
                        .difference(&latest_files)
                        .map(|fe| fe.clone())
                        .collect();

                    let mut changed_files: HashSet<File> = HashSet::new();
                    for file in latest_files.iter() {
                        if let Some(fe) = all_files.get(&file) {
                            // file was changed
                            if fe.last_modified().unwrap() != file.last_modified().unwrap() {
                                changed_files.insert(file.clone());

                                all_files.remove(file);
                                all_files.insert(file.clone());
                            }
                        }
                    }

                    let mut renamed_files: Vec<RenamedFileEntry> = Vec::new();
                    for created in created_files.clone().iter() {
                        for deleted in deleted_files.clone().iter() {
                            if deleted.last_modified() == created.last_modified() {
                                renamed_files
                                    .push(RenamedFileEntry::new(created.name(), deleted.name()));

                                all_files.remove(deleted);
                                all_files.insert(created.clone());
                                break;
                            }
                        }
                    }

                    created_files.retain(|f| !renamed_files.iter().any(|v| v.name().eq(f.name())));
                    deleted_files
                        .retain(|f| !renamed_files.iter().any(|v| v.old_name().eq(f.name())));

                    let local_sender = sender_mutex.lock().unwrap();
                    if created_files.len() > 0 {
                        all_files = all_files.union(&created_files).map(|f| f.clone()).collect();

                        // trigger event for added files
                        let _ = local_sender
                            .clone()
                            .send(ChannelOperation::CONTINUE(OPERATION::CREATE(created_files)));
                    }

                    // trigger event for changed files
                    if changed_files.len() > 0 {
                        let _ = local_sender
                            .clone()
                            .send(ChannelOperation::CONTINUE(OPERATION::CHANGE(changed_files)));
                    }

                    if deleted_files.len() > 0 {
                        for file in deleted_files.iter() {
                            all_files.remove(&file);
                        }

                        // trigger event for added files
                        let _ = local_sender.clone().send(ChannelOperation::CONTINUE(
                            OPERATION::DELETE(deleted_files.clone()),
                        ));
                    };

                    if renamed_files.len() > 0 {
                        // trigger event for added files
                        let _ = local_sender.clone().send(ChannelOperation::CONTINUE(
                            OPERATION::RENAME(renamed_files.into_iter().collect()),
                        ));
                    };

                    drop(local_sender);

                    thread::sleep(Duration::from_millis(refresh_rate));
                }
            });

        self.main_thread = Some(main);
        self.events_thread = Some(child);
        self.channel_sender = Some(sender.clone());

        Ok(true)
    }

    pub fn stop(&mut self) -> Result<bool, Error> {
        if !*self.is_started.lock().unwrap() {
            return Ok(false);
        }

        //set the flag to false to be picked up by the main thread
        *self.is_started.lock().unwrap() = false;

        self.events_thread = None;
        self.main_thread = None;
        self.events_thread = None;

        Ok(true)
    }

    fn get_files(search_dir: &SearchDir, notify_filters: NotifyFilters) -> HashSet<File> {
        let meta: &Metadata = search_dir.metadata();
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
            && old_meta.created().unwrap() != file.created();

        creation_time
    }
}

#[cfg(test)]
mod tests {

    use self::{events::OnChangedEventArgs, FileWatcher};

    use super::*;

    #[test]
    fn it_works() {
        let folder = "D:\\Test";

        let mut fw = FileWatcher::new(folder, Some(String::from("*.txt")), 250, None);
        fw.on_changed(|ev: OnChangedEventArgs| {
            println!("{:?}", ev.files());
        });

        let _ = match fw.start() {
            Ok(started) => started,
            Err(error) => panic!("Could not start the file watcher: {}", error),
        };
    }
}
