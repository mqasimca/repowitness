//! One-shot local composition for exact source-span to opaque SCIP symbol navigation.

use std::{
    error::Error,
    fmt,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use repowitness_application::{
    ByteOffset, ByteSpan, RepositoryPathLimits, RepositoryPathTextByteLimit,
    RepositoryPathTextError, RepositoryPathTextV1, ScipEvidenceReadSelection,
    ScipSymbolResolveError as ApplicationScipSymbolResolveError, ScipSymbolResolvePort,
    ScipSymbolResolvePortResult, ScipSymbolResolveRequest, ScipSymbolResolveResult,
    SymbolGetSelector, scip_symbol_resolve,
};
use repowitness_domain::{
    AnalysisArtifactDigest, RepositoryPath, SourceContentDigest, SourceSnapshotDigest,
};

use crate::local_scip_evidence_read::scip_evidence_selection;
use crate::{
    DEFAULT_LOCAL_SCIP_EVIDENCE_READ_DEADLINE, GenerationId, LocalScipEvidenceReadError,
    LocalScipEvidenceWorkspace, OwnedSqliteReader, ScipSyntaxSymbolResolution, SqliteStoreError,
};

const SHA256_TEXT_BYTES: usize = 64;
const PERSISTED_PATH_BYTES: u64 = 1_048_576;
const PERSISTED_PATH_COMPONENTS: u64 = 1_048_576;
const PERSISTED_PATH_TEXT_BYTES: u64 = 7 + (PERSISTED_PATH_BYTES * 2);
const PERSISTED_PATH_LIMITS: RepositoryPathLimits =
    RepositoryPathLimits::new(PERSISTED_PATH_BYTES, PERSISTED_PATH_COMPONENTS);

/// Validated local exact-span SCIP resolution result.
pub type LocalScipSymbolResolveResult = ScipSymbolResolveResult<ScipSyntaxSymbolResolution>;

/// Untrusted exact declaration-selector text copied from one search candidate.
#[derive(Clone, Copy)]
pub struct LocalScipSymbolResolveSelectorText<'a> {
    snapshot_sha256: &'a str,
    generation: i64,
    path: &'a str,
    content_sha256: &'a str,
    artifact_sha256: &'a str,
    fact_ordinal: u64,
    name_span: (u64, u64),
}

impl<'a> LocalScipSymbolResolveSelectorText<'a> {
    /// Constructs one complete immutable declaration selector.
    #[must_use]
    pub const fn new(
        snapshot_sha256: &'a str,
        generation: i64,
        path: &'a str,
        content_sha256: &'a str,
        artifact_sha256: &'a str,
        fact_ordinal: u64,
        name_span: (u64, u64),
    ) -> Self {
        Self {
            snapshot_sha256,
            generation,
            path,
            content_sha256,
            artifact_sha256,
            fact_ordinal,
            name_span,
        }
    }
}

/// Explicit local input for an exact identifier span from an indexed declaration receipt.
pub struct LocalScipSymbolResolveRequest<'a> {
    database: &'a Path,
    workspace: LocalScipEvidenceWorkspace<'a>,
    exact_view: Option<i64>,
    selector: LocalScipSymbolResolveSelectorText<'a>,
    deadline: Duration,
}

impl<'a> LocalScipSymbolResolveRequest<'a> {
    /// Constructs an active-view request for one default repository workspace.
    #[must_use]
    pub const fn new(
        database: &'a Path,
        repository_identity: &'a str,
        selector: LocalScipSymbolResolveSelectorText<'a>,
    ) -> Self {
        Self {
            database,
            workspace: LocalScipEvidenceWorkspace::SingleRepository {
                repository_identity,
            },
            exact_view: None,
            selector,
            deadline: DEFAULT_LOCAL_SCIP_EVIDENCE_READ_DEADLINE,
        }
    }

