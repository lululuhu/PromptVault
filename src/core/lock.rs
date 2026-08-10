//! Advisory repository lock — prevents concurrent writes from corrupting
//! shared state (index, refs, HEAD).
//!
//! Uses an exclusive lock file (`.pv/index.lock`) created atomically with
//! `create_new`. On Unix we additionally take an `flock` on it for robustness
//! against stale lock files left by crashed processes: if the creating process
//! is dead, the kernel releases the flock automatically.
//!
//! The guard is RAII: dropping it releases the lock.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Result};

/// An acquired repository lock. Release by dropping.
pub struct FileLock {
    path: PathBuf,
    #[cfg(unix)]
    _file: File,
}

impl FileLock {
    /// Acquire an exclusive lock on `<pv_dir>/index.lock`.
    ///
    /// Returns `Err` if another process holds the lock. Stale locks (whose
    /// holder has crashed) are detected and reclaimed on Unix via `flock`.
    pub fn acquire(pv_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(pv_dir)?;
        let path = pv_dir.join("index.lock");

        #[cfg(unix)]
        {
            return Self::acquire_unix(&path);
        }

        #[cfg(not(unix))]
        {
            Self::acquire_portable(&path)
        }
    }

    #[cfg(unix)]
    fn acquire_unix(path: &Path) -> Result<Self> {
        use std::os::unix::io::AsRawFd;

        // Open (creating if needed) then take an exclusive flock.
        // flock is automatically released by the kernel when the process exits,
        // so a crash never leaves a stale lock.
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path)?;

        // Try a non-blocking exclusive lock.
        let fd = file.as_raw_fd();
        // LOCK_EX | LOCK_NB = 2 | 4 = 6
        let rc = unsafe { libc::flock(fd, 6) };
        if rc != 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::WouldBlock {
                bail!(
                    "vault is locked by another process; remove {} if stale",
                    path.display()
                );
            }
            bail!("failed to acquire lock: {err}");
        }

        // Write holder info for diagnostics (non-fatal if it fails).
        let _ = writeln!(
            &mut &file,
            "{}",
            lock_content()
        );

        Ok(FileLock {
            path: path.to_path_buf(),
            _file: file,
        })
    }

    #[cfg(not(unix))]
    fn acquire_portable(path: &Path) -> Result<Self> {
        // Fallback: atomically create the lock file. If it exists we fail.
        // Stale locks must be removed manually on non-Unix platforms.
        let file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(path)
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::AlreadyExists {
                    anyhow::anyhow!(
                        "vault is locked by another process; remove {} if stale",
                        path.display()
                    )
                } else {
                    anyhow::anyhow!("failed to acquire lock: {e}")
                }
            })?;

        let _ = writeln!(&mut &file, "{}", lock_content());

        Ok(FileLock {
            path: path.to_path_buf(),
        })
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        // Remove the lock file. On Unix the flock is released when the file
        // handle drops; removing the file keeps things tidy.
        let _ = std::fs::remove_file(&self.path);
    }
}

fn lock_content() -> String {
    let pid = std::process::id();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("pid={pid} acquired={now}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acquire_and_release() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_path_buf();

        {
            let _lock = FileLock::acquire(&path).unwrap();
            // Lock file exists while held.
            assert!(path.join("index.lock").exists());
        }
        // After drop, the lock file is removed.
        assert!(!path.join("index.lock").exists());
    }

    #[test]
    fn double_acquire_fails() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_path_buf();

        let _lock1 = FileLock::acquire(&path).unwrap();

        // A second lock on the same path must fail (or at least not panic).
        // On Unix with flock, the second acquire should fail with WouldBlock.
        #[cfg(unix)]
        {
            let result = FileLock::acquire(&path);
            assert!(result.is_err(), "second acquire should fail while held");
        }
        // Drop lock1 to release.
        drop(_lock1);

        // After release, we can acquire again.
        let _lock2 = FileLock::acquire(&path).unwrap();
    }
}
