//! End-to-end evidence-balanced admission of immutable Git-memory observations.

use std::{
    process::Command,
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};

use repowitness_application::RepositoryIdentityTextV1;
use repowitness_domain::{
    MemoryObservationSource, MemoryRecordedAtUnixMillis, RepositoryIdentityDigest,
    SourceSnapshotDigest,
};
use repowitness_local::{
    EvidenceContextTier, KnownAtApplicability, KnownAtEvidenceBasis, KnownAtHistoryCoverage,
    LocalEvidenceContextBuildRequest, LocalEvidenceContextItem, LocalIndexRequest,
    LocalKnownAtHistoryRequest, LocalMemoryApprovalRequest, LocalMemoryHistoryImportRequest,
    LocalMemoryRevalidationRequest, LocalMemoryWriteRequest, MemoryEffectiveState,
    OwnedSqliteReader, approve_local_memory, build_local_evidence_context,
    import_local_memory_history, index_local_repository, read_local_known_at_history,
    revalidate_local_memory, write_local_memory,
};
use rusqlite::Connection;

#[allow(dead_code)]
#[path = "phase0_product_loop/mod.rs"]
mod fixture;

const MIGRATION_TIMESTAMP: u64 = 1_722_000_000_000;
const APPROVAL_TIMESTAMP: u64 = 1_722_000_000_001;