    /// Constructs an active-view request for one explicit connected source slot.
    #[must_use]
    pub const fn for_connected_workspace(
        database: &'a Path,
        connected_workspace: &'a str,
        source_slot: &'a str,
        selector: LocalScipSymbolResolveSelectorText<'a>,
    ) -> Self {
        Self {
            database,
            workspace: LocalScipEvidenceWorkspace::ConnectedWorkspace {
                connected_workspace,
                source_slot,
            },
            exact_view: None,
            selector,
            deadline: DEFAULT_LOCAL_SCIP_EVIDENCE_READ_DEADLINE,
        }
    }

    /// Pins one exact immutable workspace view.
    pub fn with_exact_view(
        mut self,
        workspace_view: i64,
    ) -> Result<Self, LocalScipSymbolResolveError> {
        if workspace_view <= 0 {
            return Err(LocalScipSymbolResolveError::InvalidSelection);
        }
        self.exact_view = Some(workspace_view);
        Ok(self)
    }

    /// Replaces the end-to-end deadline duration.
    #[must_use]
    pub const fn with_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = deadline;
        self
    }
}

impl fmt::Debug for LocalScipSymbolResolveRequest<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalScipSymbolResolveRequest")
            .field("database", &"<redacted-path>")
            .field("workspace", &self.workspace)
            .field("exact_view", &self.exact_view)
            .field("snapshot_sha256", &"<redacted-digest>")
            .field("generation", &self.selector.generation)
            .field("path", &"<redacted-path>")
            .field("content_sha256", &"<redacted-digest>")
            .field("artifact_sha256", &"<redacted-digest>")
            .field("fact_ordinal", &self.selector.fact_ordinal)
            .field("name_span", &self.selector.name_span)
            .field("deadline", &"<monotonic>")
            .finish()
    }
}

/// Stable content-redacted local exact-span SCIP resolution failure.
#[derive(Debug)]
pub enum LocalScipSymbolResolveError {
    /// The supplied source path was malformed or non-canonical.
    Path(RepositoryPathTextError),
    /// The supplied source content digest was malformed or non-canonical.
    ContentDigest,
    /// The supplied source snapshot digest was malformed or non-canonical.
    SnapshotDigest,
    /// The supplied analysis artifact digest was malformed or non-canonical.
    ArtifactDigest,
    /// The supplied source generation was invalid.
    Generation,
    /// The identifier span was invalid.
    NameSpan,
    /// The immutable workspace selection was malformed.
    InvalidSelection,
    /// The local workspace selection could not be decoded.
    Workspace(LocalScipEvidenceReadError),
    /// The absolute deadline could not be represented.
    DeadlineNotRepresentable,
    /// The read-only SQLite owner could not start.
    ReaderStart(SqliteStoreError),
    /// The shared application use case failed.
    Resolve(ApplicationScipSymbolResolveError<LocalScipSymbolResolvePortError>),
    /// The read-only SQLite owner did not shut down cleanly.
    Shutdown(SqliteStoreError),
}

impl fmt::Display for LocalScipSymbolResolveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Path(_) => "SCIP symbol resolution path is invalid",
            Self::ContentDigest => "SCIP symbol resolution content digest is invalid",
            Self::SnapshotDigest => "SCIP symbol resolution snapshot digest is invalid",
            Self::ArtifactDigest => "SCIP symbol resolution artifact digest is invalid",
            Self::Generation => "SCIP symbol resolution generation is invalid",
            Self::NameSpan => "SCIP symbol resolution identifier span is invalid",
            Self::InvalidSelection | Self::Workspace(_) => {
                "SCIP symbol resolution immutable context is invalid"
            }
            Self::DeadlineNotRepresentable => {
                "SCIP symbol resolution deadline cannot be represented"
            }
            Self::ReaderStart(_) => "SCIP symbol resolution reader startup failed",
            Self::Resolve(_) => "local SCIP symbol resolution failed",
            Self::Shutdown(_) => "SCIP symbol resolution reader shutdown failed",
        })
    }
}

