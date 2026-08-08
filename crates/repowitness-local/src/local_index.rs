use std::{
    borrow::Cow,
    error::Error,
    fmt, fs,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use repowitness_application::{
    CompleteStagedSourceSlotIndexError, ConfigurationResolutionError, PackageScope,
    PublishSourceSlotIndexRequest, RepositoryIdentityTextError, RepositoryIdentityTextV1,
    ResolvedConfiguration, RustArtifactIdentity, RustIndexCoverage, RustSourceSnapshotIdentity,
    SourceArtifactIdentities, SourceLanguage, SourceSlotFinalFence,
    complete_staged_source_slot_index, hash_source_snapshot, phase0_source_artifact_identities,
    phase0_source_snapshot_profile, phase1_rust_graph_artifact_identity,
    raw_syntax_artifact_identities, resolve_configuration, stage_source_slot_index,
};
use repowitness_domain::{AnalysisSchemaDigest, ConfigurationDigest, ProducerManifestDigest};
use sha2::{Digest, Sha256};

use crate::{
    GenerationId, LocalRustIndexError, LocalRustIndexLimits, OwnedSqliteIndex, OwnedSqliteReader,
    SourceSlotEpoch, SqliteStoreError,
    contained_source::{FileIdentity, file_has_single_link},
    git_paths::discovered_worktree_root,
    local_graph_index::{
        LocalRustGraphProjectionError, PreparedLocalRustGraphProjection,
        prepare_local_rust_graph_projection, prepare_local_rust_graph_projection_for_source_slot,
    },
    rust_index::{
        LocalSourceIndexReuseRequest, LocalSourceSnapshotFenceError, SourceLanguageSelection,
        prepare_local_source_index_with_full_reuse_deferred_to_publication,
    },
    sqlite::SqliteMutationLease,
};

mod final_fence;
use final_fence::LocalSourceSlotFinalFence;
pub(crate) mod connected_workspace;
pub(crate) mod polling_runner;
mod post_commit;

const INITIAL_SOURCE_EPOCH: u64 = 0;
const LOCAL_SNAPSHOT_PRODUCER_DOMAIN: &[u8] =
    b"RepoWitness\0phase0-local-supported-languages-snapshot-producer\0";
const LOCAL_SNAPSHOT_CONFIGURATION_DOMAIN: &[u8] =
    b"RepoWitness\0phase1-local-resolved-snapshot-configuration\0";
const LOCAL_SNAPSHOT_CONFIGURATION_VERSION: u32 = 1;
const CONNECTED_SCOPE_CONFIGURATION_DOMAIN: &[u8] =
    b"RepoWitness\0phase1-connected-scope-configuration\0";
const CONNECTED_SCOPE_ARTIFACT_CONFIGURATION_DOMAIN: &[u8] =
    b"RepoWitness\0phase1-connected-scope-artifact-configuration\0";
const CONNECTED_SCOPE_CONFIGURATION_VERSION: u32 = 1;
/// Version of the local source-acquisition and snapshot-publication profile.
///
/// This intentionally does not participate in content-local analysis artifact
/// identities. A change to snapshot fencing must force a fresh publication,
/// while unchanged parser artifacts remain safe to reuse after their content
/// and analysis identity have been verified.
const LOCAL_SNAPSHOT_PRODUCER_VERSION: u32 = 5;

include!("local_index/model.rs");

/// Durable operation that callers must reconcile after an unknown index outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalIndexMutation {
    /// Writer startup recovery may have committed before its receipt was lost.
    StoreStartup,
    /// Workspace registration or source-epoch reservation may have committed.
    WorkspaceRegistration,
    /// Candidate generation staging may have committed.
    GenerationStaging,
    /// Required Rust graph staging may have committed.
    GraphStaging,
    /// Required raw syntax-site staging may have committed.
    RawSyntaxSiteStaging,
    /// Required path-only topology staging may have committed.
    RepositoryTopologyStaging,
    /// Source-slot completion or active-generation publication may have committed.
    GenerationPublication,
    /// A terminal WAL checkpoint may have completed.
    Checkpoint,
}

