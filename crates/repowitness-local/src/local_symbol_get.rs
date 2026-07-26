//! One-shot local composition for exact active-generation symbol retrieval.

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
    CodeSearchPortOutputError, RepositoryIdentityTextError, RepositoryIdentityTextV1,
    RepositoryPathTextByteLimit, RepositoryPathTextError, RepositoryPathTextV1, SymbolGetCandidate,
    SymbolGetError, SymbolGetLimits, SymbolGetPort, SymbolGetPortRequest, SymbolGetPortResult,
    SymbolGetRequest, SymbolGetResult, SymbolGetSelector, hash_source_content, symbol_get,
};
use repowitness_domain::{
    AnalysisArtifactDigest, RepositoryPathLimits, SourceContentDigest, SourceSnapshotDigest,
};

use crate::{
    ContainedSourceError, ContainedSourceRoot, DEFAULT_SOURCE_FILE_BYTES,
    DEFAULT_SOURCE_READ_CHUNK_BYTES, GenerationId, OwnedSqliteReader, SourceReadLimitError,
    SourceReadLimits, SqliteStoreError,
};

/// Default end-to-end deadline for one exact local symbol lookup.
pub const DEFAULT_LOCAL_SYMBOL_GET_DEADLINE: Duration = Duration::from_secs(5);

const SHA256_TEXT_BYTES: usize = 64;
const PERSISTED_PATH_BYTES: u64 = 1_048_576;
const PERSISTED_PATH_COMPONENTS: u64 = 1_048_576;
const PERSISTED_PATH_TEXT_BYTES: u64 = 7 + (PERSISTED_PATH_BYTES * 2);
const PERSISTED_PATH_LIMITS: RepositoryPathLimits =
    RepositoryPathLimits::new(PERSISTED_PATH_BYTES, PERSISTED_PATH_COMPONENTS);

/// Proof-carrying local exact-symbol result pinned to one SQLite generation.
pub type LocalSymbolGetResult = SymbolGetResult<GenerationId>;

/// Text-boundary selector copied from one `code_search` result.
#[derive(Clone, Copy)]
pub struct LocalSymbolSelectorText<'a> {
    snapshot_sha256: &'a str,
    generation: i64,
    path: &'a str,
    content_sha256: &'a str,
    artifact_sha256: &'a str,
    fact_ordinal: u64,
}

impl<'a> LocalSymbolSelectorText<'a> {
    /// Constructs one exact untrusted boundary selector.
    #[must_use]
    pub const fn new(
        snapshot_sha256: &'a str,
        generation: i64,
        path: &'a str,
        content_sha256: &'a str,
        artifact_sha256: &'a str,
        fact_ordinal: u64,
    ) -> Self {
        Self {
            snapshot_sha256,
            generation,
            path,
            content_sha256,
            artifact_sha256,
            fact_ordinal,
        }
    }
}

impl fmt::Debug for LocalSymbolSelectorText<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalSymbolSelectorText")
            .field("snapshot_sha256", &"<redacted-digest>")
            .field("generation", &self.generation)
            .field("path", &"<redacted-path>")
            .field("content_sha256", &"<redacted-digest>")
            .field("artifact_sha256", &"<redacted-digest>")
            .field("fact_ordinal", &self.fact_ordinal)
            .finish()
    }
}

/// Explicit inputs for one local exact-symbol lookup.
#[derive(Clone, Copy)]
pub struct LocalSymbolGetRequest<'a> {
    root: &'a Path,
    database: &'a Path,
    repository_identity: &'a str,
    selector: LocalSymbolSelectorText<'a>,
    limits: SymbolGetLimits,
    deadline: Duration,
}

impl<'a> LocalSymbolGetRequest<'a> {
    /// Constructs a request with the complete bounded Phase 0 profile.
    #[must_use]
    pub fn new(
        root: &'a Path,
        database: &'a Path,
        repository_identity: &'a str,
        selector: LocalSymbolSelectorText<'a>,
    ) -> Self {
        Self {
            root,
            database,
            repository_identity,
            selector,
            limits: SymbolGetLimits::default(),
            deadline: DEFAULT_LOCAL_SYMBOL_GET_DEADLINE,
        }
    }

    /// Applies an explicit end-to-end deadline duration.
    #[must_use]
    pub const fn with_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = deadline;
        self
    }
}