fn not_cancelled() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one end-to-end timeline proves the recorded-time cutoff cannot leak later approvals"
)]
fn evidence_history_requires_current_approved_memory_and_an_immutable_git_observation() {
    let directory = fixture::TempDirectory::new();
    let repository = directory.repository();
    let database = directory.database();
    fixture::initialize_repository(&repository);
    let repository_digest = RepositoryIdentityDigest::new([0xD2; 32]);
    let repository_identity = RepositoryIdentityTextV1::encode(repository_digest);

    index_local_repository(
        LocalIndexRequest::new(
            &repository,
            &database,
            repository_identity.as_str(),
            MIGRATION_TIMESTAMP,
        ),
        not_cancelled(),
    )
    .expect("the committed source fixture should index");
    let memory_yaml = fixture::exact_memory_yaml(
        &database,
        repository_digest,
        fixture::head_commit(&repository),
    );
    let written = write_local_memory(
        LocalMemoryWriteRequest::from_bytes(
            &repository,
            &memory_yaml,
            repository_identity.as_str(),
        ),
        not_cancelled(),
    )
    .expect("the exact current memory should publish");
    assert!(written.created());
    approve_local_memory(
        LocalMemoryApprovalRequest::new(
            &repository,
            &database,
            repository_identity.as_str(),
            &fixture::record_id_text(),
            "evidence-history-fixture",
            MIGRATION_TIMESTAMP,
            APPROVAL_TIMESTAMP,
        ),
        not_cancelled(),
    )
    .expect("the exact current memory should be locally approved");
    revalidate_local_memory(
        LocalMemoryRevalidationRequest::new(
            &repository,
            &database,
            repository_identity.as_str(),
            MIGRATION_TIMESTAMP,
        ),
        not_cancelled(),
    )
    .expect("the approved memory should revalidate before history import");

    git(&repository, &["add", "-f", ".code-memory/records"]);
    git(
        &repository,
        &[
            "commit",
            "--quiet",
            "-m",
            "record historical memory observation",
        ],
    );
    let observed_commit = fixture::head_commit(&repository);
    let imported = import_local_memory_history(
        LocalMemoryHistoryImportRequest::new(
            &repository,
            &database,
            repository_identity.as_str(),
            "evidence-history-fixture",
            MIGRATION_TIMESTAMP,
            APPROVAL_TIMESTAMP + 1,
        ),
        not_cancelled(),
    )
    .expect("the committed memory should import as an observation");
    assert!(imported.appended_observations() >= 1);

    let deadline = Instant::now() + Duration::from_secs(5);
    let reader = OwnedSqliteReader::start(&database, deadline)
        .expect("the immutable journal should open read-only");
    let before_observation = reader
        .known_at_trusted_git_history_evidence(
            repository_digest,
            MemoryRecordedAtUnixMillis::try_new(APPROVAL_TIMESTAMP)
                .expect("fixture timestamp is representable"),
            16,
            not_cancelled(),
            deadline,
        )
        .expect("the journal-only historical read should succeed");
    assert!(
        before_observation.is_empty(),
        "later audit events must not leak backward"
    );
    let after_observation = reader
        .known_at_trusted_git_history_evidence(
            repository_digest,
            MemoryRecordedAtUnixMillis::try_new(APPROVAL_TIMESTAMP + 1)
                .expect("fixture timestamp is representable"),
            16,
            not_cancelled(),
            deadline,
        )
        .expect("the completed historical observation should be visible");
    assert_eq!(after_observation.len(), 1);
    assert_eq!(after_observation[0].commit(), observed_commit);

    let target_snapshot = retained_active_snapshot(&database, repository_digest);
    let before_worktree_receipt = reader
        .known_at_history_receipt(
            repository_digest,
            MemoryRecordedAtUnixMillis::try_new(MIGRATION_TIMESTAMP)
                .expect("fixture timestamp is representable"),
            MemoryObservationSource::Worktree(target_snapshot),
            16,
            not_cancelled(),
            deadline,
        )
        .expect("the retained historical target should be readable");
    assert!(before_worktree_receipt.evidence().is_empty());
    assert_eq!(
        before_worktree_receipt.applicability(),
        KnownAtApplicability::NotApplicable,
        "a retained target with no pre-cutoff approval must not become applicable"
    );
    let after_worktree_receipt = reader
        .known_at_history_receipt(
            repository_digest,
            MemoryRecordedAtUnixMillis::try_new(APPROVAL_TIMESTAMP)
                .expect("fixture timestamp is representable"),
            MemoryObservationSource::Worktree(target_snapshot),
            16,
            not_cancelled(),
            deadline,
        )
        .expect("the retained historical target should be readable");
    assert_eq!(after_worktree_receipt.evidence().len(), 1);
    assert_eq!(
        after_worktree_receipt.evidence()[0].basis(),
        KnownAtEvidenceBasis::Observation
    );
    assert_eq!(
        after_worktree_receipt.applicability(),
        KnownAtApplicability::Applicable,
        "the exact retained observation and pre-cutoff approval should apply"
    );

    let before_receipt = reader
        .known_at_history_receipt(
            repository_digest,
            MemoryRecordedAtUnixMillis::try_new(APPROVAL_TIMESTAMP)
                .expect("fixture timestamp is representable"),
            MemoryObservationSource::Git(observed_commit),
            16,
            not_cancelled(),
            deadline,
        )
        .expect("the target-bound journal receipt should succeed");
    assert!(before_receipt.evidence().is_empty());
    assert_eq!(before_receipt.coverage(), KnownAtHistoryCoverage::Complete);
    assert_eq!(
        before_receipt.applicability(),
        KnownAtApplicability::Unavailable,
        "a journal receipt must not claim Git or snapshot applicability"
    );
    let after_receipt = reader
        .known_at_history_receipt(
            repository_digest,
            MemoryRecordedAtUnixMillis::try_new(APPROVAL_TIMESTAMP + 1)
                .expect("fixture timestamp is representable"),
            MemoryObservationSource::Git(observed_commit),
            16,
            not_cancelled(),
            deadline,
        )
        .expect("the target-bound journal receipt should succeed");
    assert_eq!(after_receipt.evidence().len(), 1);
    assert_eq!(
        after_receipt.evidence()[0].source(),
        MemoryObservationSource::Git(observed_commit)
    );
    assert_eq!(after_receipt.coverage(), KnownAtHistoryCoverage::Complete);
    let evaluated_before = read_local_known_at_history(
        LocalKnownAtHistoryRequest::new(
            &repository,
            &database,
            repository_identity.as_str(),
            APPROVAL_TIMESTAMP,
            MemoryObservationSource::Git(observed_commit),
        ),
        not_cancelled(),
    )
    .expect("the exact Git object should be checked without reading a projection");
    assert_eq!(
        evaluated_before.applicability(),
        KnownAtApplicability::NotApplicable,
        "later observed evidence must not leak through the Git object fence"
    );
    let evaluated_after = read_local_known_at_history(
        LocalKnownAtHistoryRequest::new(
            &repository,
            &database,
            repository_identity.as_str(),
            APPROVAL_TIMESTAMP + 1,
            MemoryObservationSource::Git(observed_commit),
        ),
        not_cancelled(),
    )
    .expect("the exact Git object should be checked without reading a projection");
    assert_eq!(
        evaluated_after.applicability(),
        KnownAtApplicability::Applicable,
        "an existing exact Git object and pre-cutoff observation should apply"
    );
    reader.shutdown(deadline).expect("reader should shut down");

    let context = build_local_evidence_context(
        LocalEvidenceContextBuildRequest::new(
            &repository,
            &database,
            repository_identity.as_str(),
            "publish",
        ),
        not_cancelled(),
    )
    .expect("the pinned source and current projection should build context");
    assert!(context.items().iter().any(|item| {
        item.tier() == EvidenceContextTier::History
            && matches!(item.payload(), LocalEvidenceContextItem::History(history)
                if history.commit() == observed_commit
                    && history.record().effective_state() == MemoryEffectiveState::Current)
    }));
}

