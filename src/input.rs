use std::env;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

pub const DEFAULT_MAX_INPUT_BYTES: usize = 50 * 1024 * 1024;
pub const LIMIT_ERROR: &str = "input exceeds maximum size";

pub fn resolve_max_bytes(cli_override: Option<usize>) -> usize {
    if let Some(n) = cli_override {
        return n;
    }
    if let Ok(raw) = env::var("DREDGE_MAX_INPUT_BYTES") {
        if let Ok(n) = raw.parse::<usize>() {
            return n;
        }
    }
    DEFAULT_MAX_INPUT_BYTES
}

pub fn read_bounded<R: Read>(mut reader: R, max_bytes: usize) -> io::Result<String> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let n = reader.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        if buf.len() + n > max_bytes {
            return Err(io::Error::new(io::ErrorKind::InvalidData, LIMIT_ERROR));
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

pub fn read_file_bounded(path: &Path, max_bytes: usize) -> io::Result<String> {
    read_bounded(File::open(path)?, max_bytes)
}
