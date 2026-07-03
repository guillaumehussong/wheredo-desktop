//! Centralized logging: console + rotating file in the app data dir.
//! Mirrors macOS `UserFeedback`: same file name, same 1 MB rotation.

use std::io::Write;
use std::path::PathBuf;

pub fn log_file_url() -> PathBuf {
    super::config::app_data_dir().join("wheredo.log")
}

pub fn log(message: &str) {
    println!("{message}");
    append_to_file(message);
}

pub fn error(title: &str, message: &str) {
    let line = format!("⚠️  {title}: {message}");
    eprintln!("{line}");
    append_to_file(&line);
}

fn append_to_file(message: &str) {
    let stamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    let line = format!("[{stamp}] {message}\n");
    let url = log_file_url();

    // Keep the log under ~1 MB by keeping the newest 500 KB when exceeded.
    if let Ok(meta) = std::fs::metadata(&url) {
        if meta.len() > 1_000_000 {
            if let Ok(all) = std::fs::read(&url) {
                let tail = &all[all.len().saturating_sub(500_000)..];
                let _ = std::fs::write(&url, tail);
            }
        }
    }

    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&url) {
        let _ = f.write_all(line.as_bytes());
    }
}