impl LocalIndexMutation {
    /// Returns non-sensitive authoritative reconciliation guidance.
    #[must_use]
    pub const fn reconciliation_guidance(self) -> &'static str {
        match self {
            Self::StoreStartup => {
                "reopen the store and run read-only database diagnostics before retrying startup"
            }
            Self::WorkspaceRegistration => {
                "reopen the store and read the durable workspace or source-slot epoch before retrying"
            }
            Self::GenerationStaging => {
                "reopen the store, allow startup recovery to classify incomplete generations, and compare the active generation before retrying"
            }
            Self::GraphStaging => {
                "reopen the store and inspect the candidate generation graph receipt before retrying"
            }
            Self::RawSyntaxSiteStaging => {
                "reopen the store and inspect the candidate generation syntax-site receipt before retrying"
            }
            Self::RepositoryTopologyStaging => {
                "reopen the store and inspect the candidate generation topology receipt before retrying"
            }
            Self::GenerationPublication => {
                "reopen the store and read the active generation and source-slot completion before retrying"
            }
            Self::Checkpoint => {
                "reopen the store and inspect the already-published active generation before retrying maintenance"
            }
        }
    }
}

/// Stable failure phase for the complete local indexing composition.
#[derive(Debug)]
pub enum LocalIndexError {
    /// The configured repository identity text was invalid.
    RepositoryIdentity {
        /// Stable validation failure without identity bytes.
        source: RepositoryIdentityTextError,
    },
    /// The built-in semantic configuration could not be resolved.
    ConfigurationResolution {
        /// Stable path-free resolution failure.
        source: ConfigurationResolutionError,
    },
    /// Effective configuration and compiled indexing limits did not compose.
    InvalidEffectiveConfiguration,
    /// The end-to-end monotonic deadline could not be represented.
    DeadlineNotRepresentable,
    /// Repository discovery, source capture, or analysis failed.
    Preparation {
        /// Stable local preparation failure.
        source: LocalRustIndexError,
    },
    /// The explicit database path could not be resolved safely.
    DatabasePathUnavailable,
    /// The database path would modify the indexed worktree.
    DatabaseInsideWorktree,
    /// The database has hard-link aliases that can bypass path-based isolation.
    DatabaseHasMultipleLinks,
    /// The database filesystem identity changed while source preparation ran.
    DatabaseChangedDuringIndexing,
    /// SQLite startup, migration, or recovery failed.
    StoreStartup {
        /// Stable SQLite boundary failure.
        source: SqliteStoreError,
    },
    /// Existing reusable artifacts could not be loaded or validated.
    ArtifactReuse {
        /// Stable SQLite boundary failure.
        source: SqliteStoreError,
    },
    /// The repository workspace could not be registered.
    WorkspaceRegistration {
        /// Stable SQLite boundary failure.
        source: SqliteStoreError,
    },
    /// Candidate generation staging failed without activation.
    PublicationStaging {
        /// Stable SQLite boundary failure.
        source: SqliteStoreError,
    },
    /// Complete generation-scoped Rust graph preparation failed.
    GraphPreparation {
        /// Stable path-free graph preparation failure.
        source: LocalRustGraphProjectionError,
    },
    /// Complete raw all-language syntax-site preparation failed.
    RawSyntaxPreparation {
        /// Stable raw syntax-site projection failure.
        source: crate::RawSyntaxPreparationError,
    },
    /// Complete path-only repository topology preparation failed.
    RepositoryTopologyPreparation {
        /// Stable path-only topology preparation failure.
        source: crate::RepositoryTopologyPreparationError,
    },
    /// Rust graph staging failed without activation.
    GraphPublicationStaging {
        /// Stable SQLite boundary failure.
        source: SqliteStoreError,
    },
    /// Raw syntax-site staging failed without activation.
    RawSyntaxPublicationStaging {
        /// Stable SQLite boundary failure.
        source: SqliteStoreError,
    },
    /// Path-only topology staging failed without activation.
    RepositoryTopologyPublicationStaging {
        /// Stable SQLite boundary failure.
        source: SqliteStoreError,
    },
    /// The authoritative post-staging source snapshot fence failed.
    FinalSourceFence {
        /// Stable path-free source-fence failure.
        source: LocalSourceSnapshotFenceError,
    },
    /// Atomic generation activation failed.
    PublicationActivation {
        /// Stable SQLite boundary failure.
        source: SqliteStoreError,
    },
    /// The post-activation WAL checkpoint failed.
    Checkpoint {
        /// Stable SQLite boundary failure.
        source: SqliteStoreError,
    },
    /// The owned SQLite writer did not shut down cleanly.
    Shutdown {
        /// Stable SQLite boundary failure.
        source: SqliteStoreError,
    },
    /// A queued mutation may have committed but no receipt arrived in bounded grace.
    MutationOutcomeUnknown {
        /// Exact durable operation whose authoritative state must be read.
        operation: LocalIndexMutation,
    },
}

