use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc, Barrier,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use repowitness_application::resolve_configuration;
use repowitness_domain::MemoryCorrespondenceReviewOperation;
use rusqlite::Connection;

use super::{
    LocalMemoryApprovalRequest, LocalMemoryCorrespondenceReviewRequest,
    LocalMemoryHistoryImportLimits, LocalMemoryHistoryImportRequest, LocalMemoryMaintenance,
    LocalMemoryManageError, LocalMemoryMutation, LocalMemoryWriteRequest, approve_local_memory,
    import_local_memory_history, map_store_error, review_local_memory_correspondence,
    validate_local_memory_actor, write_local_memory,
};
#[cfg(unix)]
use super::{LocalMemoryDatabaseIdentity, LocalMemoryMaintenanceStep};
use crate::{
    ConfigurationFileLayer, LocalIndexRequest, MemoryFormatControl, index_local_repository,
    parse_configuration_file, parse_memory_record,
};

const REPOSITORY_ID: &str =
    "rwi1:h:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const RECORD_ID: &str = "mem_00000000000000000000000000";

#[test]
fn unknown_sqlite_mutations_keep_operation_specific_memory_guidance() {
    let cases = [
        (
            LocalMemoryMutation::StoreStartup,
            "reopen the store and run read-only database diagnostics before retrying startup",
        ),
        (
            LocalMemoryMutation::Approval,
            "reload the exact memory revision, worktree observation, and local approval receipt before retrying",
        ),
        (
            LocalMemoryMutation::HistoryImport,
            "reload the immutable memory journal and compare every intended revision and Git observation before retrying",
        ),
        (
            LocalMemoryMutation::CorrespondenceReview,
            "reload correspondence-review history for the exact revision and evidence ordinal before retrying",
        ),
        (
            LocalMemoryMutation::Checkpoint,
            "retain the known memory receipt and retry only the idempotent checkpoint maintenance step",
        ),
    ];

    for (operation, guidance) in cases {
        let error = map_store_error(crate::SqliteStoreError::MutationOutcomeUnknown, operation);
        assert_eq!(
            error,
            LocalMemoryManageError::MutationOutcomeUnknown { operation }
        );
        assert_eq!(error.reconciliation_guidance(), Some(guidance));
        assert_eq!(
            error.to_string(),
            "memory mutation outcome could not be determined"
        );
    }
}

const MEMORY_YAML: &[u8] = include_bytes!("../../tests/fixtures/memory-v1/commit.yaml");
static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

#[test]
fn denied_memory_policy_stops_every_mutation_before_io() {
    let layer = parse_configuration_file(
        b"schema_version = 1\n[policy]\ndeny_memory_writes = true\n",
        ConfigurationFileLayer::User,
    )
    .expect("configuration should parse");
    let configuration = resolve_configuration(&[layer]).expect("configuration should resolve");
    let ordinal = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
    let missing = std::env::temp_dir().join(format!(
        "repowitness-policy-denied-must-not-be-opened-{}-{ordinal}",
        std::process::id()
    ));
    assert!(!missing.exists());
    let digest = "00".repeat(32);
    let cancelled = Arc::new(AtomicBool::new(false));

    let write = write_local_memory(
        LocalMemoryWriteRequest::from_bytes(&missing, MEMORY_YAML, REPOSITORY_ID)
            .with_configuration(&configuration),
        Arc::clone(&cancelled),
    );
    assert!(matches!(write, Err(LocalMemoryManageError::PolicyDenied)));

    let approval = approve_local_memory(
        LocalMemoryApprovalRequest::new(
            &missing,
            &missing,
            REPOSITORY_ID,
            RECORD_ID,
            "actor",
            1,
            1,
        )
        .with_configuration(&configuration),
        Arc::clone(&cancelled),
    );
    assert!(matches!(
        approval,
        Err(LocalMemoryManageError::PolicyDenied)
    ));

    let review = review_local_memory_correspondence(
        LocalMemoryCorrespondenceReviewRequest::new(
            &missing,
            &missing,
            REPOSITORY_ID,
            RECORD_ID,
            &digest,
            0,
            MemoryCorrespondenceReviewOperation::Approved,
            "src/lib.rs",
            &digest,
            0,
            "actor",
            1,
            1,
        )
        .with_configuration(&configuration),
        Arc::clone(&cancelled),
    );
    assert!(matches!(review, Err(LocalMemoryManageError::PolicyDenied)));

    let history = import_local_memory_history(
        LocalMemoryHistoryImportRequest::new(&missing, &missing, REPOSITORY_ID, "actor", 1, 1)
            .with_configuration(&configuration),
        cancelled,
    );
    assert!(matches!(history, Err(LocalMemoryManageError::PolicyDenied)));
    assert!(!missing.exists());
}

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new(label: &str) -> Self {
        let ordinal = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "repowitness-memory-management-{label}-{}-{ordinal}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("temporary directory should be created");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct GitFixture {
    root: TempDirectory,
}

