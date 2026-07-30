use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use repowitness_application::RepositoryIdentityTextV1;
use repowitness_application::{
    MemoryEffectiveState, MemoryImportApproval, MemoryRecallError, MemoryRecallLimits,
    MemoryRecallQuery, MemoryRecallRequest, memory_recall,
};
use repowitness_domain::{
    AnalysisArtifactDigest, ByteOffset, ByteSpan, DeclarationDigest, MemoryActorId,
    MemoryActorKind, MemoryAssurance, MemoryAuditActorId, MemoryBody, MemoryClaim, MemoryCommitId,
    MemoryDisplayRevision, MemoryEvidence, MemoryEvidenceIndex, MemoryFactOrdinal, MemoryKind,
    MemoryLifecycle, MemoryObservationSource, MemoryPresentationDigest, MemoryProducerId,
    MemoryProducerVersion, MemoryProvenance, MemoryProvenanceOrigin, MemoryQualifiedName,
    MemoryRecord, MemoryRecordHeader, MemoryRecordId, MemoryRecordedAtUnixMillis, MemoryScope,
    MemorySymbolName, MemoryTitle, MemoryValidity, ProducerIdentity, RepositoryIdentityDigest,
    RepositoryPath, RepositoryPathLimits, RustMemorySymbolKind, RustSymbolMemoryEvidence,
    SourceContentDigest, SourceSnapshotDigest,
};
use rusqlite::Connection;
use sha2::{Digest, Sha256};

use super::{
    LocalMemoryRevalidationError, LocalMemoryRevalidationLimits, LocalMemoryRevalidationMutation,
    LocalMemoryRevalidationRequest, map_revalidation_mutation_error, map_store_startup_error,
    revalidate_local_memory,
};
use crate::{
    LocalIndexRequest, MemoryFormatControl, OwnedSqliteIndex, OwnedSqliteReader, SqliteStoreError,
    index_local_repository, parse_memory_record,
};
#[cfg(unix)]
use crate::{LocalMemoryDatabaseIdentity, LocalMemoryMaintenanceStep};

const COMMIT_MEMORY_YAML: &[u8] = include_bytes!("../../tests/fixtures/memory-v1/commit.yaml");
static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct TempDirectory {
    path: PathBuf,
}

impl TempDirectory {
    fn new() -> Self {
        let ordinal = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "repowitness-memory-revalidation-{}-{ordinal}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("fixture directory should be created");
        Self { path }
    }

    fn repository(&self) -> PathBuf {
        self.path.join("repository")
    }