impl fmt::Display for LocalIndexError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::RepositoryIdentity { .. } => "repository identity is invalid",
            Self::ConfigurationResolution { .. } => "local index configuration resolution failed",
            Self::InvalidEffectiveConfiguration => {
                "local index configuration is incompatible with compiled limits"
            }
            Self::DeadlineNotRepresentable => "local index deadline is not representable",
            Self::Preparation { .. } => "local source index preparation failed",
            Self::DatabasePathUnavailable => "local index database path is unavailable",
            Self::DatabaseInsideWorktree => {
                "local index database must be outside the repository worktree"
            }
            Self::DatabaseHasMultipleLinks => {
                "local index database must not have hard-link aliases"
            }
            Self::DatabaseChangedDuringIndexing => "local index database changed during indexing",
            Self::StoreStartup { .. } => "local index store startup failed",
            Self::ArtifactReuse { .. } => "local index reusable artifact loading failed",
            Self::WorkspaceRegistration { .. } => "local index workspace registration failed",
            Self::PublicationStaging { .. } => "local index generation staging failed",
            Self::GraphPreparation { .. } => "local Rust graph preparation failed",
            Self::RawSyntaxPreparation { .. } => "local raw syntax-site preparation failed",
            Self::RepositoryTopologyPreparation { .. } => {
                "local repository topology preparation failed"
            }
            Self::GraphPublicationStaging { .. } => "local Rust graph staging failed",
            Self::RawSyntaxPublicationStaging { .. } => "local raw syntax-site staging failed",
            Self::RepositoryTopologyPublicationStaging { .. } => {
                "local repository topology staging failed"
            }
            Self::FinalSourceFence { .. } => "local index final source fence failed",
            Self::PublicationActivation { .. } => "local index generation activation failed",
            Self::Checkpoint { .. } => "local index checkpoint failed after activation",
            Self::Shutdown { .. } => "local index writer shutdown failed after activation",
            Self::MutationOutcomeUnknown { .. } => {
                "local index mutation outcome could not be determined"
            }
        })
    }
}

impl LocalIndexError {
    /// Returns operation-specific guidance only for an outcome-unknown mutation.
    #[must_use]
    pub const fn reconciliation_guidance(&self) -> Option<&'static str> {
        match self {
            Self::MutationOutcomeUnknown { operation } => Some(operation.reconciliation_guidance()),
            _ => None,
        }
    }
}

