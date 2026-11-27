//! File transactions shared by the tray app and its settings process.
use anyhow::Result;
use std::{fs::{File, OpenOptions}, io::Write, os::fd::AsRawFd, path::Path, sync::atomic::{AtomicU64, Ordering}};

extern "C" { fn flock(fd: i32, operation: i32) -> i32; }
struct Lock(File);
impl Drop for Lock {
    fn drop(&mut self) { unsafe { flock(self.0.as_raw_fd(), 8); } }
}

/// Locks a persistent sidecar, since replacing the data file changes its inode.
pub fn with_lock<T>(path: &Path, body: impl FnOnce() -> Result<T>) -> Result<T> {
    if let Some(dir) = path.parent() { std::fs::create_dir_all(dir)?; }
    let file = OpenOptions::new().create(true).truncate(false).read(true).write(true).open(path.with_extension("lock"))?;
    loop {
        if unsafe { flock(file.as_raw_fd(), 2) } == 0 { break; }
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::Interrupted { return Err(error.into()); }
    }
    let _lock = Lock(file);
    body()
}

/// Writes beside the destination and replaces it only once every byte is on disk.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    if let Some(dir) = path.parent() { std::fs::create_dir_all(dir)?; }
    let (temporary, mut file) = loop {
        let temporary = path.with_extension(format!("{}-{}.tmp", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        match OpenOptions::new().write(true).create_new(true).open(&temporary) {
            Ok(file) => break (temporary, file),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    };
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        std::fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() { let _ = std::fs::remove_file(temporary); }
    result
}
