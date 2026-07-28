//! Public Phase 0 source-change, memory-revalidation, recall, and context fixture.

use std::{
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
};

use repowitness_application::RepositoryIdentityTextV1;
use repowitness_domain::RepositoryIdentityDigest;
use repowitness_local::{
    ContextItem, ContextOmission, GenerationId, LocalContextBuildRequest, LocalIndexRequest,
    LocalMemoryApprovalRequest, LocalMemoryRecallRequest, LocalMemoryRecallSelection,
    LocalMemoryRevalidationRequest, LocalMemoryWriteRequest, MemoryEffectiveState,
    MemoryRecallEvidenceOutcome, approve_local_memory, build_local_context, index_local_repository,
    recall_local_memory, revalidate_local_memory, write_local_memory,
};

#[path = "phase0_product_loop/mod.rs"]
mod fixture;

use fixture::{
    AFTER_SOURCE, BEFORE_SOURCE, TempDirectory, exact_memory_yaml, head_commit,
    initialize_repository, record_id_text,
};

const MIGRATION_TIMESTAMP: u64 = 1_722_000_000_000;
const APPROVAL_TIMESTAMP: u64 = 1_722_000_000_001;

fn not_cancelled() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}

struct ProductLoopFixture {
    _directory: TempDirectory,
    repository: PathBuf,
    database: PathBuf,
    repository_digest: RepositoryIdentityDigest,
    repository_identity: RepositoryIdentityTextV1,
}

impl ProductLoopFixture {
    fn new() -> Self {
        let directory = TempDirectory::new();
        let repository = directory.repository();
        let database = directory.database();
        initialize_repository(&repository);
        let repository_digest = RepositoryIdentityDigest::new([0xA4; 32]);
        let repository_identity = RepositoryIdentityTextV1::encode(repository_digest);
        Self {
            _directory: directory,
            repository,
            database,
            repository_digest,
            repository_identity,
        }
    }

    fn identity(&self) -> &str {
        self.repository_identity.as_str()
    }
}

#[test]
fn source_change_revalidates_recall_and_context_through_public_local_apis() {
    let fixture = ProductLoopFixture::new();
    let initial_generation = index_write_and_approve(&fixture);
    assert_current_projection_and_recall(&fixture, initial_generation);
    assert_current_context(&fixture);
    let changed_generation = change_source_and_reindex(&fixture, initial_generation);
    assert_stale_projection_and_recall(&fixture, changed_generation);
    assert_stale_context(&fixture);
}

fn index_write_and_approve(fixture: &ProductLoopFixture) -> GenerationId {
    let commit = head_commit(&fixture.repository);
    let initial_index = index_local_repository(
        LocalIndexRequest::new(
            &fixture.repository,
            &fixture.database,
            fixture.identity(),
            MIGRATION_TIMESTAMP,
        ),
        not_cancelled(),
    )
    .expect("the committed Rust fixture should index");
    let yaml = exact_memory_yaml(&fixture.database, fixture.repository_digest, commit);
    let written = write_local_memory(
        LocalMemoryWriteRequest::from_bytes(&fixture.repository, &yaml, fixture.identity()),
        not_cancelled(),
    )
    .expect("the canonical memory record should publish");
    assert!(written.created());

    let record_id = record_id_text();
    let approved = approve_local_memory(
        LocalMemoryApprovalRequest::new(
            &fixture.repository,
            &fixture.database,
            fixture.identity(),
            &record_id,
            "phase0-product-fixture",
            MIGRATION_TIMESTAMP,
            APPROVAL_TIMESTAMP,
        ),
        not_cancelled(),
    )
    .expect("the exact current record should be approved");
    assert_eq!(approved.revision(), written.revision());
    assert!(approved.version_inserted());
    assert!(approved.observation_inserted());
    assert!(approved.approval_inserted());
    initial_index.generation()
}

