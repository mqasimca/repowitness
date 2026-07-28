use std::{
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
    PublishRustIndexError, PublishRustIndexRequest, RepositoryIdentityTextError,
    RepositoryIdentityTextV1, RustArtifactIdentity, RustIndexCoverage, RustSourceSnapshotIdentity,
    SourceArtifactIdentities, SourceLanguage, phase0_source_artifact_identities,
    phase0_source_snapshot_profile, publish_rust_index,
};
use repowitness_domain::{AnalysisSchemaDigest, ConfigurationDigest, ProducerManifestDigest};
use sha2::{Digest, Sha256};

use crate::{
    GenerationId, LocalRustIndexError, LocalRustIndexLimits, OwnedSqliteIndex, OwnedSqliteReader,
    SqliteStoreError, contained_source::FileIdentity, git_paths::discovered_worktree_root,
    rust_index::prepare_local_source_index_excluding_identity_with_reuse,
    sqlite::SqliteMutationLease,
};

const ONE_SHOT_SOURCE_EPOCH: u64 = 0;
const LOCAL_PRODUCER_DOMAIN: &[u8] = b"RepoWitness\0phase0-local-source-producer\0";
const LOCAL_SNAPSHOT_PRODUCER_DOMAIN: &[u8] =
    b"RepoWitness\0phase0-local-supported-languages-snapshot-producer\0";
const LOCAL_PRODUCER_VERSION: u32 = 4;

/// Complete explicit input for one bounded local Phase 0 indexing operation.
#[derive(Clone, Copy)]
pub struct LocalIndexRequest<'a> {
    repository_root: &'a Path,
    database: &'a Path,
    repository_identity: &'a str,
    migration_applied_at_unix_ms: u64,
    limits: LocalRustIndexLimits,
}

impl fmt::Debug for LocalIndexRequest<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalIndexRequest")
            .field("repository_root", &"<redacted-path>")
            .field("database", &"<redacted-path>")
            .field("repository_identity", &"<redacted-identity>")
            .field(
                "migration_applied_at_unix_ms",
                &self.migration_applied_at_unix_ms,
            )
            .field("limits", &self.limits)
            .finish()
    }
}

impl<'a> LocalIndexRequest<'a> {
    /// Constructs a request using the conservative default indexing limits.
    #[must_use]
    pub fn new(
        repository_root: &'a Path,
        database: &'a Path,
        repository_identity: &'a str,
        migration_applied_at_unix_ms: u64,
    ) -> Self {
        Self {
            repository_root,
            database,
            repository_identity,
            migration_applied_at_unix_ms,
            limits: LocalRustIndexLimits::default(),
        }
    }

    /// Replaces the complete end-to-end resource policy.
    #[must_use]
    pub const fn with_limits(mut self, limits: LocalRustIndexLimits) -> Self {
        self.limits = limits;
        self
    }
}

/// Non-sensitive aggregate outcome from one activated local generation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalIndexReport {
    generation: GenerationId,
    source_epoch: u64,
    recovered_generations: u64,
    discovered_paths: u64,
    indexed_rust_files: u64,
    indexed_go_files: u64,
    indexed_typescript_files: u64,
    indexed_tsx_files: u64,
    indexed_python_files: u64,
    skipped_unsupported_paths: u64,
    total_source_bytes: u64,
    total_facts: u64,
    syntax_error_nodes: u64,
    known_parser_limitation_nodes: u64,
    reused_rust_files: u64,
    analyzed_rust_files: u64,
    reused_go_files: u64,
    analyzed_go_files: u64,
    reused_typescript_files: u64,
    analyzed_typescript_files: u64,
    reused_tsx_files: u64,
    analyzed_tsx_files: u64,
    reused_python_files: u64,
    analyzed_python_files: u64,
}

impl LocalIndexReport {
    /// Returns the database-local active generation identity.
    #[must_use]
    pub const fn generation(self) -> GenerationId {
        self.generation
    }

    /// Returns the source epoch compared during atomic activation.
    #[must_use]
    pub const fn source_epoch(self) -> u64 {
        self.source_epoch
    }

    /// Returns incomplete generations recovered when the writer started.
    #[must_use]
    pub const fn recovered_generations(self) -> u64 {
        self.recovered_generations
    }

    /// Returns all repository paths admitted by bounded Git discovery.
    #[must_use]
    pub const fn discovered_paths(self) -> u64 {
        self.discovered_paths
    }

    /// Returns case-sensitive `.rs` files included in this generation.
    #[must_use]
    pub const fn indexed_rust_files(self) -> u64 {
        self.indexed_rust_files
    }

    /// Returns case-sensitive `.go` files included in this generation.
    #[must_use]
    pub const fn indexed_go_files(self) -> u64 {
        self.indexed_go_files
    }

