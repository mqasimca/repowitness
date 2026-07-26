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
    RepositoryIdentityTextV1, RustIndexCoverage, RustSourceSnapshotIdentity,
    phase0_rust_artifact_identity, publish_rust_index,
};
use repowitness_domain::ProducerManifestDigest;
use sha2::{Digest, Sha256};

use crate::{
    GenerationId, LocalRustIndexError, LocalRustIndexLimits, OwnedSqliteIndex, OwnedSqliteReader,
    SqliteStoreError, contained_source::FileIdentity, git_paths::discovered_worktree_root,
    rust_index::prepare_local_rust_index_excluding_identity_with_reuse,
    sqlite::SqliteMutationLease,
};

const ONE_SHOT_SOURCE_EPOCH: u64 = 0;
const LOCAL_PRODUCER_DOMAIN: &[u8] = b"RepoWitness\0phase0-local-rust-producer\0";
const LOCAL_PRODUCER_VERSION: u32 = 1;

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
    skipped_non_rust_paths: u64,
    total_source_bytes: u64,
    total_facts: u64,
    syntax_error_nodes: u64,
    reused_rust_files: u64,
    analyzed_rust_files: u64,
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

    /// Returns discovered paths outside the Phase 0 Rust scope.
    #[must_use]
    pub const fn skipped_non_rust_paths(self) -> u64 {
        self.skipped_non_rust_paths
    }

    /// Returns exact analyzed Rust source bytes.
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
            Self::Preparation { .. } => "local Rust index preparation failed",
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

/// Prepares, stages, validates, and atomically activates one local Rust index.
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
    let artifact = phase0_local_rust_artifact_identity();
    let preparation = prepare_with_artifact_reuse(
        &worktree,
        &database,
        database_identity.as_ref(),
        artifact,
        preparation_limits,
        &cancelled,
        deadline,
    )?;
    let report_input = ReportInput::from_preparation(&preparation);
    let identity = RustSourceSnapshotIdentity::new(
        repository,
        preparation.git_state(),
        preparation.worktree_state(),
        artifact.configuration(),
        artifact.producer_manifest(),
        artifact.schema(),
        artifact.canonicalization_version(),
    );
    let coverage = RustIndexCoverage::new(
        report_input.indexed_rust_files,
        report_input.skipped_non_rust_paths,
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

    Ok(LocalIndexReport {
        generation: publication.generation(),
        source_epoch: publication.source_epoch(),
        recovered_generations: startup.recovered_generations(),
        discovered_paths: report_input.discovered_paths,
        indexed_rust_files: report_input.indexed_rust_files,
        skipped_non_rust_paths: report_input.skipped_non_rust_paths,
        total_source_bytes: report_input.total_source_bytes,
        total_facts: report_input.total_facts,
        syntax_error_nodes: report_input.syntax_error_nodes,
        reused_rust_files: report_input.reused_rust_files,
        analyzed_rust_files: report_input.analyzed_rust_files,
    })
}

fn map_store_startup_error(source: SqliteStoreError) -> LocalIndexError {
    match source {
        SqliteStoreError::DatabaseIdentityChanged => LocalIndexError::DatabaseChangedDuringIndexing,
        source => LocalIndexError::StoreStartup { source },
    }
}

fn prepare_with_artifact_reuse(
    worktree: &Path,
    database: &Path,
    database_identity: Option<&FileIdentity>,
    artifact: repowitness_application::RustArtifactIdentity,
    preparation_limits: LocalRustIndexLimits,
    cancelled: &Arc<AtomicBool>,
    deadline: Instant,
) -> Result<crate::LocalRustIndexPreparation, LocalIndexError> {
    let reuse_reader = if database.is_file() {
        match OwnedSqliteReader::start(database, deadline) {
            Ok(reader) => Some(reader),
            Err(SqliteStoreError::SchemaVersionMismatch) => None,
            Err(source) => return Err(LocalIndexError::ArtifactReuse { source }),
        }
    } else {
        None
    };
    let preparation = prepare_local_rust_index_excluding_identity_with_reuse(
        worktree,
        artifact,
        preparation_limits,
        cancelled.as_ref(),
        database_identity,
        |requested, load_deadline| match &reuse_reader {
            Some(reader) => reader.load_reusable_artifacts(
                requested,
                artifact,
                preparation_limits.preparation(),
                Arc::clone(cancelled),
                load_deadline,
            ),
            None => Ok(Default::default()),
        },
    )
    .map_err(|source| match source {
        LocalRustIndexError::ExcludedFileAlias => LocalIndexError::DatabaseHasMultipleLinks,
        LocalRustIndexError::ArtifactReuse { source } => LocalIndexError::ArtifactReuse { source },
        source => LocalIndexError::Preparation { source },
    })?;
    if let Some(reader) = reuse_reader {
        reader
            .shutdown(deadline)
            .map_err(|source| LocalIndexError::ArtifactReuse { source })?;
    }
    Ok(preparation)
}