fn assert_current_projection_and_recall(
    fixture: &ProductLoopFixture,
    initial_generation: GenerationId,
) {
    let initial_projection = revalidate_local_memory(
        LocalMemoryRevalidationRequest::new(
            &fixture.repository,
            &fixture.database,
            fixture.identity(),
            MIGRATION_TIMESTAMP,
        ),
        not_cancelled(),
    )
    .expect("the approved memory should revalidate");
    assert_eq!(initial_projection.generation(), initial_generation);
    assert_eq!(initial_projection.projected_records(), 1);
    assert_eq!(initial_projection.unresolved_records(), 0);

    let current = recall_local_memory(
        LocalMemoryRecallRequest::new(
            &fixture.database,
            fixture.identity(),
            LocalMemoryRecallSelection::Query("publish"),
        ),
        not_cancelled(),
    )
    .expect("the current memory should be recalled");
    assert_eq!(current.records().len(), 1);
    assert_eq!(
        current.records()[0].effective_state(),
        MemoryEffectiveState::Current
    );
    assert_eq!(
        current.records()[0].evidence()[0].outcome(),
        MemoryRecallEvidenceOutcome::Exact
    );
}

fn assert_current_context(fixture: &ProductLoopFixture) {
    let initial_context = build_local_context(
        LocalContextBuildRequest::new(
            &fixture.repository,
            &fixture.database,
            fixture.identity(),
            "publish",
        ),
        not_cancelled(),
    )
    .expect("current memory and exact source should compile into context");
    assert!(
        initial_context
            .items()
            .iter()
            .any(|item| matches!(item, ContextItem::Memory(_)))
    );
    assert!(
        initial_context
            .items()
            .iter()
            .any(|item| matches!(item, ContextItem::Source(_)))
    );
    assert_eq!(initial_context.coverage().memory_included(), 1);
}

fn change_source_and_reindex(
    fixture: &ProductLoopFixture,
    initial_generation: GenerationId,
) -> GenerationId {
    std::fs::write(fixture.repository.join("src/lib.rs"), AFTER_SOURCE)
        .expect("the fixture source should change");
    assert_ne!(BEFORE_SOURCE, AFTER_SOURCE);
    let changed_index = index_local_repository(
        LocalIndexRequest::new(
            &fixture.repository,
            &fixture.database,
            fixture.identity(),
            MIGRATION_TIMESTAMP,
        ),
        not_cancelled(),
    )
    .expect("the changed Rust fixture should reindex");
    assert_ne!(changed_index.generation(), initial_generation);
    changed_index.generation()
}

fn assert_stale_projection_and_recall(
    fixture: &ProductLoopFixture,
    changed_generation: GenerationId,
) {
    let changed_projection = revalidate_local_memory(
        LocalMemoryRevalidationRequest::new(
            &fixture.repository,
            &fixture.database,
            fixture.identity(),
            MIGRATION_TIMESTAMP,
        ),
        not_cancelled(),
    )
    .expect("the changed source should produce a new memory projection");
    assert_eq!(changed_projection.generation(), changed_generation);
    assert_eq!(changed_projection.projected_records(), 1);

    let stale = recall_local_memory(
        LocalMemoryRecallRequest::new(
            &fixture.database,
            fixture.identity(),
            LocalMemoryRecallSelection::Query("publish"),
        ),
        not_cancelled(),
    )
    .expect("the changed memory state should remain explicitly recallable");
    assert_eq!(stale.records().len(), 1);
    assert_eq!(
        stale.records()[0].effective_state(),
        MemoryEffectiveState::Stale
    );
    assert_eq!(
        stale.records()[0].evidence()[0].outcome(),
        MemoryRecallEvidenceOutcome::Changed
    );
    assert_eq!(
        stale
            .projection_coverage()
            .state_count(MemoryEffectiveState::Stale),
        1
    );
}

fn assert_stale_context(fixture: &ProductLoopFixture) {
    let changed_context = build_local_context(
        LocalContextBuildRequest::new(
            &fixture.repository,
            &fixture.database,
            fixture.identity(),
            "publish",
        ),
        not_cancelled(),
    )
    .expect("changed source should compile while stale memory is excluded");
    assert!(
        changed_context
            .items()
            .iter()
            .all(|item| !matches!(item, ContextItem::Memory(_)))
    );
    assert!(
        changed_context
            .items()
            .iter()
            .any(|item| matches!(item, ContextItem::Source(_)))
    );
    assert_eq!(changed_context.coverage().memory_non_current_omitted(), 1);
    assert!(
        changed_context
            .omissions()
            .contains(&ContextOmission::MemoryNotCurrent(1))
    );
}