impl fmt::Debug for LocalSymbolGetRequest<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalSymbolGetRequest")
            .field("root", &"<redacted-path>")
            .field("database", &"<redacted-path>")
            .field("repository_identity", &"<redacted-identity>")
            .field("selector", &self.selector)
            .field("limits", &self.limits)
            .field("deadline", &self.deadline)
            .finish()
    }
}

/// Stable lowercase SHA-256 boundary-text failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Sha256TextError;

impl fmt::Display for Sha256TextError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SHA-256 text must contain exactly 64 lowercase hexadecimal digits")
    }
}

impl Error for Sha256TextError {}

/// Stable local adapter failure while retrieving verified declaration bytes.
#[derive(Debug)]
pub enum LocalSymbolPortError {
    /// The SQLite owner could not complete the exact lookup.
    Database(SqliteStoreError),
    /// Source-read bounds could not be represented.
    SourceLimits(SourceReadLimitError),
    /// The capability-contained source read failed.
    Source(ContainedSourceError),
    /// The current source bytes no longer match the indexed content identity.
    StaleSource,
    /// Persisted declaration offsets do not fit within the verified source bytes.
    InvalidSourceSpan,
    /// Persisted occurrence data violated the application occurrence contract.
    InvalidOccurrence(CodeSearchPortOutputError),
    /// The request deadline elapsed before a complete declaration was available.
    DeadlineExceeded,
    /// The request was cancelled before a complete declaration was available.
    Cancelled,
}

impl fmt::Display for LocalSymbolPortError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Database(_) => "exact symbol database lookup failed",
            Self::SourceLimits(_) => "exact symbol source-read limits are invalid",
            Self::Source(_) => "exact symbol source read failed",
            Self::StaleSource => "current source does not match the indexed occurrence",
            Self::InvalidSourceSpan => "indexed symbol source span is invalid",
            Self::InvalidOccurrence(_) => "indexed symbol occurrence is invalid",
            Self::DeadlineExceeded => "exact symbol lookup deadline exceeded",
            Self::Cancelled => "exact symbol lookup was cancelled",
        })
    }
}

impl Error for LocalSymbolPortError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Database(source) => Some(source),
            Self::SourceLimits(source) => Some(source),
            Self::Source(source) => Some(source),
            Self::InvalidOccurrence(source) => Some(source),
            Self::StaleSource
            | Self::InvalidSourceSpan
            | Self::DeadlineExceeded
            | Self::Cancelled => None,
        }
    }
}

/// Stable one-shot local exact-symbol failure.
#[derive(Debug)]
pub enum LocalSymbolGetError {
    /// The repository identity text is malformed or non-canonical.
    RepositoryIdentity(RepositoryIdentityTextError),
    /// The snapshot digest text is malformed or non-canonical.
    Snapshot(Sha256TextError),
    /// The generation is not a positive SQLite generation identity.
    Generation,
    /// The repository-path text is malformed or non-canonical.
    Path(RepositoryPathTextError),
    /// The content digest text is malformed or non-canonical.
    Content(Sha256TextError),
    /// The artifact digest text is malformed or non-canonical.
    Artifact(Sha256TextError),
    /// The absolute deadline cannot be represented.
    DeadlineNotRepresentable,
    /// The capability-contained repository root could not open.
    RootOpen(ContainedSourceError),
    /// The owned read connection could not start.
    ReaderStart(SqliteStoreError),
    /// The shared exact-symbol application use case failed.
    Get(SymbolGetError<LocalSymbolPortError>),
    /// The owned read connection did not shut down cleanly.
    Shutdown(SqliteStoreError),
}

impl fmt::Display for LocalSymbolGetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::RepositoryIdentity(_) => "repository identity is invalid",
            Self::Snapshot(_) => "snapshot digest is invalid",
            Self::Generation => "generation identity is invalid",
            Self::Path(_) => "repository path is invalid",
            Self::Content(_) => "content digest is invalid",
            Self::Artifact(_) => "artifact digest is invalid",
            Self::DeadlineNotRepresentable => "symbol-get deadline cannot be represented",
            Self::RootOpen(_) => "repository source root could not open",
            Self::ReaderStart(_) => "local index reader could not start",
            Self::Get(_) => "local symbol get failed",
            Self::Shutdown(_) => "local index reader could not shut down",
        })
    }
}