fn remaining_preparation_limits(
    limits: LocalRustIndexLimits,
    deadline: Instant,
) -> Result<LocalRustIndexLimits, LocalIndexError> {
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .filter(|duration| !duration.is_zero())
        .ok_or(LocalIndexError::Preparation {
            source: LocalRustIndexError::DeadlineExceeded,
        })?;
    Ok(LocalRustIndexLimits::new(
        remaining,
        limits.discovery(),
        limits.source_read(),
        limits.preparation(),
    ))
}

fn validated_database_outside_worktree(
    worktree: &Path,
    database: &Path,
) -> Result<std::path::PathBuf, LocalIndexError> {
    let database = match fs::symlink_metadata(database) {
        Ok(_) => {
            fs::canonicalize(database).map_err(|_| LocalIndexError::DatabasePathUnavailable)?
        }
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            let parent = match database.parent() {
                Some(parent) if !parent.as_os_str().is_empty() => parent,
                _ => Path::new("."),
            };
            let parent =
                fs::canonicalize(parent).map_err(|_| LocalIndexError::DatabasePathUnavailable)?;
            let file_name = database
                .file_name()
                .ok_or(LocalIndexError::DatabasePathUnavailable)?;
            parent.join(file_name)
        }
        Err(_) => return Err(LocalIndexError::DatabasePathUnavailable),
    };
    if database.starts_with(worktree) {
        return Err(LocalIndexError::DatabaseInsideWorktree);
    }
    Ok(database)
}

#[cfg(unix)]
fn database_alias_identity(database: &Path) -> Result<Option<FileIdentity>, LocalIndexError> {
    use std::os::unix::fs::MetadataExt;

    match fs::metadata(database) {
        Ok(metadata) if !metadata.is_file() => Err(LocalIndexError::DatabasePathUnavailable),
        Ok(metadata) if metadata.nlink() > 1 => Err(LocalIndexError::DatabaseHasMultipleLinks),
        Ok(_) => FileIdentity::from_path(database)
            .map(Some)
            .map_err(|_| LocalIndexError::DatabasePathUnavailable),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(LocalIndexError::DatabasePathUnavailable),
    }
}

#[cfg(windows)]
fn database_alias_identity(database: &Path) -> Result<Option<FileIdentity>, LocalIndexError> {
    match fs::metadata(database) {
        Ok(metadata) if metadata.is_file() => FileIdentity::from_path(database)
            .map(Some)
            .map_err(|_| LocalIndexError::DatabasePathUnavailable),
        Ok(_) => Err(LocalIndexError::DatabasePathUnavailable),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(LocalIndexError::DatabasePathUnavailable),
    }
}

#[cfg(not(any(unix, windows)))]
fn database_alias_identity(_database: &Path) -> Result<Option<FileIdentity>, LocalIndexError> {
    Ok(None)
}

fn phase0_local_rust_artifact_identity() -> repowitness_application::RustArtifactIdentity {
    let base = phase0_rust_artifact_identity();
    let mut hasher = Sha256::new();
    hasher.update(LOCAL_PRODUCER_DOMAIN);
    hasher.update(LOCAL_PRODUCER_VERSION.to_be_bytes());
    hasher.update(base.producer_manifest().as_bytes());
    update_length_prefixed(&mut hasher, include_bytes!("contained_source.rs"));
    update_length_prefixed(&mut hasher, include_bytes!("git_paths.rs"));
    update_length_prefixed(&mut hasher, include_bytes!("rust_index.rs"));
    update_length_prefixed(&mut hasher, include_bytes!("source_state.rs"));
    update_length_prefixed(&mut hasher, include_bytes!("local_index.rs"));
    repowitness_application::RustArtifactIdentity::new(
        ProducerManifestDigest::new(hasher.finalize().into()),
        base.configuration(),
        base.schema(),
        base.canonicalization_version(),
    )
}