impl Error for LocalScipSymbolResolveError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Path(error) => Some(error),
            Self::Workspace(error) => Some(error),
            Self::ReaderStart(error) | Self::Shutdown(error) => Some(error),
            Self::Resolve(error) => Some(error),
            Self::ContentDigest
            | Self::SnapshotDigest
            | Self::ArtifactDigest
            | Self::Generation
            | Self::NameSpan
            | Self::InvalidSelection
            | Self::DeadlineNotRepresentable => None,
        }
    }
}

/// Stable local adapter failure behind the exact-span SCIP-resolution port.
#[derive(Debug)]
pub enum LocalScipSymbolResolvePortError {
    /// The selected workspace view or requested source slot is unavailable.
    ViewUnavailable,
    /// Immutable view pinning failed.
    View(SqliteStoreError),
    /// The bounded exact-span overlay reader failed.
    Resolve(SqliteStoreError),
    /// The exact declaration receipt does not belong to the selected source generation.
    ExactDeclarationUnavailable,
}

impl fmt::Display for LocalScipSymbolResolvePortError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ViewUnavailable => "SCIP symbol resolution workspace view is unavailable",
            Self::View(_) => "SCIP symbol resolution workspace view read failed",
            Self::Resolve(_) => "SCIP symbol resolution overlay read failed",
            Self::ExactDeclarationUnavailable => {
                "SCIP symbol resolution exact declaration is unavailable"
            }
        })
    }
}

impl Error for LocalScipSymbolResolvePortError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::View(error) | Self::Resolve(error) => Some(error),
            Self::ViewUnavailable | Self::ExactDeclarationUnavailable => None,
        }
    }
}

struct LocalScipSymbolResolvePort<'a> {
    reader: &'a OwnedSqliteReader,
    expected_snapshot: SourceSnapshotDigest,
    expected_generation: GenerationId,
    selector: SymbolGetSelector,
}

impl ScipSymbolResolvePort for LocalScipSymbolResolvePort<'_> {
    type Output = ScipSyntaxSymbolResolution;
    type Error = LocalScipSymbolResolvePortError;

    fn resolve(
        &self,
        selection: ScipEvidenceReadSelection,
        path: &RepositoryPath,
        content: SourceContentDigest,
        name_span: ByteSpan,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<ScipSymbolResolvePortResult<Self::Output>, Self::Error> {
        let view = self
            .reader
            .pin_workspace_view(
                selection.connected_workspace(),
                selection.workspace_view(),
                Arc::clone(&cancelled),
                deadline,
            )
            .map_err(LocalScipSymbolResolvePortError::View)?
            .ok_or(LocalScipSymbolResolvePortError::ViewUnavailable)?;
        let member = match selection.source_slot() {
            Some(source_slot) => view
                .members()
                .iter()
                .find(|member| member.source_slot() == source_slot)
                .ok_or(LocalScipSymbolResolvePortError::ViewUnavailable)?,
            None => {
                let [member] = view.members() else {
                    return Err(LocalScipSymbolResolvePortError::ViewUnavailable);
                };
                member
            }
        };
        if member.generation() != self.expected_generation {
            return Err(LocalScipSymbolResolvePortError::ExactDeclarationUnavailable);
        }
        let declaration = self
            .reader
            .get_symbol(
                member.repository(),
                self.expected_snapshot,
                self.expected_generation,
                self.selector.clone(),
                Arc::clone(&cancelled),
                deadline,
            )
            .map_err(LocalScipSymbolResolvePortError::Resolve)?;
        if declaration
            .hit()
            .is_none_or(|hit| hit.name_span() != name_span)
        {
            return Err(LocalScipSymbolResolvePortError::ExactDeclarationUnavailable);
        }
        let source_slot = member.source_slot();
        let output = self
            .reader
            .scip_symbol_at_syntax_span(
                &view,
                source_slot,
                path.clone(),
                content,
                name_span,
                cancelled,
                deadline,
            )
            .map_err(LocalScipSymbolResolvePortError::Resolve)?;
        Ok(ScipSymbolResolvePortResult::new(
            view.connected_workspace(),
            view.view().get(),
            source_slot,
            output,
        ))
    }
}

