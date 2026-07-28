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

use repowitness_domain::{
    AnalysisArtifactDigest, ByteOffset, ByteSpan, DeclarationDigest, MemoryActorId,
    MemoryActorKind, MemoryAssurance, MemoryBody, MemoryClaim, MemoryCommitId,
    MemoryDisplayRevision, MemoryEvidence, MemoryEvidenceIndex, MemoryFactOrdinal, MemoryKind,
    MemoryLifecycle, MemoryProducerId, MemoryProducerVersion, MemoryProvenance,
    MemoryProvenanceOrigin, MemoryQualifiedName, MemoryRecord, MemoryRecordHeader, MemoryRecordId,
    MemoryScope, MemorySymbolName, MemoryTitle, MemoryValidity, ProducerIdentity,
    RepositoryIdentityDigest, RepositoryPath, RepositoryPathLimits, RustMemorySymbolKind,
    RustSymbolMemoryEvidence, SourceContentDigest, SourceSnapshotDigest,
};
use repowitness_local::{MemoryFormatControl, MemoryRecordIdTextV1, generate_memory_yaml};
use rusqlite::Connection;

pub const BEFORE_SOURCE: &[u8] = include_bytes!("../fixtures/phase0-product-loop/before.rs");
pub const AFTER_SOURCE: &[u8] = include_bytes!("../fixtures/phase0-product-loop/after.rs");

const RECORD_ID: MemoryRecordId = MemoryRecordId::new([0x91; 16]);
static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

pub struct TempDirectory(PathBuf);

impl TempDirectory {
    pub fn new() -> Self {
        let ordinal = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "repowitness-phase0-product-loop-{}-{ordinal}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("fixture directory should be created");
        Self(path)
    }

    pub fn repository(&self) -> PathBuf {
        self.0.join("repository")
    }

    pub fn database(&self) -> PathBuf {
        self.0.join("index.sqlite3")
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub fn initialize_repository(repository: &Path) {
    fs::create_dir_all(repository.join("src")).expect("source directory should be created");
    git(repository, &["init", "--quiet"]);
    git(repository, &["config", "user.name", "RepoWitness Test"]);
    git(
        repository,
        &["config", "user.email", "repowitness@example.invalid"],
    );
    fs::write(repository.join(".gitignore"), b".code-memory/\n")
        .expect("memory directory should be ignored");
    fs::write(repository.join("src/lib.rs"), BEFORE_SOURCE)
        .expect("initial source should be written");
    git(repository, &["add", "--", ".gitignore", "src/lib.rs"]);
    git(repository, &["commit", "--quiet", "-m", "phase0 fixture"]);
}

pub fn head_commit(repository: &Path) -> MemoryCommitId {
    let output = git_output(repository, &["rev-parse", "--verify", "HEAD"]);
    let hex = std::str::from_utf8(&output)
        .expect("Git object identity should be UTF-8")
        .trim();
    assert_eq!(hex.len(), 40, "the fixture should use SHA-1 Git objects");
    let mut bytes = [0_u8; 20];
    for (index, byte) in bytes.iter_mut().enumerate() {
        let offset = index * 2;
        *byte = u8::from_str_radix(&hex[offset..offset + 2], 16)
            .expect("Git object identity should be lowercase hexadecimal");
    }
    MemoryCommitId::Sha1(bytes)
}

pub fn record_id_text() -> String {
    MemoryRecordIdTextV1::encode(RECORD_ID).into_string()
}

pub fn exact_memory_yaml(
    database: &Path,
    repository: RepositoryIdentityDigest,
    introduced_by: MemoryCommitId,
) -> Vec<u8> {
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
    let record = exact_memory_record(repository, introduced_by, evidence);
    let cancelled = Arc::new(AtomicBool::new(false));
    generate_memory_yaml(
        &record,
        MemoryFormatControl::new(cancelled.as_ref(), Instant::now() + Duration::from_secs(10)),
    )
    .expect("the exact memory record should serialize")
}

fn exact_memory_record(
    repository: RepositoryIdentityDigest,
    introduced_by: MemoryCommitId,
    evidence: RustSymbolMemoryEvidence,
) -> MemoryRecord {
    MemoryRecord::try_new(
        MemoryRecordHeader::try_new(
            RECORD_ID,
            MemoryDisplayRevision::try_new(1).expect("display revision should be valid"),
            Vec::new(),
        )
        .expect("record header should be valid"),
        MemoryClaim::new(
            MemoryKind::Decision,
            MemoryTitle::try_new("Keep publish behavior stable".to_owned())
                .expect("title should be valid"),
            MemoryBody::try_new(
                "The publish function remains the supported release decision.".to_owned(),
            )
            .expect("body should be valid"),
        ),
        MemoryScope::new(
            repository,
            MemoryEvidenceIndex::try_new(0).expect("evidence index should be valid"),
        ),
        MemoryProvenance::new(
            MemoryProvenanceOrigin::Human,
            MemoryActorKind::LocalAsserted,
            MemoryActorId::try_new("phase0-maintainer".to_owned()).expect("actor should be valid"),
        ),
        MemoryAssurance::LocallyApproved,
        MemoryLifecycle::Active,
        MemoryValidity::try_commits(vec![introduced_by], Vec::new())
            .expect("commit validity should be valid"),
        vec![MemoryEvidence::RustSymbol(evidence)],
        Vec::new(),
        false,
    )
    .expect("the exact memory record should be valid")
}

fn git(repository: &Path, arguments: &[&str]) {
    let status = git_command(repository)
        .args(arguments)
        .status()
        .expect("Git fixture command should start");
    assert!(status.success(), "Git fixture command should succeed");
}

fn git_output(repository: &Path, arguments: &[&str]) -> Vec<u8> {
    let output = git_command(repository)
        .args(arguments)
        .output()
        .expect("Git fixture command should start");
    assert!(
        output.status.success(),
        "Git fixture command should succeed"
    );
    output.stdout
}

fn git_command(repository: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .args(["-c", "core.hooksPath=/dev/null"])
        .current_dir(repository)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0");
    command
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
             LIMIT 1",
            [repository.as_bytes().as_slice()],
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
        .expect("one indexed Rust function should exist")
}
