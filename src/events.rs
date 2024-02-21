use std::collections::HashSet;
use std::fmt::Debug;
use std::hash::Hash;

use crate::search_dir::File;

//event args
pub(crate) trait EventArgs<T: Clone>: Sized + Clone + Debug {
    fn files(&self) -> &HashSet<T>;
}

#[derive(Debug, Clone)]
pub struct OnCreatedEventArgs {
    files: HashSet<File>,
}

impl OnCreatedEventArgs {
    pub fn new(files: HashSet<File>) -> Self {
        Self { files: files }
    }

    pub fn files(&self) -> &HashSet<File> {
        &self.files
    }
}

impl EventArgs<File> for OnCreatedEventArgs {
    fn files(&self) -> &HashSet<File> {
        &self.files
    }
}

#[derive(Debug, Clone)]
struct BaseEventArgs<T: Clone> {
    files: HashSet<T>,
}

#[derive(Debug, Clone)]
pub struct OnChangedEventArgs {
    args: BaseEventArgs<File>,
}

impl EventArgs<File> for OnChangedEventArgs {
    fn files(&self) -> &HashSet<File> {
        &self.args.files
    }
}

impl OnChangedEventArgs {
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
pub struct OnDeletedEventArgs {
    args: BaseEventArgs<File>,
}

impl EventArgs<File> for OnDeletedEventArgs {
    fn files(&self) -> &HashSet<File> {
        &self.args.files
    }
}

impl OnDeletedEventArgs {
    pub fn new(files: HashSet<File>) -> Self {
        Self {
            args: BaseEventArgs { files },
        }
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

#[derive(Debug, Clone)]
pub struct OnRenamedEventArgs {
    args: BaseEventArgs<RenamedFileEntry>,
}

impl OnRenamedEventArgs {
    pub fn new(files: HashSet<RenamedFileEntry>) -> Self {
        Self {
            args: BaseEventArgs { files },
        }
    }
}

impl EventArgs<RenamedFileEntry> for OnRenamedEventArgs {
    fn files(&self) -> &HashSet<RenamedFileEntry> {
        &self.args.files
    }
}
