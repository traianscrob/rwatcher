use std::collections::HashSet;
use std::fmt::Debug;

use crate::search_dir::{File, RenamedFileEntry};

#[derive(Debug, Clone)]
pub struct OnCreatedEventArgs {
    args: BaseEventArgs<File>,
}

impl OnCreatedEventArgs {
    pub fn new(files: HashSet<File>) -> Self {
        Self {
            args: BaseEventArgs { files },
        }
    }

    pub fn files(&self) -> &HashSet<File> {
        &self.args.files
    }
}

#[derive(Debug, Clone)]
struct BaseEventArgs<T: Clone + Debug> {
    files: HashSet<T>,
}

impl<T: Clone + Debug> BaseEventArgs<T> {
    fn new(files: HashSet<T>) -> Self {
        Self { files }
    }

    fn files(&self) -> &HashSet<T> {
        &self.files
    }
}

#[derive(Debug, Clone)]
pub struct OnChangedEventArgs {
    args: BaseEventArgs<File>,
}

impl OnChangedEventArgs {
    pub fn new(files: HashSet<File>) -> Self {
        Self {
            args: BaseEventArgs::new(files),
        }
    }

    pub fn files(&self) -> &HashSet<File> {
        self.args.files()
    }
}

#[derive(Debug, Clone)]
pub struct OnDeletedEventArgs {
    args: BaseEventArgs<File>,
}

impl OnDeletedEventArgs {
    pub fn new(files: HashSet<File>) -> Self {
        Self {
            args: BaseEventArgs::new(files),
        }
    }

    pub fn files(&self) -> &HashSet<File> {
        self.args.files()
    }
}

#[derive(Debug, Clone)]
pub struct OnRenamedEventArgs {
    args: BaseEventArgs<RenamedFileEntry>,
}

impl OnRenamedEventArgs {
    pub fn new(files: HashSet<RenamedFileEntry>) -> Self {
        Self {
            args: BaseEventArgs::new(files),
        }
    }

    pub fn files(&self) -> &HashSet<RenamedFileEntry> {
        self.args.files()
    }
}
