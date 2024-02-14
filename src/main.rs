use file_watcher::{FileWatcher, FileWatcherOptions, NotifyFilters};
use std::io;

mod file_watcher;

fn main() -> std::io::Result<()> {
    let mut op = FileWatcherOptions::new("D:\\Test");
    op.with_extensions(&["*.txt", "*.pdf", "*.rs", "*.jpg", "*.jpeg", "*.*"])
        .with_refresh_rate(250)
        .with_notify_filters(NotifyFilters::CreationTime | NotifyFilters::LastWrite)
        .with_on_changes(|ev| {
            let files = ev.files();
            println!("{:?} -> {}", ev.operation(), files.len());

            for f in files {
                println!("-> {:?}", f);
            }

            println!();
        });

    let mut fw = FileWatcher::new_with_options(&op);

    let _ = match fw.start() {
        Ok(started) => {
            println!("[INFO] Watching folder: {}", fw.watched_dir());
            println!("[INFO] File extensions watched: {:?}", fw.extensions());
            println!("[INFO] Press any key to exit!");

            started
        }
        Err(error) => panic!("Could not start the file watcher: {}", error),
    };

    let _ = io::stdin().read_line(&mut String::new());

    match fw.stop() {
        Ok(stopped) => match stopped {
            true => println!("File watcher has stopped!"),
            _ => println!("File watcher is already stopped!"),
        },
        Err(err) => println!("Error while attempting to stop the file watcher: {:?}", err),
    }

    drop(fw);

    Ok(())
}