impl Error for LocalSymbolGetError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::RepositoryIdentity(source) => Some(source),
            Self::Snapshot(source) => Some(source),
            Self::Path(source) => Some(source),
            Self::Content(source) => Some(source),
            Self::Artifact(source) => Some(source),
            Self::RootOpen(source) => Some(source),
            Self::ReaderStart(source) => Some(source),
            Self::Get(source) => Some(source),
            Self::Shutdown(source) => Some(source),
            Self::Generation | Self::DeadlineNotRepresentable => None,
        }
    }
}

struct LocalSymbolPort<'a> {
    reader: &'a OwnedSqliteReader,
    root: &'a ContainedSourceRoot,
}

impl SymbolGetPort for LocalSymbolPort<'_> {
    type Generation = GenerationId;
    type Error = LocalSymbolPortError;

    fn get(
        &self,
        request: SymbolGetPortRequest<Self::Generation>,
    ) -> Result<SymbolGetPortResult<Self::Generation>, Self::Error> {
        check_control(&request.cancelled(), request.deadline())?;
        let selector = request.selector().clone();
        let results = self
            .reader
            .get_symbol(
                request.repository(),
                request.expected_snapshot(),
                *request.expected_generation(),
                selector,
                request.cancelled(),
                request.deadline(),
            )
            .map_err(LocalSymbolPortError::Database)?;
        let (snapshot, generation, producer_manifest, index_coverage, hit) = results.into_parts();
        let candidate = hit
            .map(|hit| verified_candidate(self.root, hit, request))
            .transpose()?;
        Ok(SymbolGetPortResult::new(
            snapshot,
            generation,
            producer_manifest,
            index_coverage,
            candidate,
        ))
    }
}

fn verified_candidate(
    root: &ContainedSourceRoot,
    hit: crate::SearchHit,
    request: SymbolGetPortRequest<GenerationId>,
) -> Result<SymbolGetCandidate, LocalSymbolPortError> {
    check_control(&request.cancelled(), request.deadline())?;
    if hit.declaration_span().len().get() > request.limits().max_declaration_bytes() {
        return Err(LocalSymbolPortError::InvalidSourceSpan);
    }
    let read_limits = SourceReadLimits::try_new(
        remaining(request.deadline())?,
        DEFAULT_SOURCE_FILE_BYTES,
        DEFAULT_SOURCE_READ_CHUNK_BYTES,
    )
    .map_err(LocalSymbolPortError::SourceLimits)?;
    let cancelled = request.cancelled();
    let source = root
        .read_with_cancel(hit.path(), read_limits, || {
            cancelled.load(Ordering::Acquire)
        })
        .map_err(map_source_error)?;
    check_control(&cancelled, request.deadline())?;
    if hash_source_content(&source) != hit.content_digest() {
        return Err(LocalSymbolPortError::StaleSource);
    }
    let declaration = declaration_bytes(&source, hit.declaration_span())?;
    let occurrence = repowitness_application::RustSymbolOccurrence::try_new(
        hit.fact_ordinal(),
        hit.artifact_digest(),
        hit.kind(),
        hit.name().to_owned(),
        hit.qualified_name().to_owned(),
        hit.name_span(),
        hit.declaration_span(),
    )
    .map_err(LocalSymbolPortError::InvalidOccurrence)?;
    Ok(SymbolGetCandidate::new(
        hit.path().clone(),
        hit.content_digest(),
        occurrence,
        declaration,
    ))
}

fn declaration_bytes(
    source: &[u8],
    span: repowitness_domain::ByteSpan,
) -> Result<Box<[u8]>, LocalSymbolPortError> {
    let start =
        usize::try_from(span.start().get()).map_err(|_| LocalSymbolPortError::InvalidSourceSpan)?;
    let end =
        usize::try_from(span.end().get()).map_err(|_| LocalSymbolPortError::InvalidSourceSpan)?;
    source
        .get(start..end)
        .map(<[u8]>::to_vec)
        .map(Vec::into_boxed_slice)
        .ok_or(LocalSymbolPortError::InvalidSourceSpan)
}

fn remaining(deadline: Instant) -> Result<Duration, LocalSymbolPortError> {
    deadline
        .checked_duration_since(Instant::now())
        .ok_or(LocalSymbolPortError::DeadlineExceeded)
}

