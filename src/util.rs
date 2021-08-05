use std::net::UdpSocket;
use std::path::{Path, PathBuf};
use std::env::temp_dir;

use getch::Getch;
use uuid::Uuid;

pub fn find_available_port() -> Result<u16, std::io::Error> {
    Ok(UdpSocket::bind("127.0.0.1:0")?.local_addr()?.port())
}

pub fn temp_file(extension: &str) -> PathBuf {
    let mut dir = temp_dir();
    let file_name = format!("{}.{}", Uuid::new_v4(), extension);

    dir.push(file_name);

    dir
}

pub fn prompt_on_write<P: AsRef<Path>>(path: P) -> bool {
    if path.as_ref().exists() {
        println!("\n{} already exists.\n\nOverwrite [y/n]?", path.as_ref().display());
        let ch = Getch::new().getch().unwrap_or(0) as char;
        ch == 'y' || ch == 'Y'
    } else {
        true
    }
}