    /// Returns case-sensitive `.ts` files included in this generation.
    #[must_use]
    pub const fn indexed_typescript_files(self) -> u64 {
        self.indexed_typescript_files
    }

    /// Returns case-sensitive `.tsx` files included in this generation.
    #[must_use]
    pub const fn indexed_tsx_files(self) -> u64 {
        self.indexed_tsx_files
    }

    /// Returns case-sensitive `.py` and `.pyi` files included in this generation.
    #[must_use]
    pub const fn indexed_python_files(self) -> u64 {
        self.indexed_python_files
    }

    /// Returns discovered paths outside the supported language scope.
    #[must_use]
    pub const fn skipped_unsupported_paths(self) -> u64 {
        self.skipped_unsupported_paths
    }

    /// Compatibility accessor for paths outside the indexed language scope.
    #[must_use]
    pub const fn skipped_non_rust_paths(self) -> u64 {
        self.skipped_unsupported_paths
    }

    /// Returns exact analyzed supported-source bytes.
    #[must_use]
    pub const fn total_source_bytes(self) -> u64 {
        self.total_source_bytes
    }

    /// Returns extracted symbol facts in the active generation.
    #[must_use]
    pub const fn total_facts(self) -> u64 {
        self.total_facts
    }

    /// Returns explicit Tree-sitter error-node coverage.
    #[must_use]
    pub const fn syntax_error_nodes(self) -> u64 {
        self.syntax_error_nodes
    }

    /// Returns the non-subtractive subset caused by known parser limitations.
    #[must_use]
    pub const fn known_parser_limitation_nodes(self) -> u64 {
        self.known_parser_limitation_nodes
    }

    /// Returns files restored from exact persisted analysis artifacts.
    #[must_use]
    pub const fn reused_rust_files(self) -> u64 {
        self.reused_rust_files
    }

    /// Returns files parsed by the current Rust analysis producer.
    #[must_use]
    pub const fn analyzed_rust_files(self) -> u64 {
        self.analyzed_rust_files
    }

    /// Returns Go files restored from exact persisted analysis artifacts.
    #[must_use]
    pub const fn reused_go_files(self) -> u64 {
        self.reused_go_files
    }

    /// Returns Go files parsed by the current analysis producer.
    #[must_use]
    pub const fn analyzed_go_files(self) -> u64 {
        self.analyzed_go_files
    }

    /// Returns TypeScript files restored from exact persisted analysis artifacts.
    #[must_use]
    pub const fn reused_typescript_files(self) -> u64 {
        self.reused_typescript_files
    }

    /// Returns TypeScript files parsed by the current analysis producer.
    #[must_use]
    pub const fn analyzed_typescript_files(self) -> u64 {
        self.analyzed_typescript_files
    }

    /// Returns TSX files restored from exact persisted analysis artifacts.
    #[must_use]
    pub const fn reused_tsx_files(self) -> u64 {
        self.reused_tsx_files
    }

    /// Returns TSX files parsed by the current analysis producer.
    #[must_use]
    pub const fn analyzed_tsx_files(self) -> u64 {
        self.analyzed_tsx_files
    }

    /// Returns Python files restored from exact persisted analysis artifacts.
    #[must_use]
    pub const fn reused_python_files(self) -> u64 {
        self.reused_python_files
    }

    /// Returns Python files parsed by the current analysis producer.
    #[must_use]
    pub const fn analyzed_python_files(self) -> u64 {
        self.analyzed_python_files
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
}

impl fmt::Display for LocalIndexError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::RepositoryIdentity { .. } => "repository identity is invalid",
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
            Self::PublicationActivation { .. } => "local index generation activation failed",
            Self::Checkpoint { .. } => "local index checkpoint failed after activation",
            Self::Shutdown { .. } => "local index writer shutdown failed after activation",
        })
    }
}

impl Error for LocalIndexError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::RepositoryIdentity { source } => Some(source),
            Self::Preparation { source } => Some(source),
            Self::StoreStartup { source }
            | Self::ArtifactReuse { source }
            | Self::WorkspaceRegistration { source }
            | Self::PublicationStaging { source }
            | Self::PublicationActivation { source }
            | Self::Checkpoint { source }
            | Self::Shutdown { source } => Some(source),
            Self::DeadlineNotRepresentable
            | Self::DatabasePathUnavailable
            | Self::DatabaseInsideWorktree
            | Self::DatabaseHasMultipleLinks
            | Self::DatabaseChangedDuringIndexing => None,
        }
    }
}