fn update_length_prefixed(hasher: &mut Sha256, value: &[u8]) {
    let length = u64::try_from(value.len()).expect("static adapter inputs fit in u64");
    hasher.update(length.to_be_bytes());
    hasher.update(value);
}

struct ReportInput {
    discovered_paths: u64,
    indexed_rust_files: u64,
    skipped_non_rust_paths: u64,
    total_source_bytes: u64,
    total_facts: u64,
    syntax_error_nodes: u64,
    reused_rust_files: u64,
    analyzed_rust_files: u64,
}

impl ReportInput {
    fn from_preparation(preparation: &crate::LocalRustIndexPreparation) -> Self {
        Self {
            discovered_paths: preparation.discovered_paths(),
            indexed_rust_files: preparation.selected_rust_files(),
            skipped_non_rust_paths: preparation.skipped_non_rust_paths(),
            total_source_bytes: preparation.prepared().total_source_bytes(),
            total_facts: preparation.prepared().total_facts(),
            syntax_error_nodes: preparation.prepared().total_syntax_error_nodes(),
            reused_rust_files: preparation.prepared().reused_files(),
            analyzed_rust_files: preparation.prepared().analyzed_files(),
        }
    }
}

#[cfg(test)]
mod tests {
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

    use repowitness_application::{RepositoryIdentityTextV1, phase0_rust_artifact_identity};
    use rusqlite::Connection;

    use crate::{OwnedSqliteReader, SearchLimits};

    use super::{
        LocalIndexError, LocalIndexRequest, index_local_rust_repository,
        index_local_rust_repository_with_hook, phase0_local_rust_artifact_identity,
    };

    const REPOSITORY_ID: &str = concat!(
        "rwi1:h:",
        "A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1A1"
    );
    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    struct TempDirectory(PathBuf);

