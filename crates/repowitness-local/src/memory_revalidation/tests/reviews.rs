use repowitness_application::{MemoryRecallEvidenceAssurance, MemoryRecallEvidenceOutcome};
use repowitness_domain::{
    CanonicalMemoryDigest, MemoryCorrespondenceReviewOperation, MemoryRecordId, RepositoryPath,
    RepositoryPathLimits,
};

use super::*;
use crate::{
    LocalMemoryCorrespondenceReviewRequest, LocalMemoryRecallRequest, LocalMemoryRecallSelection,
    MemoryRecordIdTextV1, RepositoryPathTextByteLimit, RepositoryPathTextV1, recall_local_memory,
    review_local_memory_correspondence, sqlite::memory_review::CorrespondenceReviewDecision,
};

struct ReviewSelector {
    record_id: String,
    revision: String,
    path: String,
    artifact: String,
    fact_ordinal: u64,
}

fn review_selector(database: &Path, repository: RepositoryIdentityDigest) -> ReviewSelector {
    review_selector_at(database, repository, 0)
}

fn review_selector_at(
    database: &Path,
    repository: RepositoryIdentityDigest,
    occurrence_offset: u32,
) -> ReviewSelector {
    let occurrence = active_occurrence_at(database, repository, occurrence_offset);
    let record_id = MemoryRecordId::new([0x91; 16]);
    let revision = Connection::open(database)
        .expect("database should open")
        .query_row(
            "SELECT revision_digest
             FROM memory_versions
             WHERE record_id = ?1",
            [record_id.as_bytes().as_slice()],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .expect("approved memory revision should exist");
    let path = RepositoryPath::try_from_vec(
        occurrence.path,
        RepositoryPathLimits::new(1_048_576, 65_535),
    )
    .expect("persisted path should be valid");
    let path = RepositoryPathTextV1::encode(&path, RepositoryPathTextByteLimit::new(65_535))
        .expect("path text should be representable");

    ReviewSelector {
        record_id: MemoryRecordIdTextV1::encode(record_id).into_string(),
        revision: hex(&revision),
        path: path.into_string(),
        artifact: hex(&occurrence.artifact),
        fact_ordinal: u64::try_from(occurrence.fact_ordinal)
            .expect("fact ordinal should be nonnegative"),
    }
}

fn append_review(
    repository: &Path,
    database: &Path,
    identity: &str,
    selector: &ReviewSelector,
    operation: MemoryCorrespondenceReviewOperation,
    actor: &str,
    recorded_at: u64,
) -> bool {
    review_local_memory_correspondence(
        LocalMemoryCorrespondenceReviewRequest::new(
            repository,
            database,
            identity,
            &selector.record_id,
            &selector.revision,
            0,
            operation,
            &selector.path,
            &selector.artifact,
            selector.fact_ordinal,
            actor,
            123,
            recorded_at,
        ),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("valid correspondence review should append")
    .inserted()
}

fn recall_one(database: &Path, identity: &str) -> crate::LocalMemoryRecallResult {
    recall_local_memory(
        LocalMemoryRecallRequest::new(database, identity, LocalMemoryRecallSelection::All),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("active memory projection should recall")
}

fn multiple_target_projection_fixture() -> (
    TempDirectory,
    PathBuf,
    PathBuf,
    RepositoryIdentityDigest,
    String,
) {
    let fixture = TempDirectory::new();
    let repository = fixture.repository();
    let database = fixture.database();
    initialize_repository(&repository);
    fs::write(
        repository.join("src/lib.rs"),
        b"pub fn current() -> bool { true }\npub fn alternate() -> bool { true }\n",
    )
    .expect("multiple-target source should be written");
    git(&repository, &["add", "src/lib.rs"]);
    git(
        &repository,
        &["commit", "--quiet", "-m", "multiple targets"],
    );
    let repository_identity = RepositoryIdentityDigest::new([0xA6; 32]);
    let identity = RepositoryIdentityTextV1::encode(repository_identity);
    index_local_repository(
        LocalIndexRequest::new(&repository, &database, identity.as_str(), 123),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("multiple-target source index should activate");
    import_exact_memory(&database, repository_identity);
    revalidate_local_memory(
        LocalMemoryRevalidationRequest::new(&repository, &database, identity.as_str(), 123),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("initial exact projection should activate");
    (
        fixture,
        repository,
        database,
        repository_identity,
        identity.into_string(),
    )
}

#[test]
fn trusted_manual_link_is_idempotent_and_drives_reviewed_revalidation() {
    let (_fixture, repository, database, repository_identity, identity) =
        exact_projection_fixture();
    let selector = review_selector(&database, repository_identity);

    assert!(append_review(
        &repository,
        &database,
        &identity,
        &selector,
        MemoryCorrespondenceReviewOperation::ManualLink,
        "trusted-reviewer",
        1_722_000_000_100,
    ));
    assert!(!append_review(
        &repository,
        &database,
        &identity,
        &selector,
        MemoryCorrespondenceReviewOperation::ManualLink,
        "trusted-reviewer",
        1_722_000_000_101,
    ));

    let report = revalidate_local_memory(
        LocalMemoryRevalidationRequest::new(&repository, &database, &identity, 123),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("reviewed projection should activate");
    assert_eq!(report.unresolved_records(), 0);

    let recalled = recall_one(&database, &identity);
    assert_eq!(recalled.records().len(), 1);
    let evidence = &recalled.records()[0].evidence()[0];
    assert_eq!(
        evidence.outcome(),
        MemoryRecallEvidenceOutcome::ReviewedLink
    );
    assert_eq!(
        evidence.assurance(),
        MemoryRecallEvidenceAssurance::Reviewed
    );
    let target = evidence
        .target()
        .expect("reviewed link should retain an exact target");
    assert_eq!(
        *target.artifact_digest().as_bytes(),
        hex_bytes(&selector.artifact)
    );
    assert_eq!(target.fact_ordinal(), selector.fact_ordinal);

    let audit_count = Connection::open(&database)
        .expect("database should open")
        .query_row(
            "SELECT count(*) FROM memory_correspondence_audit",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("review count should be readable");
    assert_eq!(audit_count, 1);
}

#[test]
fn conflicting_positive_and_negative_reviews_fail_closed() {
    let (_fixture, repository, database, repository_identity, identity) =
        exact_projection_fixture();
    let selector = review_selector(&database, repository_identity);
    assert!(append_review(
        &repository,
        &database,
        &identity,
        &selector,
        MemoryCorrespondenceReviewOperation::Approved,
        "trusted-approver",
        1_722_000_000_200,
    ));
    assert!(append_review(
        &repository,
        &database,
        &identity,
        &selector,
        MemoryCorrespondenceReviewOperation::Rejected,
        "trusted-rejector",
        1_722_000_000_201,
    ));

    let report = revalidate_local_memory(
        LocalMemoryRevalidationRequest::new(&repository, &database, &identity, 123),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("conflicting review should project as indeterminate");
    assert_eq!(report.unresolved_records(), 1);
    let recalled = recall_one(&database, &identity);
    let evidence = &recalled.records()[0].evidence()[0];
    assert_eq!(
        evidence.outcome(),
        MemoryRecallEvidenceOutcome::Indeterminate
    );
    assert_eq!(evidence.assurance(), MemoryRecallEvidenceAssurance::None);
    assert!(evidence.target().is_none());
}

#[test]
fn competing_approved_targets_fail_closed_independent_of_actor_and_operation() {
    let (_fixture, repository, database, repository_identity, identity) =
        multiple_target_projection_fixture();
    let first = review_selector_at(&database, repository_identity, 0);
    let second = review_selector_at(&database, repository_identity, 1);
    assert_ne!(first.fact_ordinal, second.fact_ordinal);
    assert!(append_review(
        &repository,
        &database,
        &identity,
        &first,
        MemoryCorrespondenceReviewOperation::Approved,
        "first-reviewer",
        1_722_000_000_225,
    ));
    assert!(append_review(
        &repository,
        &database,
        &identity,
        &second,
        MemoryCorrespondenceReviewOperation::ManualLink,
        "second-reviewer",
        1_722_000_000_226,
    ));

    let report = revalidate_local_memory(
        LocalMemoryRevalidationRequest::new(&repository, &database, &identity, 123),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("competing positive reviews should project as indeterminate");
    assert_eq!(report.unresolved_records(), 1);
    let recalled = recall_one(&database, &identity);
    let evidence = &recalled.records()[0].evidence()[0];
    assert_eq!(
        evidence.outcome(),
        MemoryRecallEvidenceOutcome::Indeterminate
    );
    assert_eq!(evidence.assurance(), MemoryRecallEvidenceAssurance::None);
    assert!(evidence.target().is_none());
    assert!(evidence.candidates().is_empty());
}

#[test]
fn an_obsolete_target_snapshot_review_does_not_affect_the_active_generation() {
    let (_fixture, repository, database, repository_identity, identity) =
        exact_commit_projection_fixture();
    let selector = review_selector(&database, repository_identity);
    assert!(append_review(
        &repository,
        &database,
        &identity,
        &selector,
        MemoryCorrespondenceReviewOperation::ManualLink,
        "snapshot-reviewer",
        1_722_000_000_240,
    ));

    fs::write(
        repository.join("src/lib.rs"),
        b"pub fn current() -> bool { false }\n",
    )
    .expect("new source snapshot should be written");
    git(&repository, &["add", "src/lib.rs"]);
    git(
        &repository,
        &["commit", "--quiet", "-m", "change reviewed target"],
    );
    index_local_repository(
        LocalIndexRequest::new(&repository, &database, &identity, 123),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("new source generation should activate");

    let cancelled = Arc::new(AtomicBool::new(false));
    let (store, _) =
        OwnedSqliteIndex::start(&database, 123, deadline()).expect("store should reopen");
    let source = store
        .load_memory_source(repository_identity, Arc::clone(&cancelled), deadline())
        .expect("active source should load");
    let revision = CanonicalMemoryDigest::try_from_slice(&hex_bytes(&selector.revision))
        .expect("review revision should be valid");
    let reviews = store
        .load_memory_correspondence_reviews(
            source,
            MemoryRecordId::new([0x91; 16]),
            revision,
            0,
            Arc::clone(&cancelled),
            deadline(),
        )
        .expect("current-snapshot reviews should load");
    assert_eq!(reviews.decision(), &CorrespondenceReviewDecision::None);
    store.shutdown(deadline()).expect("store should shut down");

    let report = revalidate_local_memory(
        LocalMemoryRevalidationRequest::new(&repository, &database, &identity, 123),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("current snapshot should revalidate without the obsolete review");
    assert_eq!(report.unresolved_records(), 0);
    let recalled = recall_one(&database, &identity);
    assert_eq!(
        recalled.records()[0].effective_state(),
        MemoryEffectiveState::Stale
    );
    let evidence = &recalled.records()[0].evidence()[0];
    assert_eq!(evidence.outcome(), MemoryRecallEvidenceOutcome::Changed);
    assert_eq!(evidence.assurance(), MemoryRecallEvidenceAssurance::None);

    let audit_count = Connection::open(&database)
        .expect("database should open")
        .query_row(
            "SELECT count(*) FROM memory_correspondence_audit",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("obsolete review should remain auditable");
    assert_eq!(audit_count, 1);
}

#[test]
fn an_unopposed_rejection_never_selects_a_fallback_target() {
    let (_fixture, repository, database, repository_identity, identity) =
        exact_projection_fixture();
    let selector = review_selector(&database, repository_identity);
    assert!(append_review(
        &repository,
        &database,
        &identity,
        &selector,
        MemoryCorrespondenceReviewOperation::Rejected,
        "trusted-rejector",
        1_722_000_000_250,
    ));

    let report = revalidate_local_memory(
        LocalMemoryRevalidationRequest::new(&repository, &database, &identity, 123),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("rejected exact target should project conservatively");
    assert_eq!(report.unresolved_records(), 1);
    let recalled = recall_one(&database, &identity);
    let evidence = &recalled.records()[0].evidence()[0];
    assert_eq!(
        evidence.outcome(),
        MemoryRecallEvidenceOutcome::Indeterminate
    );
    assert_eq!(evidence.assurance(), MemoryRecallEvidenceAssurance::None);
    assert!(evidence.target().is_none());
    assert!(evidence.candidates().is_empty());
}

#[test]
fn invalid_review_targets_append_no_audit_event() {
    let (_fixture, repository, database, repository_identity, identity) =
        exact_projection_fixture();
    let selector = review_selector(&database, repository_identity);
    let invalid_artifact = "ff".repeat(32);
    let error = review_local_memory_correspondence(
        LocalMemoryCorrespondenceReviewRequest::new(
            &repository,
            &database,
            &identity,
            &selector.record_id,
            &selector.revision,
            0,
            MemoryCorrespondenceReviewOperation::Approved,
            &selector.path,
            &invalid_artifact,
            selector.fact_ordinal,
            "trusted-reviewer",
            123,
            1_722_000_000_275,
        ),
        Arc::new(AtomicBool::new(false)),
    )
    .expect_err("a nonexistent exact target should be rejected");
    assert_eq!(
        error,
        crate::LocalMemoryManageError::ReviewTargetUnavailable
    );

    let audit_count = Connection::open(&database)
        .expect("database should open")
        .query_row(
            "SELECT count(*) FROM memory_correspondence_audit",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("review count should be readable");
    assert_eq!(audit_count, 0);
}

#[cfg(unix)]
#[test]
fn review_reports_database_replacement_after_the_commit() {
    let (fixture, repository, database, repository_identity, identity) = exact_projection_fixture();
    let selector = review_selector(&database, repository_identity);
    let moved = fixture.path.join("review-writer-opened.sqlite3");
    let request = LocalMemoryCorrespondenceReviewRequest::new(
        &repository,
        &database,
        &identity,
        &selector.record_id,
        &selector.revision,
        0,
        MemoryCorrespondenceReviewOperation::Approved,
        &selector.path,
        &selector.artifact,
        selector.fact_ordinal,
        "trusted-reviewer",
        123,
        1_722_000_000_300,
    );

    let receipt = crate::memory_management::review_local_memory_correspondence_with_hook(
        request,
        Arc::new(AtomicBool::new(false)),
        || {
            fs::rename(&database, &moved).expect("writer-opened database should move");
            fs::copy(&moved, &database).expect("database path should be replaced");
        },
    )
    .expect("known review commit should retain its receipt");

    assert!(receipt.inserted());
    let maintenance = receipt.maintenance();
    assert!(!maintenance.complete());
    assert_eq!(maintenance.warning_count(), 1);
    assert_eq!(
        maintenance.database_identity(),
        LocalMemoryDatabaseIdentity::ChangedAfterCommit
    );
    assert_eq!(
        maintenance.checkpoint(),
        LocalMemoryMaintenanceStep::Complete
    );
    assert_eq!(maintenance.shutdown(), LocalMemoryMaintenanceStep::Complete);
    let reviews: i64 = Connection::open(moved)
        .expect("writer-opened database should remain readable")
        .query_row(
            "SELECT count(*) FROM memory_correspondence_audit",
            [],
            |row| row.get(0),
        )
        .expect("known review should remain durable");
    assert_eq!(reviews, 1);
}

#[test]
fn review_boundaries_and_debug_are_redacted_before_database_io() {
    let private_repository = Path::new("/private/repository");
    let private_database = Path::new("/private/index.sqlite3");
    let request = LocalMemoryCorrespondenceReviewRequest::new(
        private_repository,
        private_database,
        "private-identity",
        "private-record",
        "private-revision",
        0,
        MemoryCorrespondenceReviewOperation::Approved,
        "private-path",
        "private-artifact",
        0,
        "private-actor",
        123,
        1_722_000_000_300,
    );
    let debug = format!("{request:?}");
    assert!(!debug.contains("private"));

    assert_eq!(
        review_local_memory_correspondence(request, Arc::new(AtomicBool::new(false)),)
            .expect_err("invalid identity should fail before database I/O"),
        crate::LocalMemoryManageError::RepositoryIdentityInvalid
    );
    assert!(!private_database.exists());
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    output
}

fn hex_bytes(text: &str) -> [u8; 32] {
    let mut output = [0_u8; 32];
    for (index, pair) in text.as_bytes().chunks_exact(2).enumerate() {
        output[index] = (hex_nibble(pair[0]) << 4) | hex_nibble(pair[1]);
    }
    output
}

const fn hex_nibble(value: u8) -> u8 {
    match value {
        b'0'..=b'9' => value - b'0',
        b'a'..=b'f' => value - b'a' + 10,
        _ => panic!("test digest should be lowercase hexadecimal"),
    }
}