impl Error for LocalIndexError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::RepositoryIdentity { source } => Some(source),
            Self::ConfigurationResolution { source } => Some(source),
            Self::Preparation { source } => Some(source),
            Self::StoreStartup { source }
            | Self::ArtifactReuse { source }
            | Self::WorkspaceRegistration { source }
            | Self::PublicationStaging { source }
            | Self::GraphPublicationStaging { source }
            | Self::RawSyntaxPublicationStaging { source }
            | Self::RepositoryTopologyPublicationStaging { source }
            | Self::PublicationActivation { source }
            | Self::Checkpoint { source }
            | Self::Shutdown { source } => Some(source),
            Self::GraphPreparation { source } => Some(source),
            Self::RawSyntaxPreparation { source } => Some(source),
            Self::RepositoryTopologyPreparation { source } => Some(source),
            Self::FinalSourceFence { source } => Some(source),
            Self::InvalidEffectiveConfiguration
            | Self::DeadlineNotRepresentable
            | Self::DatabasePathUnavailable
            | Self::DatabaseInsideWorktree
            | Self::DatabaseHasMultipleLinks
            | Self::DatabaseChangedDuringIndexing
            | Self::MutationOutcomeUnknown { .. } => None,
        }
    }
}

/// Prepares, stages, validates, and atomically activates one local source index.
///
/// The explicit cancellation flag is shared with preparation and persistence.
/// An existing database may be opened read-only after bounded source capture
/// to load reusable artifacts. A new database is not created until repository
/// identity and preparation have succeeded.
///
/// Do not retry [`LocalIndexError::MutationOutcomeUnknown`] until the caller
/// follows its operation-specific [`LocalIndexError::reconciliation_guidance`].
pub fn index_local_rust_repository(
    request: LocalIndexRequest<'_>,
    cancelled: Arc<AtomicBool>,
) -> Result<LocalIndexReport, LocalIndexError> {
    index_local_rust_repository_with_hooks(request, cancelled, || {}, || {})
}

/// Language-neutral entry point for the local supported-language index.
///
/// Unknown mutation outcomes have the same reconciliation requirement as
/// [`index_local_rust_repository`].
pub fn index_local_repository(
    request: LocalIndexRequest<'_>,
    cancelled: Arc<AtomicBool>,
) -> Result<LocalIndexReport, LocalIndexError> {
    index_local_rust_repository(request, cancelled)
}

/// Reconciles one local source index without publishing a new generation when
/// its complete source and semantics-affecting inputs are unchanged.
///
/// The current worktree is still captured and final-fenced before returning an
/// existing generation. Changed or incomplete state follows the normal atomic
/// publication path.
pub fn reconcile_local_repository(
    request: LocalIndexRequest<'_>,
    cancelled: Arc<AtomicBool>,
) -> Result<LocalIndexReport, LocalIndexError> {
    match index_local_repository_with_mode(request, cancelled, true, || {}, || {})? {
        LocalReconciliationOutcome::Published(report)
        | LocalReconciliationOutcome::Resumed(report)
        | LocalReconciliationOutcome::Unchanged(report) => Ok(report),
    }
}

struct PreparedLocalIndexPublication {
    identity: RustSourceSnapshotIdentity,
    prepared: repowitness_application::PreparedRustIndex,
    graph: PreparedLocalRustGraphProjection,
    raw_syntax: crate::PreparedRawSyntaxGeneration,
    topology: Option<crate::PreparedRepositoryTopology>,
    coverage: RustIndexCoverage,
}

#[cfg(test)]
fn index_local_rust_repository_with_hook(
    request: LocalIndexRequest<'_>,
    cancelled: Arc<AtomicBool>,
    after_lease: impl FnOnce(),
) -> Result<LocalIndexReport, LocalIndexError> {
    index_local_rust_repository_with_hooks(request, cancelled, after_lease, || {})
}