impl GitFixture {
    fn new() -> Self {
        let root = TempDirectory::new("repository");
        git(root.path(), &["init", "--quiet"]);
        git(root.path(), &["config", "user.name", "RepoWitness Test"]);
        git(
            root.path(),
            &["config", "user.email", "repowitness@example.invalid"],
        );
        fs::write(root.path().join("lib.rs"), b"pub fn publish() {}\n")
            .expect("source should be written");
        git(root.path(), &["add", "lib.rs"]);
        git(root.path(), &["commit", "--quiet", "-m", "source"]);
        Self { root }
    }

    fn path(&self) -> &Path {
        self.root.path()
    }

    fn write_memory(&self, bytes: &[u8]) {
        let records = self.path().join(".code-memory/records");
        fs::create_dir_all(&records).expect("records directory should be created");
        fs::write(records.join(format!("{RECORD_ID}.yaml")), bytes)
            .expect("memory record should be written");
    }

    fn commit_memory(&self, bytes: &[u8], message: &str) {
        self.write_memory(bytes);
        git(self.path(), &["add", ".code-memory/records"]);
        git(self.path(), &["commit", "--quiet", "-m", message]);
    }
}

fn git(root: &Path, arguments: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(arguments)
        .status()
        .expect("Git should start");
    assert!(status.success(), "Git fixture command should succeed");
}

