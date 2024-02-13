mod file_watcher;

#[cfg(test)]
mod tests {
    use std::{
        fs::{self, OpenOptions},
        io::{self, Write},
        path::PathBuf,
    };

    use self::file_watcher::FileWatcher;

    use super::*;

    #[test]
    fn it_works() {
        let folder = "D:\\Test";

        let mut fw = FileWatcher::new(folder, Some(&["*.txt", "*.pdf"]), false, 250);
        fw.on_changes(|ev| {
            println!("{:?}", ev.operation());
            println!("{:?}", ev.files());
        });

        let _ = match fw.start() {
            Ok(started) => started,
            Err(error) => panic!("Could not start the file watcher: {}", error),
        };

        let mut path_buf = PathBuf::new();
        path_buf.push(folder);
        path_buf.push("watch.watch");

        let _ = fs::File::create(path_buf.clone()).unwrap();
        if let Ok(mut file) = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path_buf.clone())
        {
            if let Err(error) = file.write(path_buf.as_os_str().as_encoded_bytes()) {
                panic!("{}", error)
            };
        }

        let _ = io::stdin().read_line(&mut String::new());
    }
}
