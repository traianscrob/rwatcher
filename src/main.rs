use rwatcher::{FileWatcher, FileWatcherOptions, NotifyFilters};
use std::io;

mod events;
mod search_dir;

fn main() -> std::io::Result<()> {
    let mut op = FileWatcherOptions::new("D:\\Test");
    op.with_filter("*.txt;*.pdf;*.sql;*.jpg")
        .with_refresh_rate(250)
        .with_notify_filters(NotifyFilters::CreationTime | NotifyFilters::LastWrite)
        .with_directory_depth(4)
        .with_on_created(|ev| {
            let files = ev.files();
            println!("{} -> {}", "CREATED", files.len());

            for f in files {
                println!("-> {}", f.name());
            }

            println!();
        })
        .with_on_deleted(|ev| {
            let files = ev.files();
            println!("{:?} -> {}", "DELETED", files.len());

            for f in files {
                println!("-> {}", f.name());
            }

            println!();
        })
        .with_on_changed(|ev| {
            let files = ev.files();
            println!("{} -> {}", "CHANGED", files.len());

            for f in files {
                println!("-> {}", f.name());
            }

            println!();
        })
        .with_on_renamed(|ev| {
            let files = ev.files();
            println!("{} -> {}", "RENAMED", files.len());

            for f in files {
                println!("-> {} -> {}", f.old_name(), f.name());
            }

            println!();
        });

    let mut fw = FileWatcher::new_with_options(&op);
    let _ = match fw.start() {
        Ok(started) => {
            println!("[INFO] Watching folder: {}", fw.watched_dir());
            println!("[INFO] File extensions watched: {:?}", fw.filter());
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