/// Prepares, stages, validates, and atomically activates one local source index.
///
/// The explicit cancellation flag is shared with preparation and persistence.
/// An existing database may be opened read-only after bounded source capture
/// to load reusable artifacts. A new database is not created until repository
/// identity and preparation have succeeded.
pub fn index_local_rust_repository(
    request: LocalIndexRequest<'_>,
    cancelled: Arc<AtomicBool>,
) -> Result<LocalIndexReport, LocalIndexError> {
    index_local_rust_repository_with_hook(request, cancelled, || {})
}

/// Language-neutral entry point for the local supported-language index.
pub fn index_local_repository(
    request: LocalIndexRequest<'_>,
    cancelled: Arc<AtomicBool>,
) -> Result<LocalIndexReport, LocalIndexError> {
    index_local_rust_repository(request, cancelled)
}

fn index_local_rust_repository_with_hook(
    request: LocalIndexRequest<'_>,
    cancelled: Arc<AtomicBool>,
    after_lease: impl FnOnce(),
) -> Result<LocalIndexReport, LocalIndexError> {
    let repository = RepositoryIdentityTextV1::decode(request.repository_identity)
        .map_err(|source| LocalIndexError::RepositoryIdentity { source })?;
    let deadline = Instant::now()
        .checked_add(request.limits.deadline())
        .ok_or(LocalIndexError::DeadlineNotRepresentable)?;
    if cancelled.load(Ordering::Relaxed) {
        return Err(LocalIndexError::Preparation {
            source: LocalRustIndexError::Cancelled,
        });
    }
    let worktree = discovered_worktree_root(request.repository_root).map_err(|source| {
        LocalIndexError::Preparation {
            source: LocalRustIndexError::Discovery { source },
        }
    })?;
    let database = validated_database_outside_worktree(&worktree, request.database)?;
    let mutation_lease = SqliteMutationLease::acquire(&database, deadline)
        .map_err(|source| LocalIndexError::StoreStartup { source })?;
    let database_identity = database_alias_identity(&database)?;
    after_lease();
    let preparation_limits = remaining_preparation_limits(request.limits, deadline)?;
    let artifacts = phase0_local_source_artifact_identities();
    let preparation = prepare_with_artifact_reuse(
        &worktree,
        &database,
        database_identity.as_ref(),
        artifacts,
        preparation_limits,
        &cancelled,
        deadline,
    )?;
    let report_input = ReportInput::from_preparation(&preparation);
    let snapshot_profile = phase0_local_source_snapshot_profile(artifacts);
    let identity = RustSourceSnapshotIdentity::new_supported_languages(
        repository,
        preparation.git_state(),
        preparation.worktree_state(),
        snapshot_profile.configuration,
        snapshot_profile.producer_manifest,
        snapshot_profile.analysis_schema,
        snapshot_profile.canonicalization_version,
    );
    let coverage = RustIndexCoverage::new(
        report_input.indexed_files,
        report_input.skipped_unsupported_paths,
        report_input.syntax_error_nodes,
        0,
    );
    let prepared = preparation.into_prepared();
    let confirmed_database_identity = database_alias_identity(&database)?;
    if confirmed_database_identity != database_identity {
        return Err(LocalIndexError::DatabaseChangedDuringIndexing);
    }
    drop(confirmed_database_identity);

    let (writer, startup) = OwnedSqliteIndex::start_with_lease(
        mutation_lease,
        database_identity,
        request.migration_applied_at_unix_ms,
        Arc::clone(&cancelled),
        deadline,
    )
    .map_err(map_store_startup_error)?;
    writer
        .register_workspace(repository, ONE_SHOT_SOURCE_EPOCH, deadline)
        .map_err(|source| LocalIndexError::WorkspaceRegistration { source })?;
    let publication = publish_rust_index(
        &writer,
        PublishRustIndexRequest::new(
            ONE_SHOT_SOURCE_EPOCH,
            identity,
            prepared,
            coverage,
            Arc::clone(&cancelled),
            deadline,
        ),
    )
    .map_err(|error| match error {
        PublishRustIndexError::Stage(source) => LocalIndexError::PublicationStaging { source },
        PublishRustIndexError::Activate(source) => {
            LocalIndexError::PublicationActivation { source }
        }
    })?;
    writer
        .checkpoint(deadline)
        .map_err(|source| LocalIndexError::Checkpoint { source })?;
    writer
        .shutdown(deadline)
        .map_err(|source| LocalIndexError::Shutdown { source })?;

    Ok(activated_report(
        publication.generation(),
        publication.source_epoch(),
        startup.recovered_generations(),
        report_input,
    ))
}

fn map_store_startup_error(source: SqliteStoreError) -> LocalIndexError {
    match source {
        SqliteStoreError::DatabaseIdentityChanged => LocalIndexError::DatabaseChangedDuringIndexing,
        source => LocalIndexError::StoreStartup { source },
    }
}

include!("local_index/preparation.rs");

#[cfg(test)]
mod tests;