fn retained_active_snapshot(
    database: &std::path::Path,
    repository: RepositoryIdentityDigest,
) -> SourceSnapshotDigest {
    let connection = Connection::open(database).expect("database should open");
    let snapshot: Vec<u8> = connection
        .query_row(
            "SELECT generation.snapshot_digest
               FROM workspaces AS workspace
               JOIN index_generations AS generation
                 ON generation.generation_id = workspace.active_generation_id
              WHERE workspace.repository_identity = ?1
                AND generation.lifecycle_state = 'active'",
            [repository.as_bytes().as_slice()],
            |row| row.get(0),
        )
        .expect("fixture should retain its active source snapshot");
    SourceSnapshotDigest::try_from_slice(&snapshot).expect("snapshot should be well formed")
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "the end-to-end stale-history fixture keeps every lifecycle transition visible"
)]
fn evidence_context_never_emits_stale_memory_or_its_history_receipt() {
    let directory = fixture::TempDirectory::new();
    let repository = directory.repository();
    let database = directory.database();
    fixture::initialize_repository(&repository);
    let repository_digest = RepositoryIdentityDigest::new([0xD3; 32]);
    let repository_identity = RepositoryIdentityTextV1::encode(repository_digest);

    index_local_repository(
        LocalIndexRequest::new(
            &repository,
            &database,
            repository_identity.as_str(),
            MIGRATION_TIMESTAMP,
        ),
        not_cancelled(),
    )
    .expect("the committed source fixture should index");
    let memory_yaml = fixture::exact_memory_yaml(
        &database,
        repository_digest,
        fixture::head_commit(&repository),
    );
    write_local_memory(
        LocalMemoryWriteRequest::from_bytes(
            &repository,
            &memory_yaml,
            repository_identity.as_str(),
        ),
        not_cancelled(),
    )
    .expect("the exact memory should publish");
    approve_local_memory(
        LocalMemoryApprovalRequest::new(
            &repository,
            &database,
            repository_identity.as_str(),
            &fixture::record_id_text(),
            "evidence-history-stale-fixture",
            MIGRATION_TIMESTAMP,
            APPROVAL_TIMESTAMP,
        ),
        not_cancelled(),
    )
    .expect("the exact memory should be locally approved");
    revalidate_local_memory(
        LocalMemoryRevalidationRequest::new(
            &repository,
            &database,
            repository_identity.as_str(),
            MIGRATION_TIMESTAMP,
        ),
        not_cancelled(),
    )
    .expect("the approved memory should initially revalidate");
    git(&repository, &["add", "-f", ".code-memory/records"]);
    git(
        &repository,
        &[
            "commit",
            "--quiet",
            "-m",
            "record memory before source change",
        ],
    );
    import_local_memory_history(
        LocalMemoryHistoryImportRequest::new(
            &repository,
            &database,
            repository_identity.as_str(),
            "evidence-history-stale-fixture",
            MIGRATION_TIMESTAMP,
            APPROVAL_TIMESTAMP + 1,
        ),
        not_cancelled(),
    )
    .expect("the committed memory should import as an observation");

    std::fs::write(repository.join("src/lib.rs"), fixture::AFTER_SOURCE)
        .expect("the semantic source change should be written");
    index_local_repository(
        LocalIndexRequest::new(
            &repository,
            &database,
            repository_identity.as_str(),
            MIGRATION_TIMESTAMP + 2,
        ),
        not_cancelled(),
    )
    .expect("the changed source fixture should index");
    revalidate_local_memory(
        LocalMemoryRevalidationRequest::new(
            &repository,
            &database,
            repository_identity.as_str(),
            MIGRATION_TIMESTAMP + 2,
        ),
        not_cancelled(),
    )
    .expect("the changed source should make the old evidence non-current");

    let context = build_local_evidence_context(
        LocalEvidenceContextBuildRequest::new(
            &repository,
            &database,
            repository_identity.as_str(),
            "publish",
        ),
        not_cancelled(),
    )
    .expect("the changed pinned source should build a context");
    assert!(context.items().iter().all(|item| {
        !matches!(
            item.payload(),
            LocalEvidenceContextItem::Memory(_) | LocalEvidenceContextItem::History(_)
        )
    }));
}

fn git(repository: &std::path::Path, arguments: &[&str]) {
    let status = Command::new("git")
        .args(["-c", "core.hooksPath=/dev/null"])
        .current_dir(repository)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .args(arguments)
        .status()
        .expect("Git fixture command should start");
    assert!(status.success(), "Git fixture command should succeed");
}
