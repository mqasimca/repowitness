//! End-to-end Phase 2 admission of immutable Git-memory observations.

use std::{
    process::Command,
    sync::{Arc, atomic::AtomicBool},
};

use repowitness_application::RepositoryIdentityTextV1;
use repowitness_domain::RepositoryIdentityDigest;
use repowitness_local::{
    LocalIndexRequest, LocalMemoryApprovalRequest, LocalMemoryHistoryImportRequest,
    LocalMemoryRevalidationRequest, LocalMemoryWriteRequest, LocalPhase2ContextBuildRequest,
    LocalPhase2ContextItem, MemoryEffectiveState, Phase2ContextTier, approve_local_memory,
    build_local_phase2_context, import_local_memory_history, index_local_repository,
    revalidate_local_memory, write_local_memory,
};

#[allow(dead_code)]
#[path = "phase0_product_loop/mod.rs"]
mod fixture;

const MIGRATION_TIMESTAMP: u64 = 1_722_000_000_000;
const APPROVAL_TIMESTAMP: u64 = 1_722_000_000_001;

fn not_cancelled() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}

#[test]
fn phase2_history_requires_current_approved_memory_and_an_immutable_git_observation() {
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
            "phase2-history-fixture",
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
            "phase2-history-fixture",
            MIGRATION_TIMESTAMP,
            APPROVAL_TIMESTAMP + 1,
        ),
        not_cancelled(),
    )
    .expect("the committed memory should import as an observation");
    assert!(imported.appended_observations() >= 1);

    let context = build_local_phase2_context(
        LocalPhase2ContextBuildRequest::new(
            &repository,
            &database,
            repository_identity.as_str(),
            "publish",
        ),
        not_cancelled(),
    )
    .expect("the pinned source and current projection should build context");
    assert!(context.items().iter().any(|item| {
        item.tier() == Phase2ContextTier::History
            && matches!(item.payload(), LocalPhase2ContextItem::History(history)
                if history.commit() == observed_commit
                    && history.record().effective_state() == MemoryEffectiveState::Current)
    }));
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "the end-to-end stale-history fixture keeps every lifecycle transition visible"
)]
fn phase2_context_never_emits_stale_memory_or_its_history_receipt() {
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
            "phase2-history-stale-fixture",
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
            "phase2-history-stale-fixture",
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

    let context = build_local_phase2_context(
        LocalPhase2ContextBuildRequest::new(
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
            LocalPhase2ContextItem::Memory(_) | LocalPhase2ContextItem::History(_)
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