fn index_local_rust_repository_with_hooks(
    request: LocalIndexRequest<'_>,
    cancelled: Arc<AtomicBool>,
    after_lease: impl FnOnce(),
    after_graph_staging: impl FnOnce(),
) -> Result<LocalIndexReport, LocalIndexError> {
    let mut after_lease = Some(after_lease);
    let mut after_graph_staging = Some(after_graph_staging);
    match index_local_repository_with_mode_and_control(
        request,
        cancelled,
        false,
        move |phase| match phase {
            LocalIndexPhase::MutationLeaseAcquired => {
                if let Some(hook) = after_lease.take() {
                    hook();
                }
            }
            LocalIndexPhase::GraphStaged => {
                if let Some(hook) = after_graph_staging.take() {
                    hook();
                }
            }
            LocalIndexPhase::WriterStarted | LocalIndexPhase::PublicationCommitted => {}
        },
        |_, deadline| deadline,
    )? {
        LocalReconciliationOutcome::Published(report) => Ok(report),
        LocalReconciliationOutcome::Resumed(_) | LocalReconciliationOutcome::Unchanged(_) => {
            unreachable!("one-shot indexing always publishes a fresh generation")
        }
    }
}

#[cfg(test)]
fn index_local_rust_repository_with_control_hooks(
    request: LocalIndexRequest<'_>,
    cancelled: Arc<AtomicBool>,
    after_phase: impl FnMut(LocalIndexPhase),
    maintenance_deadline: impl FnMut(post_commit::PostCommitMaintenancePhase, Instant) -> Instant,
) -> Result<LocalIndexReport, LocalIndexError> {
    match index_local_repository_with_mode_and_control(
        request,
        cancelled,
        false,
        after_phase,
        maintenance_deadline,
    )? {
        LocalReconciliationOutcome::Published(report) => Ok(report),
        LocalReconciliationOutcome::Resumed(_) | LocalReconciliationOutcome::Unchanged(_) => {
            unreachable!("one-shot indexing always publishes a fresh generation")
        }
    }
}

fn publish_prepared_local_index(
    writer: &OwnedSqliteIndex,
    repository: repowitness_domain::RepositoryIdentityDigest,
    publication: PreparedLocalIndexPublication,
    final_fence: &LocalSourceSlotFinalFence<'_>,
    after_graph_staging: impl FnOnce(),
    cancelled: &Arc<AtomicBool>,
    deadline: Instant,
) -> Result<(GenerationId, SourceSlotEpoch), LocalIndexError> {
    let persisted_epoch = writer
        .ensure_workspace(repository, INITIAL_SOURCE_EPOCH, deadline)
        .map_err(|source| {
            map_index_mutation_error(
                LocalIndexMutation::WorkspaceRegistration,
                source,
                |source| LocalIndexError::WorkspaceRegistration { source },
            )
        })?;
    publish_prepared_local_index_at_epoch(
        writer,
        repository,
        persisted_epoch,
        publication,
        final_fence,
        after_graph_staging,
        cancelled,
        deadline,
    )
}

