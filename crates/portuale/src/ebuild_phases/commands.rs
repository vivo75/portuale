//! Running a phase command list, with logging.

use crate::ebuild_phases::*;

pub(crate) struct LogPump {
    pub(crate) thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for LogPump {
    fn drop(&mut self) {
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Drains one pipe's read end into the shared gzip encoder; returns bytes forwarded.
pub(crate) fn pump_one_pipe(
    mut read_end: std::fs::File,
    encoder: &std::sync::Arc<std::sync::Mutex<flate2::write::GzEncoder<std::fs::File>>>,
) -> u64 {
    use std::io::{Read, Write};
    let mut forwarded: u64 = 0;
    let mut buf = [0u8; 65536];
    loop {
        match read_end.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                let Ok(mut enc) = encoder.lock() else {
                    break;
                };
                if enc.write_all(&buf[..n]).is_err() {
                    break;
                }
                forwarded += n as u64;
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    forwarded
}