fn check_control(cancelled: &AtomicBool, deadline: Instant) -> Result<(), LocalSymbolPortError> {
    if cancelled.load(Ordering::Acquire) {
        Err(LocalSymbolPortError::Cancelled)
    } else if Instant::now() >= deadline {
        Err(LocalSymbolPortError::DeadlineExceeded)
    } else {
        Ok(())
    }
}

fn map_source_error(error: ContainedSourceError) -> LocalSymbolPortError {
    match error {
        ContainedSourceError::Cancelled => LocalSymbolPortError::Cancelled,
        ContainedSourceError::DeadlineExceeded { .. } => LocalSymbolPortError::DeadlineExceeded,
        error => LocalSymbolPortError::Source(error),
    }
}

/// Opens the exact local context, retrieves a verified declaration, and shuts down.
pub fn get_local_rust_symbol(
    request: LocalSymbolGetRequest<'_>,
    cancelled: Arc<AtomicBool>,
) -> Result<LocalSymbolGetResult, LocalSymbolGetError> {
    let repository = RepositoryIdentityTextV1::decode(request.repository_identity)
        .map_err(LocalSymbolGetError::RepositoryIdentity)?;
    let snapshot = SourceSnapshotDigest::new(
        decode_sha256(request.selector.snapshot_sha256).map_err(LocalSymbolGetError::Snapshot)?,
    );
    if request.selector.generation <= 0 {
        return Err(LocalSymbolGetError::Generation);
    }
    let generation = GenerationId::from_database(request.selector.generation);
    let path = RepositoryPathTextV1::decode(
        request.selector.path,
        RepositoryPathTextByteLimit::new(PERSISTED_PATH_TEXT_BYTES),
        PERSISTED_PATH_LIMITS,
    )
    .map_err(LocalSymbolGetError::Path)?;
    let content = SourceContentDigest::new(
        decode_sha256(request.selector.content_sha256).map_err(LocalSymbolGetError::Content)?,
    );
    let artifact = AnalysisArtifactDigest::new(
        decode_sha256(request.selector.artifact_sha256).map_err(LocalSymbolGetError::Artifact)?,
    );
    let selector = SymbolGetSelector::new(path, content, artifact, request.selector.fact_ordinal);
    let deadline = Instant::now()
        .checked_add(request.deadline)
        .ok_or(LocalSymbolGetError::DeadlineNotRepresentable)?;
    check_facade_control(&cancelled, deadline)?;
    let root = ContainedSourceRoot::open(request.root).map_err(LocalSymbolGetError::RootOpen)?;
    let reader = OwnedSqliteReader::start(request.database, deadline)
        .map_err(LocalSymbolGetError::ReaderStart)?;
    let port = LocalSymbolPort {
        reader: &reader,
        root: &root,
    };
    let result = symbol_get(
        &port,
        SymbolGetRequest::new(
            repository,
            snapshot,
            generation,
            selector,
            request.limits,
            cancelled,
            deadline,
        ),
    );
    let shutdown = reader.shutdown(deadline);
    match (result, shutdown) {
        (Ok(result), Ok(())) => Ok(result),
        (Err(source), _) => Err(LocalSymbolGetError::Get(source)),
        (Ok(_), Err(source)) => Err(LocalSymbolGetError::Shutdown(source)),
    }
}

fn check_facade_control(
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), LocalSymbolGetError> {
    if cancelled.load(Ordering::Acquire) {
        Err(LocalSymbolGetError::Get(SymbolGetError::Cancelled))
    } else if Instant::now() >= deadline {
        Err(LocalSymbolGetError::Get(SymbolGetError::DeadlineExceeded))
    } else {
        Ok(())
    }
}

