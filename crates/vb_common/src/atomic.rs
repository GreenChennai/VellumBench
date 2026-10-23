//! Same-directory atomic file replacement shared by project exporters.

use std::fs::{self, OpenOptions};
use std::io::{self, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);
const TEMP_NAME_ATTEMPTS: usize = 128;

/// Write bytes to a unique temporary sibling, flush it, then atomically replace `path`.
///
/// The temporary file uses `create_new` and a process-local counter, so concurrent
/// exports cannot truncate each other's partial output. Any failed write or rename
/// removes its temporary file and leaves the previous destination untouched.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));

    for _ in 0..TEMP_NAME_ATTEMPTS {
        let temp_id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let temp_path: PathBuf = parent.join(format!(".vb-tmp-{}-{temp_id}", std::process::id()));
        let mut file = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        };

        let write_result = file.write_all(bytes).and_then(|()| file.sync_all());
        drop(file);
        if let Err(error) = write_result {
            let _ = fs::remove_file(&temp_path);
            return Err(error);
        }
        if let Err(error) = fs::rename(&temp_path, path) {
            let _ = fs::remove_file(&temp_path);
            return Err(error);
        }
        return Ok(());
    }

    Err(io::Error::new(
        ErrorKind::AlreadyExists,
        "could not allocate a unique atomic-write temporary file",
    ))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::write_atomic;

    static NEXT_TEST_ID: AtomicU64 = AtomicU64::new(0);

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let id = NEXT_TEST_ID.fetch_add(1, Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("vellumb-atomic-{}-{id}", std::process::id()));
            fs::create_dir_all(&path).expect("create atomic-write test directory");
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn atomically_creates_and_replaces_target() {
        let directory = TempDir::new();
        let target = directory.0.join("export.svg");
        write_atomic(&target, b"old-complete").expect("create output");
        write_atomic(&target, b"new-complete").expect("replace output");
        assert_eq!(
            fs::read(target).expect("read final target"),
            b"new-complete"
        );
        assert_eq!(
            fs::read_dir(&directory.0)
                .expect("list output directory")
                .count(),
            1,
            "temporary siblings are removed after replacement"
        );
    }

    #[test]
    fn failed_write_does_not_create_output_or_temporary_sibling() {
        let directory = TempDir::new();
        let target = directory.0.join("missing").join("export.png");
        assert!(write_atomic(&target, b"image").is_err());
        assert!(!target.exists());
        assert_eq!(
            fs::read_dir(&directory.0)
                .expect("list output directory")
                .count(),
            0
        );
    }
}