/// Opens one reader, resolves one exact syntax span, and shuts down.
pub fn resolve_local_scip_symbol(
    request: LocalScipSymbolResolveRequest<'_>,
    cancelled: Arc<AtomicBool>,
) -> Result<LocalScipSymbolResolveResult, LocalScipSymbolResolveError> {
    let selection = scip_evidence_selection(&request.workspace, request.exact_view)
        .map_err(LocalScipSymbolResolveError::Workspace)?;
    let snapshot = SourceSnapshotDigest::new(
        decode_sha256(request.selector.snapshot_sha256)
            .map_err(|_| LocalScipSymbolResolveError::SnapshotDigest)?,
    );
    if request.selector.generation <= 0 {
        return Err(LocalScipSymbolResolveError::Generation);
    }
    let generation = GenerationId::from_database(request.selector.generation);
    let path = RepositoryPathTextV1::decode(
        request.selector.path,
        RepositoryPathTextByteLimit::new(PERSISTED_PATH_TEXT_BYTES),
        PERSISTED_PATH_LIMITS,
    )
    .map_err(LocalScipSymbolResolveError::Path)?;
    let content = SourceContentDigest::new(
        decode_sha256(request.selector.content_sha256)
            .map_err(|_| LocalScipSymbolResolveError::ContentDigest)?,
    );
    let artifact = AnalysisArtifactDigest::new(
        decode_sha256(request.selector.artifact_sha256)
            .map_err(|_| LocalScipSymbolResolveError::ArtifactDigest)?,
    );
    let selector = SymbolGetSelector::new(
        path.clone(),
        content,
        artifact,
        request.selector.fact_ordinal,
    );
    let name_span = ByteSpan::try_new(
        ByteOffset::new(request.selector.name_span.0),
        ByteOffset::new(request.selector.name_span.1),
    )
    .map_err(|_| LocalScipSymbolResolveError::NameSpan)?;
    let deadline = Instant::now()
        .checked_add(request.deadline)
        .ok_or(LocalScipSymbolResolveError::DeadlineNotRepresentable)?;
    if cancelled.load(Ordering::Acquire) {
        return Err(LocalScipSymbolResolveError::Resolve(
            ApplicationScipSymbolResolveError::Cancelled,
        ));
    }
    let reader = OwnedSqliteReader::start(request.database, deadline)
        .map_err(LocalScipSymbolResolveError::ReaderStart)?;
    let result = scip_symbol_resolve(
        &LocalScipSymbolResolvePort {
            reader: &reader,
            expected_snapshot: snapshot,
            expected_generation: generation,
            selector,
        },
        ScipSymbolResolveRequest::new(selection, path, content, name_span, cancelled, deadline),
    );
    let shutdown = reader.shutdown(deadline);
    match (result, shutdown) {
        (Ok(result), Ok(())) => Ok(result),
        (Err(error), _) => Err(LocalScipSymbolResolveError::Resolve(error)),
        (Ok(_), Err(error)) => Err(LocalScipSymbolResolveError::Shutdown(error)),
    }
}

fn decode_sha256(text: &str) -> Result<[u8; 32], ()> {
    if text.len() != SHA256_TEXT_BYTES {
        return Err(());
    }
    let mut output = [0_u8; 32];
    for (index, pair) in text.as_bytes().chunks_exact(2).enumerate() {
        let high = lowercase_hex_nibble(pair[0]).ok_or(())?;
        let low = lowercase_hex_nibble(pair[1]).ok_or(())?;
        output[index] = (high << 4) | low;
    }
    Ok(output)
}

const fn lowercase_hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}
