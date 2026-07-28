use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use repowitness_application::{
    ImmutableRustSource, ImportMemoryRecordRequest, MemoryEvidenceOutcome, MemoryImportApproval,
    RustArtifactIdentity, RustIndexLimits, RustSourceSnapshotIdentity, evaluate_memory_projection,
    import_memory_record, prepare_rust_index,
};
use repowitness_domain::{
    AnalysisSchemaDigest, CanonicalMemoryDigest, ConfigurationDigest, GitStateDigest,
    MemoryAuditActorId, MemoryCommitId, MemoryEvidence, MemoryObservationSource,
    MemoryPresentationDigest, MemoryProjectValidity, MemoryRecord, MemoryRecordedAtUnixMillis,
    MemoryRevalidationTarget, ProducerManifestDigest, RepositoryIdentityDigest, RepositoryPath,
    RepositoryPathLimits, WorktreeStateDigest,
};
use rusqlite::{Connection, TransactionBehavior, params};
use sha2::{Digest, Sha256};

use crate::{
    BackupLimits, MemoryFormatControl, OwnedSqliteReader, ProjectionRebuildLimits, SearchLimits,
    create_online_backup, parse_memory_record,
};

use super::{
    GenerationCoverage, OwnedSqliteIndex, SqliteStoreError, WriterCommand, mutation_lease_path,
};
use crate::sqlite::memory_projection::{
    MemoryProjectionLoadLimits, MemoryProjectionResultLimits, PreparedMemoryProjection,
    PreparedProjectionCandidate, PreparedProjectionEvidence, PreparedProjectionRecord,
    PreparedProjectionRecordKind, ProjectionCandidateRelation, ProjectionOccurrence,
};
use crate::sqlite::writer::MAX_STARTUP_RECOVERY_GENERATIONS;

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);
const PATH_LIMITS: RepositoryPathLimits = RepositoryPathLimits::new(4096, 256);
const COMMIT_MEMORY_YAML: &[u8] = include_bytes!("../../../tests/fixtures/memory-v1/commit.yaml");

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new() -> Self {
        let ordinal = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "repowitness-owned-store-{}-{ordinal}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("fixture directory should be created");
        Self(path)
    }

    fn database(&self) -> PathBuf {
        self.0.join("index.sqlite3")
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn deadline() -> Instant {
    Instant::now()
        .checked_add(Duration::from_secs(5))
        .expect("test deadline should be representable")
}

fn memory_input(
    yaml: &[u8],
) -> (
    MemoryRecord,
    CanonicalMemoryDigest,
    MemoryPresentationDigest,
) {
    let cancelled = AtomicBool::new(false);
    let parsed = parse_memory_record(yaml, MemoryFormatControl::new(&cancelled, deadline()))
        .expect("memory fixture should parse");
    let revision = parsed.digest();
    (
        parsed.into_record(),
        revision,
        MemoryPresentationDigest::new(Sha256::digest(yaml).into()),
    )
}

fn memory_actor() -> MemoryAuditActorId {
    MemoryAuditActorId::try_new("trusted-test-actor".to_owned())
        .expect("fixture audit actor should be valid")
}

fn memory_recorded_at() -> MemoryRecordedAtUnixMillis {
    MemoryRecordedAtUnixMillis::try_new(1_722_000_000_000)
        .expect("fixture timestamp should be valid")
}

fn memory_source() -> MemoryObservationSource {
    MemoryObservationSource::Git(MemoryCommitId::Sha1([0x11; 20]))
}

include!("tests/memory_import.rs");
include!("tests/generation.rs");
include!("tests/projection.rs");
include!("tests/recovery.rs");
