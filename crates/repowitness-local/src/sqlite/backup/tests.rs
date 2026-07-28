use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use super::{BackupLimits, create_online_backup, path_with_suffix, temporary_backup_path};
use crate::sqlite::{SqliteStoreError, open_index_writer};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new() -> Self {
        let ordinal = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "repowitness-backup-{}-{ordinal}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("fixture directory should be created");
        Self(path)
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn preexisting_partial_sidecars_are_never_removed() {
    let directory = TempDirectory::new();
    let source = directory.0.join("source.sqlite3");
    drop(open_index_writer(&source, 123).expect("source database should be created"));
    let sentinel = b"unrelated data";

    for (ordinal, suffix) in ["-journal", "-wal", "-shm"].into_iter().enumerate() {
        let destination = directory.0.join(format!("backup-{ordinal}.sqlite3"));
        let temporary =
            temporary_backup_path(&destination).expect("temporary path should be representable");
        let unrelated_sidecar = path_with_suffix(&temporary, suffix);
        fs::write(&unrelated_sidecar, sentinel).expect("unrelated sidecar should be created");
        let deadline = Instant::now()
            .checked_add(Duration::from_secs(5))
            .expect("test deadline should be representable");

        let error = create_online_backup(
            &source,
            &destination,
            BackupLimits::default(),
            Arc::new(AtomicBool::new(false)),
            deadline,
        )
        .expect_err("an occupied partial-backup namespace must fail closed");

        assert_eq!(error, SqliteStoreError::BackupDestinationUnavailable);
        assert_eq!(
            fs::read(&unrelated_sidecar).expect("unrelated sidecar should remain readable"),
            sentinel
        );
        assert!(!destination.exists());
        assert!(!temporary.exists());
    }
}