fn indexed_database(repository: &GitFixture, outside: &TempDirectory) -> PathBuf {
    let database = outside.path().join("index.sqlite3");
    index_local_repository(
        LocalIndexRequest::new(repository.path(), &database, REPOSITORY_ID, 123),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("fixture repository should index");
    database
}

#[test]
fn current_record_approval_is_exact_idempotent_and_separate_from_authored_text() {
    let repository = GitFixture::new();
    let outside = TempDirectory::new("approval-database");
    let database = indexed_database(&repository, &outside);
    repository.write_memory(MEMORY_YAML);
    let request = LocalMemoryApprovalRequest::new(
        repository.path(),
        &database,
        REPOSITORY_ID,
        RECORD_ID,
        "trusted-test-actor",
        456,
        1_722_000_000_000,
    );

    let first = approve_local_memory(request, Arc::new(AtomicBool::new(false)))
        .expect("current record should approve");
    assert!(first.version_inserted());
    assert!(first.observation_inserted());
    assert!(first.approval_inserted());
    assert_eq!(first.maintenance(), LocalMemoryMaintenance::Complete);

    let repeated = approve_local_memory(request, Arc::new(AtomicBool::new(false)))
        .expect("exact repeated approval should be idempotent");
    assert_eq!(repeated.revision(), first.revision());
    assert!(!repeated.version_inserted());
    assert!(!repeated.observation_inserted());
    assert!(!repeated.approval_inserted());
    assert_eq!(repeated.maintenance(), LocalMemoryMaintenance::Complete);

    let connection = Connection::open(database).expect("database should open");
    let operations: (i64, i64) = connection
        .query_row(
            "SELECT
                 count(*) FILTER (WHERE operation = 'observed'),
                 count(*) FILTER (WHERE operation = 'locally_approved')
             FROM memory_audit",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("audit operations should be readable");
    assert_eq!(operations, (1, 1));
}

#[cfg(unix)]
#[test]
fn approval_reports_database_replacement_after_the_commit() {
    let repository = GitFixture::new();
    let outside = TempDirectory::new("approval-replacement");
    let database = indexed_database(&repository, &outside);
    let moved = outside.path().join("writer-opened.sqlite3");
    repository.write_memory(MEMORY_YAML);
    let request = LocalMemoryApprovalRequest::new(
        repository.path(),
        &database,
        REPOSITORY_ID,
        RECORD_ID,
        "trusted-test-actor",
        456,
        1_722_000_000_000,
    );

    let receipt =
        super::approval::approve_with_hook(request, Arc::new(AtomicBool::new(false)), || {
            fs::rename(&database, &moved).expect("writer-opened database should move");
            fs::copy(&moved, &database).expect("database path should be replaced");
        })
        .expect("known approval commit should retain its receipt");

    let maintenance = receipt.maintenance();
    assert!(!maintenance.complete());
    assert_eq!(maintenance.warning_count(), 1);
    assert_eq!(
        maintenance.checkpoint(),
        LocalMemoryMaintenanceStep::Complete
    );
    assert_eq!(maintenance.shutdown(), LocalMemoryMaintenanceStep::Complete);
    assert_eq!(
        maintenance.database_identity(),
        LocalMemoryDatabaseIdentity::ChangedAfterCommit
    );
    let approvals: i64 = Connection::open(moved)
        .expect("writer-opened database should remain readable")
        .query_row(
            "SELECT count(*) FROM memory_audit WHERE operation = 'locally_approved'",
            [],
            |row| row.get(0),
        )
        .expect("known approval should remain durable");
    assert_eq!(approvals, 1);
}

mod history;
mod maintenance;

#[test]
fn canonical_write_creates_updates_and_rejects_a_stale_parent() {
    let repository = GitFixture::new();
    let inputs = TempDirectory::new("write-inputs");
    let create_input = inputs.path().join("create.yaml");
    fs::write(&create_input, MEMORY_YAML).expect("create input should be written");
    let create_request =
        LocalMemoryWriteRequest::new(repository.path(), &create_input, REPOSITORY_ID);

    let created = write_local_memory(create_request, Arc::new(AtomicBool::new(false)))
        .expect("new record should publish");
    assert!(created.created());
    assert!(created.canonical_bytes() > 0);
    let target = repository
        .path()
        .join(format!(".code-memory/records/{RECORD_ID}.yaml"));
    assert!(target.is_file());

    let parent = hex(created.revision().as_bytes());
    let update = update_yaml(
        &parent,
        "Readers must observe only a completely staged generation.",
    );
    let update_input = inputs.path().join("update.yaml");
    fs::write(&update_input, update).expect("update input should be written");
    let update_request =
        LocalMemoryWriteRequest::new(repository.path(), &update_input, REPOSITORY_ID);
    let updated = write_local_memory(update_request, Arc::new(AtomicBool::new(false)))
        .expect("matching parent should update");
    assert!(!updated.created());
    assert_ne!(updated.revision(), created.revision());

    assert_eq!(
        write_local_memory(update_request, Arc::new(AtomicBool::new(false)))
            .expect_err("stale parent should not overwrite"),
        LocalMemoryManageError::WriteConflict
    );
    let entries = fs::read_dir(target.parent().expect("target should have a parent"))
        .expect("records directory should be readable")
        .collect::<Result<Vec<_>, _>>()
        .expect("directory entries should be readable");
    assert_eq!(entries.len(), 1);
}

#[test]
fn concurrent_updates_with_one_parent_preserve_one_winner_and_one_conflict() {
    let repository = GitFixture::new();
    let created = write_local_memory(
        LocalMemoryWriteRequest::from_bytes(repository.path(), MEMORY_YAML, REPOSITORY_ID),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("initial record should publish");
    let parent = hex(created.revision().as_bytes());
    let repository = Arc::new(repository.path().to_path_buf());
    let barrier = Arc::new(Barrier::new(3));
    let mut workers = Vec::new();
    for body in [
        "The first concurrent update must not be silently overwritten.",
        "The second concurrent update must not be silently overwritten.",
    ] {
        let repository = Arc::clone(&repository);
        let barrier = Arc::clone(&barrier);
        let bytes = update_yaml(&parent, body).into_bytes();
        workers.push(thread::spawn(move || {
            barrier.wait();
            write_local_memory(
                LocalMemoryWriteRequest::from_bytes(&repository, &bytes, REPOSITORY_ID),
                Arc::new(AtomicBool::new(false)),
            )
        }));
    }
    barrier.wait();

    let mut published = 0_u8;
    let mut conflicted = 0_u8;
    for worker in workers {
        match worker.join().expect("writer thread should not panic") {
            Ok(_) => published += 1,
            Err(LocalMemoryManageError::WriteConflict) => conflicted += 1,
            Err(error) => panic!("unexpected concurrent writer result: {error:?}"),
        }
    }
    assert_eq!((published, conflicted), (1, 1));
    let records = repository.join(".code-memory/records");
    let entries = fs::read_dir(records)
        .expect("records directory should be readable")
        .collect::<Result<Vec<_>, _>>()
        .expect("record entries should be readable");
    assert_eq!(entries.len(), 1);
}

#[test]
fn an_explicit_tombstone_is_written_as_a_new_conflict_checked_version() {
    let repository = GitFixture::new();
    let created = write_local_memory(
        LocalMemoryWriteRequest::from_bytes(repository.path(), MEMORY_YAML, REPOSITORY_ID),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("initial record should publish");
    let parent = hex(created.revision().as_bytes());
    let tombstone = update_yaml(&parent, "This decision is intentionally retired.")
        .replacen("lifecycle: active", "lifecycle: tombstoned", 1)
        .replacen("tombstone: false", "tombstone: true", 1);
    let receipt = write_local_memory(
        LocalMemoryWriteRequest::from_bytes(repository.path(), tombstone.as_bytes(), REPOSITORY_ID),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("the explicit tombstone should publish");
    assert!(!receipt.created());

    let target = repository
        .path()
        .join(format!(".code-memory/records/{RECORD_ID}.yaml"));
    let bytes = fs::read(target).expect("published tombstone should be readable");
    let cancelled = AtomicBool::new(false);
    let parsed = parse_memory_record(
        &bytes,
        MemoryFormatControl::new(&cancelled, Instant::now() + Duration::from_secs(10)),
    )
    .expect("published tombstone should remain canonical");
    assert!(parsed.record().tombstone());
}

#[test]
fn cancellation_and_zero_deadline_leave_shared_memory_absent() {
    let cancelled_repository = GitFixture::new();
    assert_eq!(
        write_local_memory(
            LocalMemoryWriteRequest::from_bytes(
                cancelled_repository.path(),
                MEMORY_YAML,
                REPOSITORY_ID,
            ),
            Arc::new(AtomicBool::new(true)),
        )
        .expect_err("pre-cancelled writes should stop"),
        LocalMemoryManageError::Cancelled
    );
    assert!(!cancelled_repository.path().join(".code-memory").exists());

    let expired_repository = GitFixture::new();
    assert_eq!(
        write_local_memory(
            LocalMemoryWriteRequest::from_bytes(
                expired_repository.path(),
                MEMORY_YAML,
                REPOSITORY_ID,
            )
            .with_deadline(Duration::ZERO),
            Arc::new(AtomicBool::new(false)),
        )
        .expect_err("a zero deadline should fail closed"),
        LocalMemoryManageError::InvalidLimits
    );
    assert!(!expired_repository.path().join(".code-memory").exists());
}

#[cfg(unix)]
#[test]
fn symlink_hardlink_and_special_file_boundaries_do_not_escape_or_overwrite() {
    use std::os::unix::fs::{FileTypeExt, symlink};

    let inputs = TempDirectory::new("hostile-inputs");
    let input = inputs.path().join("record.yaml");
    fs::write(&input, MEMORY_YAML).expect("input fixture should be written");

    let symlink_repository = GitFixture::new();
    let input_symlink = inputs.path().join("record-link.yaml");
    symlink(&input, &input_symlink).expect("input symlink should be created");
    assert_eq!(
        write_local_memory(
            LocalMemoryWriteRequest::new(symlink_repository.path(), &input_symlink, REPOSITORY_ID,),
            Arc::new(AtomicBool::new(false)),
        )
        .expect_err("input symlinks should be rejected"),
        LocalMemoryManageError::InputUnavailable
    );

    let hardlink_repository = GitFixture::new();
    let input_hardlink = inputs.path().join("record-hardlink.yaml");
    fs::hard_link(&input, &input_hardlink).expect("input hard link should be created");
    assert_eq!(
        write_local_memory(
            LocalMemoryWriteRequest::new(
                hardlink_repository.path(),
                &input_hardlink,
                REPOSITORY_ID,
            ),
            Arc::new(AtomicBool::new(false)),
        )
        .expect_err("input hard links should be rejected"),
        LocalMemoryManageError::InputUnavailable
    );

    let escaped_repository = GitFixture::new();
    let escape = TempDirectory::new("symlink-escape");
    symlink(
        escape.path(),
        escaped_repository.path().join(".code-memory"),
    )
    .expect("memory-directory symlink should be created");
    assert_eq!(
        write_local_memory(
            LocalMemoryWriteRequest::from_bytes(
                escaped_repository.path(),
                MEMORY_YAML,
                REPOSITORY_ID,
            ),
            Arc::new(AtomicBool::new(false)),
        )
        .expect_err("a symlinked memory directory should fail"),
        LocalMemoryManageError::FilePublicationFailed
    );
    assert!(
        fs::read_dir(escape.path())
            .expect("escape directory should be readable")
            .next()
            .is_none()
    );

    let special_repository = GitFixture::new();
    let records = special_repository.path().join(".code-memory/records");
    fs::create_dir_all(&records).expect("records directory should be created");
    let fifo = records.join(format!("{RECORD_ID}.yaml"));
    assert!(
        Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .expect("mkfifo should start")
            .success(),
        "FIFO fixture should be created"
    );
    assert!(
        write_local_memory(
            LocalMemoryWriteRequest::from_bytes(
                special_repository.path(),
                MEMORY_YAML,
                REPOSITORY_ID,
            ),
            Arc::new(AtomicBool::new(false)),
        )
        .is_err(),
        "a special-file target must never be replaced"
    );
    assert!(
        fs::symlink_metadata(fifo)
            .expect("FIFO metadata should remain")
            .file_type()
            .is_fifo()
    );
}

#[test]
fn sensitive_input_fails_before_creating_a_shared_memory_directory() {
    let repository = GitFixture::new();
    let inputs = TempDirectory::new("secret-input");
    let secret_input = inputs.path().join("secret.yaml");
    let secret = String::from_utf8(MEMORY_YAML.to_vec()).expect("fixture should be UTF-8");
    let secret = secret.replacen(
        "Readers must never observe a partially staged generation.",
        "api_key = private-value",
        1,
    );
    fs::write(&secret_input, secret).expect("secret input should be written");

    assert_eq!(
        write_local_memory(
            LocalMemoryWriteRequest::new(repository.path(), &secret_input, REPOSITORY_ID),
            Arc::new(AtomicBool::new(false)),
        )
        .expect_err("secret-bearing input should fail"),
        LocalMemoryManageError::SensitiveContent
    );
    assert!(!repository.path().join(".code-memory").exists());
}

#[test]
fn inline_write_and_management_debug_keep_external_inputs_redacted() {
    let repository = GitFixture::new();
    let outside = TempDirectory::new("redacted-requests");
    let database = outside.path().join("private-index.sqlite3");
    let write = LocalMemoryWriteRequest::from_bytes(repository.path(), MEMORY_YAML, REPOSITORY_ID);
    let debug = format!("{write:?}");
    assert!(!debug.contains(repository.path().to_string_lossy().as_ref()));
    assert!(!debug.contains("Readers must never"));
    let receipt = write_local_memory(write, Arc::new(AtomicBool::new(false)))
        .expect("bounded inline input should publish");
    assert!(receipt.created());

    let approval = LocalMemoryApprovalRequest::new(
        repository.path(),
        &database,
        REPOSITORY_ID,
        RECORD_ID,
        "private-actor",
        123,
        456,
    );
    let history = LocalMemoryHistoryImportRequest::new(
        repository.path(),
        &database,
        REPOSITORY_ID,
        "private-actor",
        123,
        456,
    );
    for debug in [format!("{approval:?}"), format!("{history:?}")] {
        assert!(!debug.contains("private"));
        assert!(!debug.contains(REPOSITORY_ID));
        assert!(!debug.contains(RECORD_ID));
    }

    assert!(validate_local_memory_actor("trusted-local-actor").is_ok());
    assert_eq!(
        validate_local_memory_actor(""),
        Err(LocalMemoryManageError::ActorInvalid)
    );
}

fn update_yaml(parent: &str, body: &str) -> String {
    String::from_utf8(MEMORY_YAML.to_vec())
        .expect("fixture should be UTF-8")
        .replacen("display_revision: 1", "display_revision: 2", 1)
        .replacen(
            "parent_revision_digests: []",
            &format!("parent_revision_digests:\n  - \"{parent}\""),
            1,
        )
        .replacen(
            "Readers must never observe a partially staged generation.",
            body,
            1,
        )
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    output
}
