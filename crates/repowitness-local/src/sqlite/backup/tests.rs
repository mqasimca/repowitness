use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use super::{
    BackupIdentityStatus, BackupLimits, BackupMaintenanceStatus, BackupPublicationStatus,
    BackupStage, create_online_backup, create_online_backup_with_faults, path_with_suffix,
    temporary_backup_path, validate_backup,
};
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

    fn source(&self) -> PathBuf {
        self.0.join("source.sqlite3")
    }

    fn destination(&self, label: &str) -> PathBuf {
        self.0.join(format!("{label}.sqlite3"))
    }

    fn create_source(&self) -> PathBuf {
        let source = self.source();
        drop(open_index_writer(&source, 123).expect("source database should be created"));
        source
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn assert_platform_publication_maintenance(status: BackupPublicationStatus) {
    assert_eq!(
        status.source_identity(),
        BackupIdentityStatus::ConfirmedAtFinalFence
    );
    assert_eq!(
        status.destination_identity(),
        BackupIdentityStatus::ConfirmedAtFinalFence
    );
    assert_eq!(
        status.temporary_cleanup(),
        BackupMaintenanceStatus::Complete
    );

    #[cfg(unix)]
    {
        assert_eq!(status.directory_sync(), BackupMaintenanceStatus::Complete);
        assert!(status.is_complete());
        assert_eq!(status.warning_count(), 0);
    }

    #[cfg(not(unix))]
    {
        assert_eq!(status.directory_sync(), BackupMaintenanceStatus::Deferred);
        assert!(!status.is_complete());
        assert_eq!(status.warning_count(), 1);
    }
}

#[test]
fn preexisting_partial_sidecars_are_never_removed() {
    let directory = TempDirectory::new();
    let source = directory.create_source();
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

#[test]
fn published_backup_reports_platform_appropriate_maintenance() {
    let directory = TempDirectory::new();
    let source = directory.create_source();
    let destination = directory.destination("complete");

    let outcome = create_online_backup(
        &source,
        &destination,
        BackupLimits::default(),
        Arc::new(AtomicBool::new(false)),
        deadline_after(Duration::from_secs(5)),
    )
    .expect("backup should publish");

    assert!(outcome.steps() > 0);
    assert!(outcome.source_pages() > 0);
    assert_platform_publication_maintenance(outcome.publication_status());
    validate_backup(&destination).expect("published destination should validate");
    assert!(
        !temporary_backup_path(&destination)
            .expect("temporary path should be representable")
            .exists()
    );
}

#[test]
fn cleanup_failure_is_a_committed_warning_not_a_rollback_error() {
    let directory = TempDirectory::new();
    let source = directory.create_source();
    let destination = directory.destination("cleanup-warning");
    let faults = |stage| {
        if stage == BackupStage::RemoveTemporary {
            Err(SqliteStoreError::BackupCleanupFailed)
        } else {
            Ok(())
        }
    };

    let outcome = create_online_backup_with_faults(
        &source,
        &destination,
        BackupLimits::default(),
        Arc::new(AtomicBool::new(false)),
        deadline_after(Duration::from_secs(5)),
        Duration::from_millis(50),
        faults,
    )
    .expect("destination publication already committed");

    assert_eq!(
        outcome.publication_status().temporary_cleanup(),
        BackupMaintenanceStatus::Deferred
    );
    assert_eq!(
        outcome.publication_status().destination_identity(),
        BackupIdentityStatus::ChangedAfterCommit,
        "the extra private link prevents exclusive-path confirmation"
    );
    assert!(outcome.publication_status().warning_count() >= 2);
    validate_backup(&destination).expect("published destination should remain valid");
}

#[cfg(any(unix, windows))]
#[test]
fn source_alias_before_publication_fails_without_creating_a_destination() {
    let directory = TempDirectory::new();
    let source = directory.create_source();
    let destination = directory.destination("source-precommit");
    let alias = directory.destination("source-alias");
    let source_for_hook = source.clone();
    let faults = move |stage| {
        if stage == BackupStage::BeforePublish {
            fs::hard_link(&source_for_hook, &alias)
                .expect("source hard-link race should be created");
        }
        Ok(())
    };

    let result = create_online_backup_with_faults(
        &source,
        &destination,
        BackupLimits::default(),
        Arc::new(AtomicBool::new(false)),
        deadline_after(Duration::from_secs(5)),
        Duration::from_millis(50),
        faults,
    );

    assert_eq!(result, Err(SqliteStoreError::DatabaseIdentityChanged));
    assert!(!destination.exists());
}

#[cfg(any(unix, windows))]
#[test]
fn source_alias_after_publication_is_reported_in_the_receipt() {
    let directory = TempDirectory::new();
    let source = directory.create_source();
    let destination = directory.destination("source-postcommit");
    let alias = directory.destination("source-postcommit-alias");
    let source_for_hook = source.clone();
    let faults = move |stage| {
        if stage == BackupStage::VerifySourceIdentity {
            fs::hard_link(&source_for_hook, &alias)
                .expect("source hard-link race should be created");
        }
        Ok(())
    };

    let outcome = create_online_backup_with_faults(
        &source,
        &destination,
        BackupLimits::default(),
        Arc::new(AtomicBool::new(false)),
        deadline_after(Duration::from_secs(5)),
        Duration::from_millis(50),
        faults,
    )
    .expect("destination publication already committed");

    assert_eq!(
        outcome.publication_status().source_identity(),
        BackupIdentityStatus::ChangedAfterCommit
    );
    assert_eq!(
        outcome.publication_status().destination_identity(),
        BackupIdentityStatus::ConfirmedAtFinalFence
    );
    validate_backup(&destination).expect("published destination should remain valid");
}

#[test]
fn destination_replacement_after_publication_is_reported_without_claiming_rollback() {
    let directory = TempDirectory::new();
    let source = directory.create_source();
    let destination = directory.destination("destination-replaced");
    let replacement = b"hostile replacement";
    let destination_for_hook = destination.clone();
    let faults = move |stage| {
        if stage == BackupStage::VerifyDestinationIdentity {
            fs::remove_file(&destination_for_hook)
                .expect("published destination should be removable");
            fs::write(&destination_for_hook, replacement).expect("replacement should be created");
        }
        Ok(())
    };

    let outcome = create_online_backup_with_faults(
        &source,
        &destination,
        BackupLimits::default(),
        Arc::new(AtomicBool::new(false)),
        deadline_after(Duration::from_secs(5)),
        Duration::from_millis(50),
        faults,
    )
    .expect("the earlier no-clobber publication already committed");

    assert_eq!(
        outcome.publication_status().destination_identity(),
        BackupIdentityStatus::ChangedAfterCommit
    );
    assert_eq!(
        fs::read(&destination).expect("replacement should remain readable"),
        replacement
    );
}

#[cfg(unix)]
#[test]
fn destination_directory_replacement_before_commit_prevents_publication() {
    let directory = TempDirectory::new();
    let source = directory.create_source();
    let publication_directory = directory.0.join("publication");
    let detached_directory = directory.0.join("publication-detached");
    fs::create_dir(&publication_directory).expect("publication directory should be created");
    let destination = publication_directory.join("backup.sqlite3");
    let publication_for_hook = publication_directory.clone();
    let detached_for_hook = detached_directory.clone();
    let faults = move |stage| {
        if stage == BackupStage::BeforePublish {
            fs::rename(&publication_for_hook, &detached_for_hook)
                .expect("authorized destination directory should be detached");
            fs::create_dir(&publication_for_hook)
                .expect("replacement destination directory should be created");
        }
        Ok(())
    };

    let result = create_online_backup_with_faults(
        &source,
        &destination,
        BackupLimits::default(),
        Arc::new(AtomicBool::new(false)),
        deadline_after(Duration::from_secs(5)),
        Duration::from_millis(50),
        faults,
    );

    assert_eq!(result, Err(SqliteStoreError::DatabaseIdentityChanged));
    assert!(!destination.exists());
    assert!(!detached_directory.join("backup.sqlite3").exists());
}

#[cfg(unix)]
#[test]
fn destination_directory_replacement_after_commit_is_reported_in_the_receipt() {
    let directory = TempDirectory::new();
    let source = directory.create_source();
    let publication_directory = directory.0.join("publication");
    let detached_directory = directory.0.join("publication-detached");
    fs::create_dir(&publication_directory).expect("publication directory should be created");
    let destination = publication_directory.join("backup.sqlite3");
    let publication_for_hook = publication_directory.clone();
    let detached_for_hook = detached_directory.clone();
    let faults = move |stage| {
        if stage == BackupStage::VerifyDestinationIdentity {
            fs::rename(&publication_for_hook, &detached_for_hook)
                .expect("authorized destination directory should be detached");
            fs::create_dir(&publication_for_hook)
                .expect("replacement destination directory should be created");
        }
        Ok(())
    };

    let outcome = create_online_backup_with_faults(
        &source,
        &destination,
        BackupLimits::default(),
        Arc::new(AtomicBool::new(false)),
        deadline_after(Duration::from_secs(5)),
        Duration::from_millis(50),
        faults,
    )
    .expect("the no-clobber destination publication already committed");

    assert_eq!(
        outcome.publication_status().destination_identity(),
        BackupIdentityStatus::ChangedAfterCommit
    );
    assert_eq!(
        outcome.publication_status().directory_sync(),
        BackupMaintenanceStatus::Deferred
    );
    assert!(!destination.exists());
    validate_backup(&detached_directory.join("backup.sqlite3"))
        .expect("detached committed backup should remain valid");
}

#[test]
fn directory_sync_failure_is_a_categorical_post_commit_warning() {
    let directory = TempDirectory::new();
    let source = directory.create_source();
    let destination = directory.destination("sync-warning");
    let faults = |stage| {
        if stage == BackupStage::SyncDirectory {
            Err(SqliteStoreError::BackupCleanupFailed)
        } else {
            Ok(())
        }
    };

    let outcome = create_online_backup_with_faults(
        &source,
        &destination,
        BackupLimits::default(),
        Arc::new(AtomicBool::new(false)),
        deadline_after(Duration::from_secs(5)),
        Duration::from_millis(50),
        faults,
    )
    .expect("destination publication already committed");

    assert_eq!(
        outcome.publication_status().directory_sync(),
        BackupMaintenanceStatus::Deferred
    );
    assert_eq!(outcome.publication_status().warning_count(), 1);
    validate_backup(&destination).expect("published destination should remain valid");
}

#[test]
fn a_receipt_arriving_during_resolution_grace_preserves_the_exact_outcome() {
    let directory = TempDirectory::new();
    let source = directory.create_source();
    let destination = directory.destination("within-grace");
    let destination_for_worker = destination.clone();
    let deadline = deadline_after(Duration::from_secs(2));
    let grace = Duration::from_millis(250);
    let (published, published_receiver) = mpsc::sync_channel(1);
    let (release, release_receiver) = mpsc::sync_channel(1);
    let faults = move |stage| {
        if stage == BackupStage::DeliverReceipt {
            let _ = published.try_send(());
            release_receiver
                .recv_timeout(Duration::from_secs(3))
                .map_err(|_| SqliteStoreError::BackupFailed)?;
        }
        Ok(())
    };
    let worker = thread::spawn(move || {
        create_online_backup_with_faults(
            &source,
            &destination_for_worker,
            BackupLimits::default(),
            Arc::new(AtomicBool::new(false)),
            deadline,
            grace,
            faults,
        )
    });

    published_receiver
        .recv_timeout(Duration::from_secs(1))
        .expect("backup must reach its committed reply");
    validate_backup(&destination).expect("delivery gate must follow a committed valid backup");
    thread::sleep(
        deadline
            .saturating_duration_since(Instant::now())
            .saturating_add(Duration::from_millis(20)),
    );
    release.send(()).expect("reply should be released in grace");
    let outcome = worker
        .join()
        .expect("caller thread should not panic")
        .expect("the exact committed receipt must win during grace");

    assert_platform_publication_maintenance(outcome.publication_status());
}

#[test]
fn no_receipt_by_the_end_of_grace_returns_outcome_unknown_within_a_bound() {
    let directory = TempDirectory::new();
    let source = directory.create_source();
    let destination = directory.destination("unknown");
    let deadline_duration = Duration::from_millis(150);
    let grace = Duration::from_millis(50);
    let deadline = deadline_after(deadline_duration);
    let (published, published_receiver) = mpsc::sync_channel(1);
    let (release, release_receiver) = mpsc::sync_channel(1);
    let (finished, finished_receiver) = mpsc::sync_channel(1);
    let faults = move |stage| {
        if stage == BackupStage::DeliverReceipt {
            let _ = published.try_send(());
            let _ = release_receiver.recv_timeout(Duration::from_secs(1));
            let _ = finished.try_send(());
        }
        Ok(())
    };
    let started = Instant::now();
    let worker = thread::spawn(move || {
        create_online_backup_with_faults(
            &source,
            &destination,
            BackupLimits::default(),
            Arc::new(AtomicBool::new(false)),
            deadline,
            grace,
            faults,
        )
    });

    published_receiver
        .recv_timeout(Duration::from_secs(1))
        .expect("backup must reach its committed reply");
    let result = worker.join().expect("caller thread should not panic");
    let elapsed = started.elapsed();
    assert_eq!(result, Err(SqliteStoreError::MutationOutcomeUnknown));
    assert!(
        elapsed <= deadline_duration + grace + Duration::from_millis(250),
        "outcome resolution must remain bounded, elapsed: {elapsed:?}"
    );
    release
        .send(())
        .expect("blocked reply hook should still be waiting");
    finished_receiver
        .recv_timeout(Duration::from_secs(1))
        .expect("detached backup worker should finish after release");
}

fn deadline_after(duration: Duration) -> Instant {
    Instant::now()
        .checked_add(duration)
        .expect("test deadline should be representable")
}
