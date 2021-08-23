use std::net::UdpSocket;
use std::io;
use std::iter;
use std::path::{Path, PathBuf};
use std::env::temp_dir;

use rand::{Rng, thread_rng};
use rand::distributions::Alphanumeric;
use getch::Getch;
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct ForwardingInfo {
    pub local_port: u16,
    pub remote_host: String,
    pub remote_port: u16
}

pub fn find_available_port() -> Result<u16, std::io::Error> {
    Ok(UdpSocket::bind("127.0.0.1:0")?.local_addr()?.port())
}

/// Parses a string of the "<local-port>:<remote-host>:<remote-port>" or
/// "<remote-host>:<remote-port>"
pub fn parse_forwarding_string(host: &str) -> Result<ForwardingInfo, io::Error> {
    let parts = host.split(":").collect::<Vec<&str>>();
    if parts.len() == 3 {
        let local_port = parts[0].parse::<u16>()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        let remote_host = parts[1];
        let remote_port = parts[2].parse::<u16>()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        Ok(ForwardingInfo {
            local_port,
            remote_host: remote_host.to_owned(),
            remote_port
        })
    } else if parts.len() == 2 {
        let remote_host = parts[0];
        let remote_port = parts[1].parse::<u16>()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        Ok(ForwardingInfo {
            local_port: find_available_port()?, // pick a random port
            remote_host: remote_host.to_owned(),
            remote_port
        })
    } else {
        Err(io::Error::new(io::ErrorKind::InvalidInput, "expected \
                           \"<local-port>:<remote-host>:<remote-port>\" \
                           or \"<remote-host>:<remote-port>\""))
    }
}

pub fn temp_file(extension: &str) -> PathBuf {
    let mut dir = temp_dir();
    let file_name = format!("{}.{}", Uuid::new_v4(), extension);
    dir.push(file_name);
    dir
}

/// From https://docs.rs/rand/0.8.4/rand/distributions/struct.Alphanumeric.html
pub fn random_alphanum(len: usize) -> String {
    let mut rng = thread_rng();
    iter::repeat(())
        .map(|()| rng.sample(Alphanumeric))
        .map(char::from)
        .take(len)
        .collect::<String>()
        .to_lowercase()
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