    impl TempDirectory {
        fn new() -> Self {
            let ordinal = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "repowitness-local-index-{}-{ordinal}",
                std::process::id()
            ));
            fs::create_dir(&path).expect("fixture directory should be created");
            Self(path)
        }

        fn repository(&self) -> PathBuf {
            self.0.join("repository")
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
            .checked_add(Duration::from_secs(10))
            .expect("test deadline should be representable")
    }

    #[test]
    fn local_producer_identity_is_stable_and_extends_the_analysis_profile() {
        let base = phase0_rust_artifact_identity();
        let first = phase0_local_rust_artifact_identity();
        let second = phase0_local_rust_artifact_identity();

        assert_eq!(first, second);
        assert_ne!(first.producer_manifest(), base.producer_manifest());
        assert_eq!(first.configuration(), base.configuration());
        assert_eq!(first.schema(), base.schema());
        assert_eq!(
            first.canonicalization_version(),
            base.canonicalization_version()
        );
    }

    fn fixture_repository(directory: &TempDirectory) -> PathBuf {
        let repository = directory.repository();
        fs::create_dir_all(repository.join("src"))
            .expect("fixture source directory should be created");
        let status = Command::new("git")
            .args(["init", "--quiet"])
            .arg(&repository)
            .status()
            .expect("Git should start");
        assert!(status.success());
        fs::write(
            repository.join("src/lib.rs"),
            "pub struct Widget;\nimpl Widget { pub fn run() {} }\n",
        )
        .expect("Rust fixture should be written");
        fs::write(repository.join("README.md"), "fixture\n")
            .expect("non-Rust fixture should be written");
        let status = Command::new("git")
            .current_dir(&repository)
            .args(["add", "--", "src/lib.rs", "README.md"])
            .status()
            .expect("Git should start");
        assert!(status.success());
        repository
    }

    #[test]
    fn facade_activates_searchable_production_generations_and_reindexes() {
        let directory = TempDirectory::new();
        let repository = fixture_repository(&directory);
        let database = directory.database();
        let request = LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, 0);

        let first = index_local_rust_repository(request, Arc::new(AtomicBool::new(false)))
            .expect("first generation should activate");
        assert_eq!(first.generation().get(), 1);
        assert_eq!(first.source_epoch(), 0);
        assert_eq!(first.recovered_generations(), 0);
        assert_eq!(first.discovered_paths(), 2);
        assert_eq!(first.indexed_rust_files(), 1);
        assert_eq!(first.skipped_non_rust_paths(), 1);
        assert_eq!(first.total_facts(), 2);
        assert_eq!(first.syntax_error_nodes(), 0);
        assert_eq!(first.reused_rust_files(), 0);
        assert_eq!(first.analyzed_rust_files(), 1);

        let reader = OwnedSqliteReader::start(&database, deadline())
            .expect("reader should open the active generation");
        let results = reader
            .search(
                RepositoryIdentityTextV1::decode(REPOSITORY_ID)
                    .expect("fixture identity should decode"),
                "Widget",
                SearchLimits::default(),
                Arc::new(AtomicBool::new(false)),
                deadline(),
            )
            .expect("active facts should be searchable");
        assert_eq!(results.generation(), first.generation());
        assert_eq!(results.hits().len(), 2);
        reader
            .shutdown(deadline())
            .expect("reader should shut down");

        let second = index_local_rust_repository(request, Arc::new(AtomicBool::new(false)))
            .expect("equivalent second generation should activate");
        assert_eq!(second.generation().get(), 2);
        assert_eq!(second.total_facts(), first.total_facts());
        assert_eq!(second.total_source_bytes(), first.total_source_bytes());
        assert_eq!(second.reused_rust_files(), 1);
        assert_eq!(second.analyzed_rust_files(), 0);

        fs::write(
            repository.join("src/lib.rs"),
            "pub struct Changed;\nimpl Changed { pub fn run() {} }\n",
        )
        .expect("changed Rust fixture should be written");
        let third = index_local_rust_repository(request, Arc::new(AtomicBool::new(false)))
            .expect("changed third generation should activate");
        assert_eq!(third.generation().get(), 3);
        assert_eq!(third.reused_rust_files(), 0);
        assert_eq!(third.analyzed_rust_files(), 1);
    }

    #[test]
    fn legacy_null_payload_is_analyzed_once_backfilled_and_then_reused() {
        let directory = TempDirectory::new();
        let repository = fixture_repository(&directory);
        let database = directory.database();
        let request = LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, 0);

        let first = index_local_rust_repository(request, Arc::new(AtomicBool::new(false)))
            .expect("first generation should activate");
        assert_eq!(first.analyzed_rust_files(), 1);

        let connection = Connection::open(&database).expect("fixture database should open");
        connection
            .execute_batch(
                "DROP TRIGGER analysis_artifact_payload_digest_set_once;
                 UPDATE analysis_artifacts SET payload_digest = NULL;
                 CREATE TRIGGER analysis_artifact_payload_digest_set_once
                 BEFORE UPDATE OF payload_digest ON analysis_artifacts
                 WHEN NOT (
                     OLD.payload_digest IS NULL
                     AND NEW.payload_digest IS NOT NULL
                     AND length(NEW.payload_digest) = 32
                 )
                 BEGIN
                     SELECT RAISE(ABORT, 'immutable analysis artifact payload identity');
                 END;",
            )
            .expect("fixture should emulate a pre-integrity artifact");
        drop(connection);

        let backfilled = index_local_rust_repository(request, Arc::new(AtomicBool::new(false)))
            .expect("legacy artifact should be analyzed and backfilled");
        assert_eq!(backfilled.reused_rust_files(), 0);
        assert_eq!(backfilled.analyzed_rust_files(), 1);

        let reused = index_local_rust_repository(request, Arc::new(AtomicBool::new(false)))
            .expect("backfilled artifact should become reusable");
        assert_eq!(reused.reused_rust_files(), 1);
        assert_eq!(reused.analyzed_rust_files(), 0);
    }

    #[test]
    fn mutation_lease_is_acquired_before_source_capture() {
        let directory = TempDirectory::new();
        let repository = fixture_repository(&directory);
        let database = directory.database();
        let request = LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, 0);

        let report = index_local_rust_repository_with_hook(
            request,
            Arc::new(AtomicBool::new(false)),
            || {
                fs::write(repository.join("src/lib.rs"), "pub struct AfterLease;\n")
                    .expect("source mutation after lease acquisition should succeed");
            },
        )
        .expect("capture after lease acquisition should activate");
        let reader = OwnedSqliteReader::start(&database, deadline())
            .expect("reader should open the active generation");
        let identity = RepositoryIdentityTextV1::decode(REPOSITORY_ID)
            .expect("fixture identity should decode");
        let current = reader
            .search(
                identity,
                "AfterLease",
                SearchLimits::default(),
                Arc::new(AtomicBool::new(false)),
                deadline(),
            )
            .expect("post-lease source should be searchable");
        let stale = reader
            .search(
                identity,
                "Widget",
                SearchLimits::default(),
                Arc::new(AtomicBool::new(false)),
                deadline(),
            )
            .expect("pre-lease source query should complete");

        assert_eq!(current.generation(), report.generation());
        assert!(!current.hits().is_empty());
        assert!(stale.hits().is_empty());
        reader
            .shutdown(deadline())
            .expect("reader should shut down");
    }

    #[test]
    fn validation_and_preparation_failures_do_not_create_a_database() {
        let directory = TempDirectory::new();
        let database = directory.database();
        let invalid_identity = "rwi1:h:PRIVATE";
        let request = LocalIndexRequest::new(
            Path::new("private-repository-that-does-not-exist"),
            &database,
            invalid_identity,
            0,
        );
        let debug = format!("{request:?}");
        assert!(!debug.contains("private-repository"));
        assert!(!debug.contains(invalid_identity));
        assert!(!debug.contains(database.to_string_lossy().as_ref()));
        let error = index_local_rust_repository(request, Arc::new(AtomicBool::new(false)))
            .expect_err("invalid identity should fail first");
        assert!(matches!(error, LocalIndexError::RepositoryIdentity { .. }));
        assert_eq!(error.to_string(), "repository identity is invalid");
        assert!(!format!("{error:?}").contains(invalid_identity));
        assert!(!database.exists());

        let error = index_local_rust_repository(
            LocalIndexRequest::new(
                Path::new("private-repository-that-does-not-exist"),
                &database,
                REPOSITORY_ID,
                0,
            ),
            Arc::new(AtomicBool::new(false)),
        )
        .expect_err("missing repository should fail preparation");
        assert!(matches!(error, LocalIndexError::Preparation { .. }));
        assert!(!error.to_string().contains("private-repository"));
        assert!(!format!("{error:?}").contains("private-repository"));
        assert!(!database.exists());
    }

    #[test]
    fn directory_database_target_is_rejected_before_preparation() {
        let directory = TempDirectory::new();
        let repository = fixture_repository(&directory);

        let error = index_local_rust_repository(
            LocalIndexRequest::new(&repository, &directory.0, REPOSITORY_ID, 0),
            Arc::new(AtomicBool::new(false)),
        )
        .expect_err("a directory cannot serve as the database file");
        assert!(matches!(error, LocalIndexError::DatabasePathUnavailable));
    }

    #[test]
    fn cancellation_is_shared_across_the_complete_facade() {
        let directory = TempDirectory::new();
        let repository = fixture_repository(&directory);
        let database = directory.database();
        let cancelled = Arc::new(AtomicBool::new(true));

        let error = index_local_rust_repository(
            LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, 0),
            cancelled,
        )
        .expect_err("pre-cancelled indexing should fail closed");

        assert!(matches!(error, LocalIndexError::Preparation { .. }));
        assert!(!database.exists());
    }

    #[test]
    fn worktree_local_database_is_rejected_before_it_can_change_source_state() {
        let directory = TempDirectory::new();
        let repository = fixture_repository(&directory);
        let database = repository.join("private-index.sqlite3");

        let error = index_local_rust_repository(
            LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, 0),
            Arc::new(AtomicBool::new(false)),
        )
        .expect_err("worktree-local database should fail closed");

        assert!(matches!(error, LocalIndexError::DatabaseInsideWorktree));
        assert_eq!(
            error.to_string(),
            "local index database must be outside the repository worktree"
        );
        assert!(!database.exists());
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn hard_linked_database_cannot_alias_a_repository_source() {
        let directory = TempDirectory::new();
        let repository = fixture_repository(&directory);
        let database = directory.database();
        let request = LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, 0);
        let first = index_local_rust_repository(request, Arc::new(AtomicBool::new(false)))
            .expect("initial database should activate");
        fs::hard_link(&database, repository.join("index.rs"))
            .expect("fixture database hard link should be created");

        let error = index_local_rust_repository(request, Arc::new(AtomicBool::new(false)))
            .expect_err("a database alias inside the worktree must be rejected");
        assert!(matches!(
            error,
            LocalIndexError::DatabaseHasMultipleLinks
                | LocalIndexError::DatabaseChangedDuringIndexing
        ));

        let reader = OwnedSqliteReader::start(&database, deadline())
            .expect("the previous active generation should remain readable");
        let results = reader
            .search(
                RepositoryIdentityTextV1::decode(REPOSITORY_ID)
                    .expect("fixture identity should decode"),
                "Widget",
                SearchLimits::default(),
                Arc::new(AtomicBool::new(false)),
                deadline(),
            )
            .expect("the previous generation should remain searchable");
        assert_eq!(results.generation(), first.generation());
        reader
            .shutdown(deadline())
            .expect("reader should shut down");
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn database_alias_created_after_lease_cannot_modify_a_repository_source() {
        let directory = TempDirectory::new();
        let repository = fixture_repository(&directory);
        let seed_database = directory.database();
        index_local_rust_repository(
            LocalIndexRequest::new(&repository, &seed_database, REPOSITORY_ID, 0),
            Arc::new(AtomicBool::new(false)),
        )
        .expect("seed database should activate");
        let source = repository.join("database-image.rs");
        fs::copy(&seed_database, &source)
            .expect("valid SQLite image should be copied into a repository source");
        let original_source = fs::read(&source).expect("source image should be readable");
        let database = directory.0.join("late-database.sqlite3");

        let error = index_local_rust_repository_with_hook(
            LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, 0),
            Arc::new(AtomicBool::new(false)),
            || {
                fs::hard_link(&source, &database)
                    .expect("database alias should be created after the initial identity check");
            },
        )
        .expect_err("a late database alias must fail before SQLite can modify the source");

        assert!(matches!(
            error,
            LocalIndexError::DatabaseHasMultipleLinks
                | LocalIndexError::DatabaseChangedDuringIndexing
        ));
        assert_eq!(
            fs::read(&source).expect("source should remain readable"),
            original_source,
            "failed indexing must leave the aliased source unchanged"
        );
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn database_replacement_after_lease_is_rejected_before_writes() {
        let directory = TempDirectory::new();
        let repository = fixture_repository(&directory);
        let database = directory.database();
        let request = LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, 0);
        index_local_rust_repository(request, Arc::new(AtomicBool::new(false)))
            .expect("initial database should activate");
        let replacement = directory.0.join("replacement.sqlite3");
        fs::copy(&database, &replacement).expect("replacement database should be copied");
        let replacement_bytes =
            fs::read(&replacement).expect("replacement database should be readable");
        let displaced = directory.0.join("displaced.sqlite3");

        let error = index_local_rust_repository_with_hook(
            request,
            Arc::new(AtomicBool::new(false)),
            || {
                fs::rename(&database, &displaced)
                    .expect("original database should be displaced after identity capture");
                fs::rename(&replacement, &database)
                    .expect("replacement should occupy the database path");
            },
        )
        .expect_err("a replaced database must fail before the writer starts");

        assert!(matches!(
            error,
            LocalIndexError::DatabaseChangedDuringIndexing
        ));
        assert_eq!(
            fs::read(&database).expect("replacement database should remain readable"),
            replacement_bytes,
            "failed indexing must not write through the replacement path"
        );
    }

    #[cfg(unix)]
    #[test]
    fn dangling_database_symlink_cannot_bypass_worktree_isolation() {
        use std::os::unix::fs::symlink;

        let directory = TempDirectory::new();
        let repository = fixture_repository(&directory);
        let target = repository.join("private-index.sqlite3");
        let database = directory.0.join("database-link");
        symlink(&target, &database).expect("fixture symlink should be created");

        let error = index_local_rust_repository(
            LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, 0),
            Arc::new(AtomicBool::new(false)),
        )
        .expect_err("dangling database symlink should fail closed");

        assert!(matches!(error, LocalIndexError::DatabasePathUnavailable));
        assert!(!target.exists());
    }
}