fn decode_sha256(text: &str) -> Result<[u8; 32], Sha256TextError> {
    if text.len() != SHA256_TEXT_BYTES {
        return Err(Sha256TextError);
    }
    let mut output = [0_u8; 32];
    for (index, pair) in text.as_bytes().chunks_exact(2).enumerate() {
        let high = lowercase_hex_nibble(pair[0]).ok_or(Sha256TextError)?;
        let low = lowercase_hex_nibble(pair[1]).ok_or(Sha256TextError)?;
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

#[cfg(test)]
mod tests {
    use std::{
        path::Path,
        sync::{Arc, atomic::AtomicBool},
        time::Duration,
    };

    use super::{
        LocalSymbolGetError, LocalSymbolGetRequest, LocalSymbolSelectorText, Sha256TextError,
        decode_sha256, get_local_rust_symbol,
    };

    const REPOSITORY_ID: &str = concat!(
        "rwi1:h:",
        "0101010101010101010101010101010101010101010101010101010101010101"
    );
    const DIGEST: &str = "0202020202020202020202020202020202020202020202020202020202020202";
    const PATH: &str = "rwp1:h:7372632F6C69622E7273";

    fn selector<'a>(snapshot: &'a str, path: &'a str) -> LocalSymbolSelectorText<'a> {
        LocalSymbolSelectorText::new(snapshot, 1, path, DIGEST, DIGEST, 0)
    }

    #[test]
    fn lowercase_sha256_text_is_exact_and_canonical() {
        assert_eq!(decode_sha256(DIGEST), Ok([2; 32]));
        let letters = "ab".repeat(32);
        assert_eq!(decode_sha256(&letters), Ok([0xab; 32]));
        assert_eq!(decode_sha256(&letters.to_uppercase()), Err(Sha256TextError));
        assert_eq!(decode_sha256(&DIGEST[..63]), Err(Sha256TextError));
        assert_eq!(
            decode_sha256("g202020202020202020202020202020202020202020202020202020202020202"),
            Err(Sha256TextError)
        );
    }

    #[test]
    fn request_debug_is_redacted_and_deadline_is_explicit() {
        let request = LocalSymbolGetRequest::new(
            Path::new("/private/root"),
            Path::new("/private/index.sqlite3"),
            REPOSITORY_ID,
            selector(DIGEST, PATH),
        )
        .with_deadline(Duration::from_secs(1));
        let debug = format!("{request:?}");
        assert!(!debug.contains("/private"));
        assert!(!debug.contains(REPOSITORY_ID));
        assert!(!debug.contains(DIGEST));
        assert!(!debug.contains(PATH));
    }

    #[test]
    fn invalid_boundary_values_fail_before_repository_or_database_io() {
        let missing = Path::new("/missing/private-input");
        let cancelled = Arc::new(AtomicBool::new(false));
        let invalid_identity = get_local_rust_symbol(
            LocalSymbolGetRequest::new(
                missing,
                missing,
                "private-invalid-identity",
                selector(DIGEST, PATH),
            ),
            Arc::clone(&cancelled),
        )
        .expect_err("invalid identity should fail");
        assert!(matches!(
            invalid_identity,
            LocalSymbolGetError::RepositoryIdentity(_)
        ));

        let invalid_snapshot = get_local_rust_symbol(
            LocalSymbolGetRequest::new(missing, missing, REPOSITORY_ID, selector("invalid", PATH)),
            Arc::clone(&cancelled),
        )
        .expect_err("invalid snapshot should fail");
        assert!(matches!(invalid_snapshot, LocalSymbolGetError::Snapshot(_)));

        let invalid_path = get_local_rust_symbol(
            LocalSymbolGetRequest::new(
                missing,
                missing,
                REPOSITORY_ID,
                selector(DIGEST, "private-invalid-path"),
            ),
            cancelled,
        )
        .expect_err("invalid path should fail");
        assert!(matches!(invalid_path, LocalSymbolGetError::Path(_)));
    }

    #[test]
    fn cancellation_and_zero_deadline_stop_before_repository_or_database_io() {
        let missing = Path::new("/missing/private-input");
        let request =
            || LocalSymbolGetRequest::new(missing, missing, REPOSITORY_ID, selector(DIGEST, PATH));
        let cancelled = get_local_rust_symbol(request(), Arc::new(AtomicBool::new(true)))
            .expect_err("pre-cancelled lookup should stop");
        assert!(matches!(
            cancelled,
            LocalSymbolGetError::Get(repowitness_application::SymbolGetError::Cancelled)
        ));

        let expired = get_local_rust_symbol(
            request().with_deadline(Duration::ZERO),
            Arc::new(AtomicBool::new(false)),
        )
        .expect_err("zero-deadline lookup should stop");
        assert!(matches!(
            expired,
            LocalSymbolGetError::Get(repowitness_application::SymbolGetError::DeadlineExceeded)
        ));
    }
}
