use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};

use super::*;

const REPOSITORY_ID: &str =
    "rwi1:h:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const RECORD_ID: &str = "mem_00000000000000000000000000";
const MEMORY_YAML: &[u8] = include_bytes!("../../../tests/fixtures/memory-v1/commit.yaml");

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(label: &str) -> Self {
        let ordinal = NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "repowitness-memory-publication-{label}-{}-{ordinal}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("test directory should be created");
        assert!(
            Command::new("git")
                .args(["init", "--quiet"])
                .current_dir(&path)
                .status()
                .expect("Git should start")
                .success(),
            "test repository should initialize"
        );
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn target(&self) -> PathBuf {
        self.path()
            .join(format!(".code-memory/records/{RECORD_ID}.yaml"))
    }

    fn records(&self) -> PathBuf {
        self.path().join(".code-memory/records")
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(unix)]
#[test]
fn contained_directory_sync_uses_a_sync_capable_handle() {
    let fixture = TestDirectory::new("directory-sync");
    let directory = Dir::open_ambient_dir(fixture.path(), ambient_authority())
        .expect("contained directory should open");
    let sync_handle =
        open_directory_sync_handle(&directory).expect("sync-capable directory should open");

    sync_directory(&sync_handle).expect("directory synchronization should succeed");
}

#[cfg(not(unix))]
#[test]
fn directory_sync_is_explicitly_deferred_after_publication() {
    let fixture = TestDirectory::new("directory-sync");
    let directory = Dir::open_ambient_dir(fixture.path(), ambient_authority())
        .expect("contained directory should open");
    let sync_handle = open_directory_sync_handle(&directory)
        .expect("deferred directory synchronization is valid");

    assert_eq!(
        sync_directory(&sync_handle),
        Err(LocalMemoryManageError::FilePublicationFailed)
    );
}

#[cfg(not(unix))]
#[test]
fn write_commits_the_record_when_directory_sync_is_deferred() {
    let fixture = TestDirectory::new("directory-sync-deferred");

    let receipt = write(
        LocalMemoryWriteRequest::from_bytes(fixture.path(), MEMORY_YAML, REPOSITORY_ID),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("unsupported directory synchronization must not reject the committed record");

    assert_eq!(
        receipt.publication_status().directory_sync(),
        MemoryFilePublicationStepStatus::Deferred
    );
    assert!(fixture.target().is_file());
}

#[test]
fn create_reports_precommit_failures_and_postcommit_warnings() {
    let before_publication = [
        PublicationStage::OpenDirectorySync,
        PublicationStage::CreateTemporary,
        PublicationStage::WriteTemporary,
        PublicationStage::SyncTemporary,
        PublicationStage::InspectTemporary,
        PublicationStage::PublishTarget,
    ];
    let after_publication = [
        PublicationStage::RemoveTemporary,
        PublicationStage::VerifyTarget,
        PublicationStage::SyncDirectory,
        PublicationStage::VerifyRecordsDirectory,
    ];
    let expected = canonical(MEMORY_YAML);

    for stage in before_publication {
        let fixture = TestDirectory::new("create-before");
        assert_eq!(
            write_with_stage_fault(&fixture, MEMORY_YAML, stage),
            Err(LocalMemoryManageError::FilePublicationFailed),
            "fault stage {stage:?} must fail"
        );
        assert!(
            !fixture.target().exists(),
            "fault stage {stage:?} must not publish a target"
        );
        assert_no_temporary_files(&fixture);
    }

    for stage in after_publication {
        let fixture = TestDirectory::new("create-after");
        let receipt = write_with_stage_fault(&fixture, MEMORY_YAML, stage)
            .expect("the atomic publication already committed");
        let expected_status = match stage {
            PublicationStage::RemoveTemporary => LocalMemoryFilePublicationStatus::new(
                MemoryFilePublicationStepStatus::Deferred,
                MemoryFileIdentityStatus::ChangedAfterCommit,
                MemoryFileIdentityStatus::ConfirmedAtFinalFence,
                MemoryFilePublicationStepStatus::Complete,
            ),
            PublicationStage::VerifyTarget => LocalMemoryFilePublicationStatus::new(
                MemoryFilePublicationStepStatus::Complete,
                MemoryFileIdentityStatus::ChangedAfterCommit,
                MemoryFileIdentityStatus::ConfirmedAtFinalFence,
                MemoryFilePublicationStepStatus::Complete,
            ),
            PublicationStage::SyncDirectory => LocalMemoryFilePublicationStatus::new(
                MemoryFilePublicationStepStatus::Complete,
                MemoryFileIdentityStatus::ConfirmedAtFinalFence,
                MemoryFileIdentityStatus::ConfirmedAtFinalFence,
                MemoryFilePublicationStepStatus::Deferred,
            ),
            PublicationStage::VerifyRecordsDirectory => LocalMemoryFilePublicationStatus::new(
                MemoryFilePublicationStepStatus::Complete,
                MemoryFileIdentityStatus::ConfirmedAtFinalFence,
                MemoryFileIdentityStatus::ChangedAfterCommit,
                MemoryFilePublicationStepStatus::Complete,
            ),
            _ => unreachable!("the post-publication fixture names every stage"),
        };
        assert_eq!(receipt.publication_status(), expected_status);
        assert!(!receipt.publication_status().is_complete());
        assert!(receipt.publication_status().warning_count() > 0);
        assert_eq!(
            fs::read(fixture.target()).expect("published target should remain readable"),
            expected,
            "fault stage {stage:?} may leave only the complete canonical target"
        );
        if stage == PublicationStage::RemoveTemporary {
            assert_eq!(
                temporary_files(&fixture).len(),
                1,
                "deferred cleanup must remain explicit"
            );
        } else {
            assert_no_temporary_files(&fixture);
        }
    }
}

#[test]
fn update_preserves_old_bytes_before_commit_and_reports_warnings_after_commit() {
    let before_publication = [
        PublicationStage::OpenDirectorySync,
        PublicationStage::CreateTemporary,
        PublicationStage::WriteTemporary,
        PublicationStage::SyncTemporary,
        PublicationStage::InspectTemporary,
        PublicationStage::PublishTarget,
    ];
    let after_publication = [
        PublicationStage::VerifyTarget,
        PublicationStage::SyncDirectory,
        PublicationStage::VerifyRecordsDirectory,
    ];

    for stage in before_publication {
        let fixture = TestDirectory::new("update-before");
        let (old, update) = create_update(&fixture);
        assert_eq!(
            write_with_stage_fault(&fixture, update.as_bytes(), stage),
            Err(LocalMemoryManageError::FilePublicationFailed),
            "fault stage {stage:?} must fail"
        );
        assert_eq!(
            fs::read(fixture.target()).expect("old target should remain readable"),
            old,
            "fault stage {stage:?} must preserve the previous target"
        );
        assert_no_temporary_files(&fixture);
    }

    for stage in after_publication {
        let fixture = TestDirectory::new("update-after");
        let (_, update) = create_update(&fixture);
        let expected = canonical(update.as_bytes());
        let receipt = write_with_stage_fault(&fixture, update.as_bytes(), stage)
            .expect("the atomic replacement already committed");
        let expected_status = match stage {
            PublicationStage::VerifyTarget => LocalMemoryFilePublicationStatus::new(
                MemoryFilePublicationStepStatus::NotRequired,
                MemoryFileIdentityStatus::ChangedAfterCommit,
                MemoryFileIdentityStatus::ConfirmedAtFinalFence,
                MemoryFilePublicationStepStatus::Complete,
            ),
            PublicationStage::SyncDirectory => LocalMemoryFilePublicationStatus::new(
                MemoryFilePublicationStepStatus::NotRequired,
                MemoryFileIdentityStatus::ConfirmedAtFinalFence,
                MemoryFileIdentityStatus::ConfirmedAtFinalFence,
                MemoryFilePublicationStepStatus::Deferred,
            ),
            PublicationStage::VerifyRecordsDirectory => LocalMemoryFilePublicationStatus::new(
                MemoryFilePublicationStepStatus::NotRequired,
                MemoryFileIdentityStatus::ConfirmedAtFinalFence,
                MemoryFileIdentityStatus::ChangedAfterCommit,
                MemoryFilePublicationStepStatus::Complete,
            ),
            _ => unreachable!("the post-publication fixture names every stage"),
        };
        assert_eq!(receipt.publication_status(), expected_status);
        assert!(!receipt.publication_status().is_complete());
        assert_eq!(receipt.publication_status().warning_count(), 1);
        assert_eq!(
            fs::read(fixture.target()).expect("new target should remain readable"),
            expected,
            "fault stage {stage:?} may leave only the complete new target"
        );
        assert_no_temporary_files(&fixture);
    }
}

#[cfg(any(unix, windows))]
#[test]
fn a_concurrent_temporary_hard_link_cannot_be_reported_as_published() {
    let fixture = TestDirectory::new("hard-link-race");
    let records = fixture.records();
    let alias = fixture.path().join("hostile-alias.yaml");
    let mut linked = false;
    let mut faults = |stage| {
        if stage == PublicationStage::PublishTarget {
            let temporary = temporary_files(&fixture)
                .into_iter()
                .next()
                .expect("one temporary file should exist");
            fs::hard_link(temporary, &alias).expect("hostile hard link should be created");
            linked = true;
        }
        Ok(())
    };

    let result = write_with_faults(
        LocalMemoryWriteRequest::from_bytes(fixture.path(), MEMORY_YAML, REPOSITORY_ID),
        Arc::new(AtomicBool::new(false)),
        &mut faults,
    );

    assert!(
        linked,
        "the hard-link race must execute, result: {result:?}"
    );
    let receipt = result.expect("the target publication must remain a committed outcome");
    assert_eq!(
        receipt.publication_status(),
        LocalMemoryFilePublicationStatus::new(
            MemoryFilePublicationStepStatus::Complete,
            MemoryFileIdentityStatus::ChangedAfterCommit,
            MemoryFileIdentityStatus::ConfirmedAtFinalFence,
            MemoryFilePublicationStepStatus::Complete,
        ),
        "the alias must be reported as an unconfirmed final target"
    );
    assert_eq!(
        fs::read(&alias).expect("the hostile alias should remain readable"),
        canonical(MEMORY_YAML)
    );
    assert!(records.join(format!("{RECORD_ID}.yaml")).exists());
    assert_no_temporary_files(&fixture);
}

#[test]
fn a_target_replacement_before_verification_cannot_be_reported_as_published() {
    let fixture = TestDirectory::new("target-replacement");
    let target = fixture.target();
    let mut replaced = false;
    let mut faults = |stage| {
        if stage == PublicationStage::VerifyTarget {
            fs::remove_file(&target).expect("published fixture target should be removable");
            fs::write(&target, b"hostile replacement")
                .expect("replacement fixture should be written");
            replaced = true;
        }
        Ok(())
    };

    let result = write_with_faults(
        LocalMemoryWriteRequest::from_bytes(fixture.path(), MEMORY_YAML, REPOSITORY_ID),
        Arc::new(AtomicBool::new(false)),
        &mut faults,
    );

    assert!(replaced, "the target replacement race must execute");
    let receipt = result.expect("the earlier atomic publication must remain visible");
    assert_eq!(
        receipt.publication_status(),
        LocalMemoryFilePublicationStatus::new(
            MemoryFilePublicationStepStatus::Complete,
            MemoryFileIdentityStatus::ChangedAfterCommit,
            MemoryFileIdentityStatus::ConfirmedAtFinalFence,
            MemoryFilePublicationStepStatus::Complete,
        ),
        "the replacement must be reported without claiming rollback"
    );
    assert_eq!(
        fs::read(target).expect("replacement should remain readable"),
        b"hostile replacement"
    );
    assert_no_temporary_files(&fixture);
}

#[cfg(any(unix, windows))]
#[test]
fn same_file_byte_mutation_before_verification_cannot_be_reported_as_published() {
    let fixture = TestDirectory::new("same-file-mutation");
    let target = fixture.target();
    let hostile = vec![b'!'; canonical(MEMORY_YAML).len()];
    let mut mutated = false;
    let mut faults = |stage| {
        if stage == PublicationStage::VerifyTarget {
            let before = FileIdentity::from_file(
                fs::File::open(&target).expect("published fixture target should open"),
            )
            .expect("published fixture identity should be available");
            fs::write(&target, &hostile)
                .expect("published fixture target should be mutable in place");
            let after = FileIdentity::from_file(
                fs::File::open(&target).expect("mutated fixture target should open"),
            )
            .expect("mutated fixture identity should be available");
            assert!(
                before == after,
                "the regression must preserve the published file identity"
            );
            mutated = true;
        }
        Ok(())
    };

    let result = write_with_faults(
        LocalMemoryWriteRequest::from_bytes(fixture.path(), MEMORY_YAML, REPOSITORY_ID),
        Arc::new(AtomicBool::new(false)),
        &mut faults,
    );

    assert!(mutated, "the same-file mutation race must execute");
    let receipt = result.expect("the earlier atomic publication must remain visible");
    assert_eq!(
        receipt.publication_status(),
        LocalMemoryFilePublicationStatus::new(
            MemoryFilePublicationStepStatus::Complete,
            MemoryFileIdentityStatus::ChangedAfterCommit,
            MemoryFileIdentityStatus::ConfirmedAtFinalFence,
            MemoryFilePublicationStepStatus::Complete,
        ),
        "the mutation must be reported without claiming rollback"
    );
    assert_eq!(
        fs::read(target).expect("mutated target should remain readable"),
        hostile
    );
    assert_no_temporary_files(&fixture);
}

#[cfg(unix)]
#[test]
fn records_directory_replacement_before_commit_prevents_publication() {
    let fixture = TestDirectory::new("records-directory-precommit");
    let records = fixture.records();
    let detached = fixture.path().join(".code-memory/records-detached");
    let records_for_hook = records.clone();
    let detached_for_hook = detached.clone();
    let mut replaced = false;
    let mut faults = |stage| {
        if stage == PublicationStage::PublishTarget {
            fs::rename(&records_for_hook, &detached_for_hook)
                .expect("authorized records directory should be detached");
            fs::create_dir(&records_for_hook)
                .expect("replacement records directory should be created");
            replaced = true;
        }
        Ok(())
    };

    let result = write_with_faults(
        LocalMemoryWriteRequest::from_bytes(fixture.path(), MEMORY_YAML, REPOSITORY_ID),
        Arc::new(AtomicBool::new(false)),
        &mut faults,
    );

    assert!(replaced, "the records-directory race must execute");
    assert_eq!(result, Err(LocalMemoryManageError::FilePublicationFailed));
    assert!(!fixture.target().exists());
    assert!(
        !detached.join(format!("{RECORD_ID}.yaml")).exists(),
        "the detached authority must not receive a committed target"
    );
}

#[cfg(unix)]
#[test]
fn records_directory_replacement_after_commit_is_a_categorical_warning() {
    let fixture = TestDirectory::new("records-directory-postcommit");
    let records = fixture.records();
    let detached = fixture.path().join(".code-memory/records-detached");
    let records_for_hook = records.clone();
    let detached_for_hook = detached.clone();
    let mut replaced = false;
    let mut faults = |stage| {
        if stage == PublicationStage::VerifyRecordsDirectory {
            fs::rename(&records_for_hook, &detached_for_hook)
                .expect("authorized records directory should be detached");
            fs::create_dir(&records_for_hook)
                .expect("replacement records directory should be created");
            replaced = true;
        }
        Ok(())
    };

    let receipt = write_with_faults(
        LocalMemoryWriteRequest::from_bytes(fixture.path(), MEMORY_YAML, REPOSITORY_ID),
        Arc::new(AtomicBool::new(false)),
        &mut faults,
    )
    .expect("the atomic publication already committed");

    assert!(replaced, "the records-directory race must execute");
    assert_eq!(
        receipt.publication_status(),
        LocalMemoryFilePublicationStatus::new(
            MemoryFilePublicationStepStatus::Complete,
            MemoryFileIdentityStatus::ConfirmedAtFinalFence,
            MemoryFileIdentityStatus::ChangedAfterCommit,
            MemoryFilePublicationStepStatus::Complete,
        )
    );
    assert!(!fixture.target().exists());
    assert_eq!(
        fs::read(detached.join(format!("{RECORD_ID}.yaml")))
            .expect("committed detached target should remain readable"),
        canonical(MEMORY_YAML)
    );
}

#[test]
fn an_unavailable_cleanup_leaves_only_a_private_noncanonical_temporary() {
    let fixture = TestDirectory::new("cleanup-failure");
    let mut write_failed = false;
    let mut cleanup_failed = false;
    let mut faults = |stage| match stage {
        PublicationStage::WriteTemporary => {
            write_failed = true;
            Err(LocalMemoryManageError::FilePublicationFailed)
        }
        PublicationStage::CleanupTemporary => {
            cleanup_failed = true;
            Err(LocalMemoryManageError::FilePublicationFailed)
        }
        _ => Ok(()),
    };

    let result = write_with_faults(
        LocalMemoryWriteRequest::from_bytes(fixture.path(), MEMORY_YAML, REPOSITORY_ID),
        Arc::new(AtomicBool::new(false)),
        &mut faults,
    );

    assert!(write_failed, "the primary write fault must execute");
    assert!(cleanup_failed, "the cleanup fault must execute");
    assert_eq!(result, Err(LocalMemoryManageError::FilePublicationFailed));
    assert!(!fixture.target().exists());
    let temporary = temporary_files(&fixture);
    assert_eq!(temporary.len(), 1);
    assert_eq!(
        fs::read(&temporary[0]).expect("private temporary should remain readable"),
        b""
    );
}

fn write_with_stage_fault(
    fixture: &TestDirectory,
    input: &[u8],
    fault: PublicationStage,
) -> Result<LocalMemoryWriteReceipt, LocalMemoryManageError> {
    let mut faults = |stage| {
        if stage == fault {
            Err(LocalMemoryManageError::FilePublicationFailed)
        } else {
            Ok(())
        }
    };
    write_with_faults(
        LocalMemoryWriteRequest::from_bytes(fixture.path(), input, REPOSITORY_ID),
        Arc::new(AtomicBool::new(false)),
        &mut faults,
    )
}

fn create_update(fixture: &TestDirectory) -> (Vec<u8>, String) {
    let receipt = write(
        LocalMemoryWriteRequest::from_bytes(fixture.path(), MEMORY_YAML, REPOSITORY_ID),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("initial memory should publish");
    let old = fs::read(fixture.target()).expect("initial target should be readable");
    let parent = hex(receipt.revision().as_bytes());
    let update = String::from_utf8(MEMORY_YAML.to_vec())
        .expect("fixture should be UTF-8")
        .replacen("display_revision: 1", "display_revision: 2", 1)
        .replacen(
            "parent_revision_digests: []",
            &format!("parent_revision_digests:\n  - \"{parent}\""),
            1,
        )
        .replacen(
            "Readers must never observe a partially staged generation.",
            "Readers must observe either the complete old or complete new memory.",
            1,
        );
    (old, update)
}

fn canonical(input: &[u8]) -> Vec<u8> {
    let cancelled = AtomicBool::new(false);
    let deadline = Instant::now() + Duration::from_secs(10);
    let parsed = parse_memory_record(input, MemoryFormatControl::new(&cancelled, deadline))
        .expect("fixture should parse");
    generate_memory_yaml(
        parsed.record(),
        MemoryFormatControl::new(&cancelled, deadline),
    )
    .expect("fixture should canonicalize")
}

fn temporary_files(fixture: &TestDirectory) -> Vec<PathBuf> {
    fs::read_dir(fixture.records())
        .expect("records directory should be readable")
        .map(|entry| entry.expect("directory entry should be readable").path())
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with(".repowitness-write-"))
        })
        .collect()
}

fn assert_no_temporary_files(fixture: &TestDirectory) {
    assert!(
        temporary_files(fixture).is_empty(),
        "publication must not leave an unreported temporary file"
    );
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    output
}