    fn database(&self) -> PathBuf {
        self.path.join("index.sqlite3")
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(10)
}

fn git(repository: &Path, arguments: &[&str]) {
    let status = Command::new("git")
        .args(["-c", "core.hooksPath=/dev/null"])
        .args(arguments)
        .current_dir(repository)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .status()
        .expect("Git fixture command should start");
    assert!(status.success(), "Git fixture command should succeed");
}

fn current_git_commit(repository: &Path) -> MemoryCommitId {
    let output = Command::new("git")
        .args(["-c", "core.hooksPath=/dev/null", "rev-parse", "HEAD"])
        .current_dir(repository)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .expect("Git fixture command should start");
    assert!(
        output.status.success(),
        "Git fixture command should succeed"
    );
    let object_id = std::str::from_utf8(&output.stdout)
        .expect("fixture object ID should be UTF-8")
        .trim();
    assert_eq!(object_id.len(), 40, "fixture repository should use SHA-1");
    let mut bytes = [0_u8; 20];
    for (index, pair) in object_id.as_bytes().chunks_exact(2).enumerate() {
        let pair = std::str::from_utf8(pair).expect("fixture object ID should be ASCII");
        bytes[index] =
            u8::from_str_radix(pair, 16).expect("fixture object ID should be hexadecimal");
    }
    MemoryCommitId::Sha1(bytes)
}

fn initialize_repository(repository: &Path) {
    fs::create_dir(repository).expect("repository directory should be created");
    git(repository, &["init", "--quiet", "--object-format=sha1"]);
    git(repository, &["config", "user.name", "RepoWitness Test"]);
    git(
        repository,
        &["config", "user.email", "repowitness@example.invalid"],
    );
    fs::create_dir(repository.join("src")).expect("source directory should be created");
    fs::write(
        repository.join("src/lib.rs"),
        b"pub fn current() -> bool { true }\n",
    )
    .expect("source fixture should be written");
    git(repository, &["add", "src/lib.rs"]);
    git(repository, &["commit", "--quiet", "-m", "fixture"]);
}

fn import_commit_memory(database: &Path) {
    let cancelled = Arc::new(AtomicBool::new(false));
    let parsed = parse_memory_record(
        COMMIT_MEMORY_YAML,
        MemoryFormatControl::new(cancelled.as_ref(), deadline()),
    )
    .expect("memory fixture should parse");
    let record = parsed.into_record();
    let repository = record.scope().repository();
    let presentation = MemoryPresentationDigest::new(Sha256::digest(COMMIT_MEMORY_YAML).into());
    let (store, _) =
        OwnedSqliteIndex::start(database, 123, deadline()).expect("store should reopen");
    store
        .import_memory_version(
            repository,
            record,
            presentation,
            MemoryObservationSource::Git(MemoryCommitId::Sha1([0x11; 20])),
            MemoryAuditActorId::try_new("trusted-test-actor".to_owned())
                .expect("audit actor should be valid"),
            MemoryRecordedAtUnixMillis::try_new(1_722_000_000_000)
                .expect("timestamp should be valid"),
            MemoryImportApproval::LocallyApproved,
            Arc::clone(&cancelled),
            deadline(),
        )
        .expect("memory fixture should import");
    store.shutdown(deadline()).expect("store should shut down");
}

struct ActiveOccurrence {
    snapshot: Vec<u8>,
    path: Vec<u8>,
    content: Vec<u8>,
    artifact: Vec<u8>,
    fact_ordinal: i64,
    kind: String,
    name: String,
    qualified_name: String,
    name_start: i64,
    name_end: i64,
    declaration_start: i64,
    declaration_end: i64,
    declaration: Vec<u8>,
}

fn active_occurrence(database: &Path, repository: RepositoryIdentityDigest) -> ActiveOccurrence {
    active_occurrence_at(database, repository, 0)
}

fn active_occurrence_at(
    database: &Path,
    repository: RepositoryIdentityDigest,
    occurrence_offset: u32,
) -> ActiveOccurrence {
    let connection = Connection::open(database).expect("database should open");
    connection
        .query_row(
            "SELECT generation.snapshot_digest, file.repository_path,
                        file.content_digest, file.artifact_digest, fact.ordinal,
                        fact.kind, fact.name, fact.qualified_name,
                        fact.name_start, fact.name_end,
                        fact.declaration_start, fact.declaration_end,
                        correspondence.declaration_digest
                 FROM workspaces AS workspace
                 JOIN index_generations AS generation
                   ON generation.generation_id = workspace.active_generation_id
                 JOIN generation_files AS file
                   ON file.generation_id = generation.generation_id
                 JOIN artifact_facts AS fact
                   ON fact.artifact_digest = file.artifact_digest
                 JOIN artifact_fact_correspondence AS correspondence
                   ON correspondence.artifact_digest = fact.artifact_digest
                  AND correspondence.fact_ordinal = fact.ordinal
                  AND correspondence.profile_id = 'rust-name-elided'
                  AND correspondence.profile_version = 1
                 WHERE workspace.repository_identity = ?1
                   AND generation.lifecycle_state = 'active'
                   AND fact.kind = 'function'
                 ORDER BY file.repository_path, fact.ordinal
                 LIMIT 1 OFFSET ?2",
            rusqlite::params![
                repository.as_bytes().as_slice(),
                i64::from(occurrence_offset)
            ],
            |row| {
                Ok(ActiveOccurrence {
                    snapshot: row.get(0)?,
                    path: row.get(1)?,
                    content: row.get(2)?,
                    artifact: row.get(3)?,
                    fact_ordinal: row.get(4)?,
                    kind: row.get(5)?,
                    name: row.get(6)?,
                    qualified_name: row.get(7)?,
                    name_start: row.get(8)?,
                    name_end: row.get(9)?,
                    declaration_start: row.get(10)?,
                    declaration_end: row.get(11)?,
                    declaration: row.get(12)?,
                })
            },
        )
        .expect("one indexed function occurrence should exist")
}

fn import_exact_memory(database: &Path, repository: RepositoryIdentityDigest) {
    import_repeated_exact_memory(database, repository, 1);
}

fn import_repeated_exact_memory(
    database: &Path,
    repository: RepositoryIdentityDigest,
    evidence_count: usize,
) {
    import_repeated_exact_memory_with_introduction(database, repository, evidence_count, None);
}

fn import_exact_commit_memory(
    database: &Path,
    repository: RepositoryIdentityDigest,
    introduction: MemoryCommitId,
) {
    import_repeated_exact_memory_with_introduction(database, repository, 1, Some(introduction));
}

fn import_repeated_exact_memory_with_introduction(
    database: &Path,
    repository: RepositoryIdentityDigest,
    evidence_count: usize,
    introduction: Option<MemoryCommitId>,
) {
    let (snapshot, evidence) = exact_memory_evidence(database, repository);
    let (validity, observation_source) = match introduction {
        Some(commit) => (
            MemoryValidity::try_commits(vec![commit], Vec::new())
                .expect("commit validity should be valid"),
            MemoryObservationSource::Git(commit),
        ),
        None => (
            MemoryValidity::worktree(snapshot),
            MemoryObservationSource::Worktree(snapshot),
        ),
    };
    let record = MemoryRecord::try_new(
        MemoryRecordHeader::try_new(
            MemoryRecordId::new([0x91; 16]),
            MemoryDisplayRevision::try_new(1).expect("display revision should be valid"),
            Vec::new(),
        )
        .expect("record header should be valid"),
        MemoryClaim::new(
            MemoryKind::Decision,
            MemoryTitle::try_new("Keep exact evidence current".to_owned())
                .expect("title should be valid"),
            MemoryBody::try_new("The exact indexed declaration remains supported.".to_owned())
                .expect("body should be valid"),
        ),
        MemoryScope::new(
            repository,
            MemoryEvidenceIndex::try_new(0).expect("evidence index should be valid"),
        ),
        MemoryProvenance::new(
            MemoryProvenanceOrigin::Human,
            MemoryActorKind::LocalAsserted,
            MemoryActorId::try_new("maintainer".to_owned()).expect("actor should be valid"),
        ),
        MemoryAssurance::LocallyApproved,
        MemoryLifecycle::Active,
        validity,
        (0..evidence_count)
            .map(|_| MemoryEvidence::RustSymbol(evidence.clone()))
            .collect(),
        Vec::new(),
        false,
    )
    .expect("exact memory record should be valid");
    let cancelled = Arc::new(AtomicBool::new(false));
    let (store, _) =
        OwnedSqliteIndex::start(database, 123, deadline()).expect("store should reopen");
    store
        .import_memory_version(
            repository,
            record,
            MemoryPresentationDigest::new([0x92; 32]),
            observation_source,
            MemoryAuditActorId::try_new("trusted-test-actor".to_owned())
                .expect("audit actor should be valid"),
            MemoryRecordedAtUnixMillis::try_new(1_722_000_000_001)
                .expect("timestamp should be valid"),
            MemoryImportApproval::LocallyApproved,
            Arc::clone(&cancelled),
            deadline(),
        )
        .expect("exact memory should import");
    store.shutdown(deadline()).expect("store should shut down");
}

fn exact_memory_evidence(
    database: &Path,
    repository: RepositoryIdentityDigest,
) -> (SourceSnapshotDigest, RustSymbolMemoryEvidence) {
    let occurrence = active_occurrence(database, repository);
    assert_eq!(occurrence.kind, "function");
    let snapshot = SourceSnapshotDigest::try_from_slice(&occurrence.snapshot)
        .expect("snapshot digest should be valid");
    let name_start =
        u64::try_from(occurrence.name_start).expect("name start should be nonnegative");
    let name_end = u64::try_from(occurrence.name_end).expect("name end should be nonnegative");
    let declaration_start = u64::try_from(occurrence.declaration_start)
        .expect("declaration start should be nonnegative");
    let declaration_end =
        u64::try_from(occurrence.declaration_end).expect("declaration end should be nonnegative");
    let evidence = RustSymbolMemoryEvidence::try_new(
        snapshot,
        RepositoryPath::try_from_vec(
            occurrence.path,
            RepositoryPathLimits::new(1_048_576, 65_535),
        )
        .expect("persisted path should be valid"),
        SourceContentDigest::try_from_slice(&occurrence.content)
            .expect("content digest should be valid"),
        AnalysisArtifactDigest::try_from_slice(&occurrence.artifact)
            .expect("artifact digest should be valid"),
        MemoryFactOrdinal::try_new(
            u64::try_from(occurrence.fact_ordinal).expect("fact ordinal should be nonnegative"),
        )
        .expect("fact ordinal should be valid"),
        RustMemorySymbolKind::Function,
        MemorySymbolName::try_new(occurrence.name).expect("symbol name should be valid"),
        MemoryQualifiedName::try_new(occurrence.qualified_name)
            .expect("qualified name should be valid"),
        ByteSpan::try_new(ByteOffset::new(name_start), ByteOffset::new(name_end))
            .expect("name span should be valid"),
        ByteSpan::try_new(
            ByteOffset::new(declaration_start),
            ByteOffset::new(declaration_end),
        )
        .expect("declaration span should be valid"),
        DeclarationDigest::try_from_slice(&occurrence.declaration)
            .expect("declaration digest should be valid"),
        ProducerIdentity::new(
            MemoryProducerId::try_new("repowitness.rust.syntax".to_owned())
                .expect("producer ID should be valid"),
            MemoryProducerVersion::try_new("phase0-rust-syntax-v1".to_owned())
                .expect("producer version should be valid"),
        ),
    )
    .expect("exact evidence should be valid");
    (snapshot, evidence)
}

#[test]
fn missing_git_object_history_is_indeterminate_and_never_auto_links() {
    let fixture = TempDirectory::new();
    let repository = fixture.repository();
    let database = fixture.database();
    initialize_repository(&repository);
    let identity = RepositoryIdentityTextV1::encode(RepositoryIdentityDigest::new([0xAA_u8; 32]));
    let index = index_local_repository(
        LocalIndexRequest::new(&repository, &database, identity.as_str(), 123),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("source index should activate");
    import_commit_memory(&database);

    let report = revalidate_local_memory(
        LocalMemoryRevalidationRequest::new(&repository, &database, identity.as_str(), 123),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("memory projection should activate");

    assert!(report.projection_id() > 0);
    assert_eq!(report.generation(), index.generation());
    assert_eq!(report.source_epoch(), index.source_epoch());
    assert_eq!(report.projected_records(), 1);
    assert_eq!(report.skipped_records(), 0);
    assert_eq!(report.unresolved_records(), 1);
    assert_eq!(report.git_queries(), 3);
    assert!(report.head_available());
    assert_eq!(
        report.maintenance(),
        crate::LocalMemoryMaintenance::Complete
    );

    let connection = Connection::open(&database).expect("database should open");
    let state = connection
        .query_row(
            "SELECT record.effective_state, record.validity_state,
                        record.evidence_state, record.reason,
                        (SELECT count(*) FROM memory_projection_evidence AS evidence
                         WHERE evidence.projection_id = projection.projection_id)
                 FROM active_memory_projections AS active
                 JOIN memory_projection_generations AS projection
                   ON projection.projection_id = active.projection_id
                 JOIN memory_projection_records AS record
                   ON record.projection_id = projection.projection_id
                 LIMIT 1",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            },
        )
        .expect("active projection should be readable");
    assert_eq!(
        state,
        (
            "indeterminate".to_owned(),
            "indeterminate".to_owned(),
            "not_evaluated".to_owned(),
            "project_indeterminate".to_owned(),
            0,
        )
    );
}

include!("tests/projection_fixtures.rs");

#[test]
fn exact_indexed_rust_evidence_becomes_current_end_to_end() {
    let (_fixture, _repository, database, _repository_identity, _identity) =
        exact_projection_fixture();

    let connection = Connection::open(&database).expect("database should open");
    let state = connection
        .query_row(
            "SELECT record.effective_state, record.validity_state,
                        record.evidence_state, record.reason,
                        evidence.outcome, evidence.assurance
                 FROM active_memory_projections AS active
                 JOIN memory_projection_records AS record
                   ON record.projection_id = active.projection_id
                 JOIN memory_projection_evidence AS evidence
                   ON evidence.projection_id = record.projection_id
                  AND evidence.record_ordinal = record.ordinal
                 LIMIT 1",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            },
        )
        .expect("exact active projection should be readable");
    assert_eq!(
        state,
        (
            "current".to_owned(),
            "valid".to_owned(),
            "exact".to_owned(),
            "evidence_exact".to_owned(),
            "exact".to_owned(),
            "automatic".to_owned(),
        )
    );
}

#[test]
fn recall_filters_and_returns_exact_current_memory_with_projection_coverage() {
    let (_fixture, _repository, database, repository_identity, _identity) =
        exact_projection_fixture();
    let reader = OwnedSqliteReader::start(&database, deadline()).expect("reader should start");
    let recalled = memory_recall(
        &reader,
        MemoryRecallRequest::new(
            repository_identity,
            MemoryRecallQuery::try_new("EXACT current").expect("query should be valid"),
            MemoryRecallLimits::default(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        ),
    )
    .expect("current memory should be recalled");
    assert_eq!(recalled.total_matches(), 1);
    assert_eq!(recalled.omitted_matches(), 0);
    assert_eq!(recalled.records().len(), 1);
    assert_eq!(
        recalled.records()[0].effective_state(),
        MemoryEffectiveState::Current
    );
    assert_eq!(
        recalled.records()[0]
            .record()
            .expect("selected revision should be present")
            .claim()
            .title()
            .as_str(),
        "Keep exact evidence current"
    );
    let target = recalled.records()[0].evidence()[0]
        .target()
        .expect("exact evidence should retain a target");
    assert_eq!(target.path().as_bytes(), b"src/lib.rs");

    let no_match = memory_recall(
        &reader,
        MemoryRecallRequest::new(
            repository_identity,
            MemoryRecallQuery::try_new("not-present").expect("query should be valid"),
            MemoryRecallLimits::default(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        ),
    )
    .expect("an empty match set should remain a complete result");
    assert_eq!(no_match.total_matches(), 0);
    assert!(no_match.records().is_empty());
    assert_eq!(
        no_match
            .projection_coverage()
            .state_count(MemoryEffectiveState::Current),
        1
    );
    reader.shutdown(deadline()).expect("reader should stop");
}

#[test]
fn a_new_source_generation_makes_the_old_memory_projection_unavailable() {
    let (_fixture, repository, database, repository_identity, identity) =
        exact_projection_fixture();
    fs::write(
        repository.join("src/lib.rs"),
        b"pub fn current() -> bool { false }\n",
    )
    .expect("source fixture should change");
    index_local_repository(
        LocalIndexRequest::new(&repository, &database, &identity, 123),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("new source generation should activate");
    let stale_reader =
        OwnedSqliteReader::start(&database, deadline()).expect("reader should restart");
    assert!(matches!(
        memory_recall(
            &stale_reader,
            MemoryRecallRequest::new(
                repository_identity,
                MemoryRecallQuery::all(),
                MemoryRecallLimits::default(),
                Arc::new(AtomicBool::new(false)),
                deadline(),
            ),
        ),
        Err(MemoryRecallError::Port(
            SqliteStoreError::MemoryProjectionUnavailable
        ))
    ));
    stale_reader
        .shutdown(deadline())
        .expect("stale reader should stop");
}

#[test]
fn invalid_limits_cancellation_and_debug_fail_before_database_io() {
    let fixture = TempDirectory::new();
    let repository = fixture.repository();
    let database = fixture.database();
    let identity = RepositoryIdentityTextV1::encode(RepositoryIdentityDigest::new([0_u8; 32]));
    let request =
        LocalMemoryRevalidationRequest::new(&repository, &database, identity.as_str(), 123);
    let debug = format!("{request:?}");
    assert!(!debug.contains(repository.to_string_lossy().as_ref()));
    assert!(!debug.contains(database.to_string_lossy().as_ref()));
    assert!(!debug.contains(identity.as_str()));

    let invalid = request.with_limits(LocalMemoryRevalidationLimits::new(
        Duration::ZERO,
        request.limits.source_state(),
        request.limits.git(),
        request.limits.max_versions(),
        request.limits.max_canonical_bytes(),
        request.limits.max_result_candidates(),
        request.limits.max_git_queries(),
    ));
    assert!(matches!(
        revalidate_local_memory(invalid, Arc::new(AtomicBool::new(false))),
        Err(LocalMemoryRevalidationError::InvalidLimits)
    ));
    assert!(!database.exists());

    assert!(matches!(
        revalidate_local_memory(request, Arc::new(AtomicBool::new(true))),
        Err(LocalMemoryRevalidationError::Cancelled)
    ));
    assert!(!database.exists());
}

mod finalization;
mod merges;
mod reviews;
