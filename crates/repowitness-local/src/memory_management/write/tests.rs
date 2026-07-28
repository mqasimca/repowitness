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

#[test]
fn contained_directory_sync_uses_a_sync_capable_handle() {
    let fixture = TestDirectory::new("directory-sync");
    let directory = Dir::open_ambient_dir(fixture.path(), ambient_authority())
        .expect("contained directory should open");
    let sync_handle =
        open_directory_sync_handle(&directory).expect("sync-capable directory should open");

    sync_directory(&sync_handle).expect("directory synchronization should succeed");
}

#[test]
fn every_create_publication_stage_has_an_explicit_atomic_failure_outcome() {
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
        assert_eq!(
            write_with_stage_fault(&fixture, MEMORY_YAML, stage),
            Err(LocalMemoryManageError::FilePublicationFailed),
            "fault stage {stage:?} must fail"
        );
        assert_eq!(
            fs::read(fixture.target()).expect("published target should remain readable"),
            expected,
            "fault stage {stage:?} may leave only the complete canonical target"
        );
        assert_no_temporary_files(&fixture);
    }
}

#[test]
fn every_update_publication_stage_preserves_either_old_or_complete_new_bytes() {
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
        assert_eq!(
            write_with_stage_fault(&fixture, update.as_bytes(), stage),
            Err(LocalMemoryManageError::FilePublicationFailed),
            "fault stage {stage:?} must fail"
        );
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
    assert_eq!(
        result,
        Err(LocalMemoryManageError::FilePublicationFailed),
        "an aliased target must never be reported as successfully published"
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
    assert_eq!(
        result,
        Err(LocalMemoryManageError::FilePublicationFailed),
        "a replaced target must never be reported as successfully published"
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
    assert_eq!(
        result,
        Err(LocalMemoryManageError::FilePublicationFailed),
        "mutated canonical bytes must never receive a publication receipt"
    );
    assert_eq!(
        fs::read(target).expect("mutated target should remain readable"),
        hostile
    );
    assert_no_temporary_files(&fixture);
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
        "failed publication must clean its temporary file"
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
