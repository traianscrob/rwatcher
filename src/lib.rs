mod file_watcher;
mod search_dir;

#[cfg(test)]
mod tests {

    use crate::file_watcher::EventArgs;

    use self::file_watcher::FileWatcher;

    use super::*;

    #[test]
    fn it_works() {
        let folder = "D:\\Test";

        let mut fw = FileWatcher::new(folder, Some("*.txt"), 250, None);
        fw.on_changed(|ev| {
            println!("{:?}", ev.operation());
            println!("{:?}", ev.files());
        });

        let _ = match fw.start() {
            Ok(started) => started,
            Err(error) => panic!("Could not start the file watcher: {}", error),
        };
    }
}