#[allow(
    clippy::too_many_arguments,
    reason = "publication identity, epoch, fence, hook, and control remain explicit"
)]
fn publish_prepared_local_index_at_epoch(
    writer: &OwnedSqliteIndex,
    repository: repowitness_domain::RepositoryIdentityDigest,
    persisted_epoch: SourceSlotEpoch,
    publication: PreparedLocalIndexPublication,
    final_fence: &LocalSourceSlotFinalFence<'_>,
    after_graph_staging: impl FnOnce(),
    cancelled: &Arc<AtomicBool>,
    deadline: Instant,
) -> Result<(GenerationId, SourceSlotEpoch), LocalIndexError> {
    let connected_workspace =
        repowitness_domain::ConnectedWorkspaceId::for_single_repository(repository);
    let source_slot = repowitness_domain::SourceSlotId::for_repository(repository);
    let reserved_epoch = writer
        .reserve_source_slot_epoch(
            connected_workspace,
            source_slot,
            persisted_epoch,
            Arc::clone(cancelled),
            deadline,
        )
        .map_err(|source| {
            map_index_mutation_error(
                LocalIndexMutation::WorkspaceRegistration,
                source,
                |source| LocalIndexError::WorkspaceRegistration { source },
            )
        })?;
    let staged = stage_source_slot_index(
        writer,
        PublishSourceSlotIndexRequest::new(
            connected_workspace,
            source_slot,
            reserved_epoch,
            publication.identity,
            publication.prepared,
            publication.coverage,
            Arc::clone(cancelled),
            deadline,
        ),
    )
    .map_err(|source| {
        map_index_mutation_error(LocalIndexMutation::GenerationStaging, source, |source| {
            LocalIndexError::PublicationStaging { source }
        })
    })?;
    let generation = staged.generation();
    let graph = publication
        .graph
        .into_generation(generation, cancelled.as_ref(), deadline)
        .map_err(|source| LocalIndexError::GraphPreparation { source })?;
    writer
        .stage_rust_graph(generation, graph, Arc::clone(cancelled), deadline)
        .map_err(|source| {
            map_index_mutation_error(LocalIndexMutation::GraphStaging, source, |source| {
                LocalIndexError::GraphPublicationStaging { source }
            })
        })?;
    writer
        .stage_raw_syntax_sites(
            generation,
            publication.raw_syntax,
            Arc::clone(cancelled),
            deadline,
        )
        .map_err(|source| {
            map_index_mutation_error(LocalIndexMutation::RawSyntaxSiteStaging, source, |source| {
                LocalIndexError::RawSyntaxPublicationStaging { source }
            })
        })?;
    if let Some(topology) = publication.topology {
        writer
            .stage_repository_topology(generation, topology, Arc::clone(cancelled), deadline)
            .map_err(|source| {
                map_index_mutation_error(
                    LocalIndexMutation::RepositoryTopologyStaging,
                    source,
                    |source| LocalIndexError::RepositoryTopologyPublicationStaging { source },
                )
            })?;
    }
    after_graph_staging();
    let completed = complete_staged_source_slot_index(writer, final_fence, staged).map_err(
        |error| match error {
            CompleteStagedSourceSlotIndexError::FinalFence(source) => {
                LocalIndexError::FinalSourceFence { source }
            }
            CompleteStagedSourceSlotIndexError::Complete(source) => map_index_mutation_error(
                LocalIndexMutation::GenerationPublication,
                source,
                |source| LocalIndexError::PublicationActivation { source },
            ),
        },
    )?;
    writer
        .activate(
            completed.generation(),
            completed.source_epoch().get(),
            deadline,
        )
        .map_err(|source| {
            map_index_mutation_error(
                LocalIndexMutation::GenerationPublication,
                source,
                |source| LocalIndexError::PublicationActivation { source },
            )
        })?;
    Ok((completed.generation(), completed.source_epoch()))
}

fn map_store_startup_error(source: SqliteStoreError) -> LocalIndexError {
    if source == SqliteStoreError::MutationOutcomeUnknown {
        return LocalIndexError::MutationOutcomeUnknown {
            operation: LocalIndexMutation::StoreStartup,
        };
    }
    match source {
        SqliteStoreError::DatabaseIdentityChanged => LocalIndexError::DatabaseChangedDuringIndexing,
        source => LocalIndexError::StoreStartup { source },
    }
}

fn map_index_mutation_error(
    operation: LocalIndexMutation,
    source: SqliteStoreError,
    otherwise: impl FnOnce(SqliteStoreError) -> LocalIndexError,
) -> LocalIndexError {
    if source == SqliteStoreError::MutationOutcomeUnknown {
        LocalIndexError::MutationOutcomeUnknown { operation }
    } else {
        otherwise(source)
    }
}

include!("local_index/reconciliation.rs");
include!("local_index/preparation.rs");

#[cfg(test)]
mod tests;
