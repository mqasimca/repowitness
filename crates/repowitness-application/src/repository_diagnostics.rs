use std::{
    error::Error,
    fmt,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use repowitness_domain::{ProducerManifestDigest, RepositoryIdentityDigest, SourceSnapshotDigest};

use crate::{
    MemoryEffectiveState, MemoryRecallProjectionCoverage, RustIndexCoverage, SourceLanguage,
};

/// Version of the stable repository-diagnostics contract.
pub const REPOSITORY_DIAGNOSTICS_PROFILE_VERSION: u16 = 3;

const SUPPORTED_LANGUAGES: [SourceLanguage; 5] = [
    SourceLanguage::Rust,
    SourceLanguage::Go,
    SourceLanguage::TypeScript,
    SourceLanguage::Tsx,
    SourceLanguage::Python,
];

const CAPABILITIES: [RepositoryDiagnosticCapability; 5] = [
    RepositoryDiagnosticCapability::LexicalSourceSearch,
    RepositoryDiagnosticCapability::ExactSymbolSource,
    RepositoryDiagnosticCapability::BoundedRustSyntaxGraph,
    RepositoryDiagnosticCapability::CurrentMemoryRecall,
    RepositoryDiagnosticCapability::BoundedContextBuild,
];

const LIMITATIONS: [RepositoryDiagnosticLimitation; 6] = [
    RepositoryDiagnosticLimitation::RustGraphSyntaxDerivedOnly,
    RepositoryDiagnosticLimitation::NoPackageMacroScipDynamicOrCrossLanguageGraph,
    RepositoryDiagnosticLimitation::NoHistorySearch,
    RepositoryDiagnosticLimitation::NoVectorRetrieval,
    RepositoryDiagnosticLimitation::NoModelTokenizer,
    RepositoryDiagnosticLimitation::NoRemoteTransport,
];

/// Implemented evidence capability available to repository read paths.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum RepositoryDiagnosticCapability {
    /// Literal FTS5 retrieval over the active source generation.
    LexicalSourceSearch,
    /// Exact selector expansion with declaration-content verification.
    ExactSymbolSource,
    /// Bounded generation-pinned Rust syntax graph reads with categorical evidence.
    BoundedRustSyntaxGraph,
    /// Retrieval from the complete current-memory projection when present.
    CurrentMemoryRecall,
    /// Deterministic bounded fusion of exact source and current memory.
    BoundedContextBuild,
}

impl RepositoryDiagnosticCapability {
    /// Returns the stable wire spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LexicalSourceSearch => "lexical_source_search",
            Self::ExactSymbolSource => "exact_symbol_source",
            Self::BoundedRustSyntaxGraph => "bounded_rust_syntax_graph",
            Self::CurrentMemoryRecall => "current_memory_recall",
            Self::BoundedContextBuild => "bounded_context_build",
        }
    }
}

/// Explicit functionality that repository read paths do not provide.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum RepositoryDiagnosticLimitation {
    /// Native graph coverage is Rust-only and derived from pinned syntax.
    RustGraphSyntaxDerivedOnly,
    /// Package-aware resolution, macro expansion, SCIP, dynamic dispatch, and
    /// cross-language edges are unavailable or explicitly unresolved.
    NoPackageMacroScipDynamicOrCrossLanguageGraph,
    /// Git-history retrieval is unavailable.
    NoHistorySearch,
    /// Vector retrieval is unavailable.
    NoVectorRetrieval,
    /// Context budgets use bytes rather than a model-specific tokenizer.
    NoModelTokenizer,
    /// Only local CLI and stdio MCP transports are implemented.
    NoRemoteTransport,
}

impl RepositoryDiagnosticLimitation {
    /// Returns the stable wire spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RustGraphSyntaxDerivedOnly => "rust_graph_syntax_derived_only",
            Self::NoPackageMacroScipDynamicOrCrossLanguageGraph => {
                "no_package_macro_scip_dynamic_or_cross_language_graph"
            }
            Self::NoHistorySearch => "no_history_search",
            Self::NoVectorRetrieval => "no_vector_retrieval",
            Self::NoModelTokenizer => "no_model_tokenizer",
            Self::NoRemoteTransport => "no_remote_transport",
        }
    }
}

/// Optional complete memory projection matching the active source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RepositoryDiagnosticsMemoryProjection<P> {
    projection: P,
    source_epoch: u64,
    snapshot: SourceSnapshotDigest,
    coverage: MemoryRecallProjectionCoverage,
}

impl<P> RepositoryDiagnosticsMemoryProjection<P> {
    /// Constructs adapter output for application validation.
    #[must_use]
    pub const fn new(
        projection: P,
        source_epoch: u64,
        snapshot: SourceSnapshotDigest,
        coverage: MemoryRecallProjectionCoverage,
    ) -> Self {
        Self {
            projection,
            source_epoch,
            snapshot,
            coverage,
        }
    }

    /// Returns the immutable projection identity.
    #[must_use]
    pub const fn projection(&self) -> &P {
        &self.projection
    }

    /// Returns the source epoch the projection was built against.
    #[must_use]
    pub const fn source_epoch(&self) -> u64 {
        self.source_epoch
    }

    /// Returns the exact source snapshot the projection was built against.
    #[must_use]
    pub const fn snapshot(&self) -> SourceSnapshotDigest {
        self.snapshot
    }

    /// Returns exact projection coverage and effective-state counts.
    #[must_use]
    pub const fn coverage(&self) -> MemoryRecallProjectionCoverage {
        self.coverage
    }
}

/// Aggregate parser diagnostics for one complete active source generation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RepositoryParserDiagnostics {
    syntax_error_nodes: u64,
    known_parser_limitation_nodes: u64,
}

impl RepositoryParserDiagnostics {
    /// Constructs adapter output for application validation.
    #[must_use]
    pub const fn new(syntax_error_nodes: u64, known_parser_limitation_nodes: u64) -> Self {
        Self {
            syntax_error_nodes,
            known_parser_limitation_nodes,
        }
    }

    /// Returns the raw number of parser syntax-error nodes.
    #[must_use]
    pub const fn syntax_error_nodes(self) -> u64 {
        self.syntax_error_nodes
    }

    /// Returns the non-subtractive subset caused by known parser limitations.
    #[must_use]
    pub const fn known_parser_limitation_nodes(self) -> u64 {
        self.known_parser_limitation_nodes
    }
}

/// Complete adapter result pinned by one read transaction.
pub struct RepositoryDiagnosticsPortResult<G, P> {
    snapshot: SourceSnapshotDigest,
    generation: G,
    source_epoch: u64,
    producer_manifest: ProducerManifestDigest,
    index_coverage: RustIndexCoverage,
    parser_diagnostics: RepositoryParserDiagnostics,
    memory_projection: Option<RepositoryDiagnosticsMemoryProjection<P>>,
}

impl<G, P> RepositoryDiagnosticsPortResult<G, P> {
    /// Constructs adapter output for application validation.
    #[must_use]
    pub const fn new(
        snapshot: SourceSnapshotDigest,
        generation: G,
        source_epoch: u64,
        producer_manifest: ProducerManifestDigest,
        index_coverage: RustIndexCoverage,
        parser_diagnostics: RepositoryParserDiagnostics,
        memory_projection: Option<RepositoryDiagnosticsMemoryProjection<P>>,
    ) -> Self {
        Self {
            snapshot,
            generation,
            source_epoch,
            producer_manifest,
            index_coverage,
            parser_diagnostics,
            memory_projection,
        }
    }
}

/// Narrow read-only diagnostics boundary shared by CLI and MCP.
pub trait RepositoryDiagnosticsPort {
    /// Opaque active index-generation identity owned by the adapter.
    type Generation: Copy + Eq;
    /// Opaque immutable memory-projection identity owned by the adapter.
    type Projection: Copy + Eq;
    /// Stable adapter failure.
    type Error;

    /// Reads one transactionally pinned repository state.
    fn diagnose(
        &self,
        repository: RepositoryIdentityDigest,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<RepositoryDiagnosticsPortResult<Self::Generation, Self::Projection>, Self::Error>;
}

/// Application request shared by local CLI and MCP adapters.
pub struct RepositoryDiagnosticsRequest {
    repository: RepositoryIdentityDigest,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
}

impl RepositoryDiagnosticsRequest {
    /// Constructs a request from validated boundary values.
    #[must_use]
    pub const fn new(
        repository: RepositoryIdentityDigest,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Self {
        Self {
            repository,
            cancelled,
            deadline,
        }
    }
}

impl fmt::Debug for RepositoryDiagnosticsRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RepositoryDiagnosticsRequest")
            .field("repository", &self.repository)
            .field("cancelled", &self.cancelled.load(Ordering::Acquire))
            .field("deadline", &"<monotonic>")
            .finish()
    }
}

/// Validated, transactionally pinned repository diagnostics.
pub struct RepositoryDiagnosticsResult<G, P> {
    snapshot: SourceSnapshotDigest,
    generation: G,
    source_epoch: u64,
    producer_manifest: ProducerManifestDigest,
    index_coverage: RustIndexCoverage,
    parser_diagnostics: RepositoryParserDiagnostics,
    memory_projection: Option<RepositoryDiagnosticsMemoryProjection<P>>,
}

impl<G, P> RepositoryDiagnosticsResult<G, P> {
    /// Returns the exact active source snapshot.
    #[must_use]
    pub const fn snapshot(&self) -> SourceSnapshotDigest {
        self.snapshot
    }

    /// Returns the exact active index generation.
    #[must_use]
    pub const fn generation(&self) -> &G {
        &self.generation
    }

    /// Returns the active workspace source epoch.
    #[must_use]
    pub const fn source_epoch(&self) -> u64 {
        self.source_epoch
    }

    /// Returns producer attribution for the active source snapshot.
    #[must_use]
    pub const fn producer_manifest(&self) -> ProducerManifestDigest {
        self.producer_manifest
    }

    /// Returns complete active-index coverage.
    #[must_use]
    pub const fn index_coverage(&self) -> RustIndexCoverage {
        self.index_coverage
    }

    /// Returns the raw number of parser syntax-error nodes in the active index.
    #[must_use]
    pub const fn syntax_error_nodes(&self) -> u64 {
        self.parser_diagnostics.syntax_error_nodes()
    }

    /// Returns the non-subtractive subset caused by known parser limitations.
    #[must_use]
    pub const fn known_parser_limitation_nodes(&self) -> u64 {
        self.parser_diagnostics.known_parser_limitation_nodes()
    }

    /// Returns the matching complete memory projection when one exists.
    #[must_use]
    pub const fn memory_projection(&self) -> Option<&RepositoryDiagnosticsMemoryProjection<P>> {
        self.memory_projection.as_ref()
    }

    /// Returns supported source languages in stable order.
    #[must_use]
    pub const fn supported_languages(&self) -> &'static [SourceLanguage] {
        &SUPPORTED_LANGUAGES
    }

    /// Returns implemented evidence capabilities in stable order.
    #[must_use]
    pub const fn capabilities(&self) -> &'static [RepositoryDiagnosticCapability] {
        &CAPABILITIES
    }

    /// Returns explicit repository-read limitations in stable order.
    #[must_use]
    pub const fn limitations(&self) -> &'static [RepositoryDiagnosticLimitation] {
        &LIMITATIONS
    }

    /// Returns the diagnostics profile version.
    #[must_use]
    pub const fn profile_version(&self) -> u16 {
        REPOSITORY_DIAGNOSTICS_PROFILE_VERSION
    }
}

impl<G: fmt::Debug, P: fmt::Debug> fmt::Debug for RepositoryDiagnosticsResult<G, P> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RepositoryDiagnosticsResult")
            .field("snapshot", &self.snapshot)
            .field("generation", &self.generation)
            .field("source_epoch", &self.source_epoch)
            .field("producer_manifest", &self.producer_manifest)
            .field("index_coverage", &self.index_coverage)
            .field("parser_diagnostics", &self.parser_diagnostics)
            .field("memory_projection", &self.memory_projection)
            .finish()
    }
}

/// Stable invalid-adapter-output classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RepositoryDiagnosticsPortOutputError {
    /// Known parser limitations are not a subset of raw syntax-error nodes.
    InvalidParserDiagnostics,
    /// The memory projection does not match the active source epoch or snapshot.
    MemorySourceMismatch,
    /// Projection coverage or effective-state counts are inconsistent.
    InvalidMemoryCoverage,
}

impl fmt::Display for RepositoryDiagnosticsPortOutputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidParserDiagnostics => {
                "repository-diagnostics adapter returned invalid parser diagnostics"
            }
            Self::MemorySourceMismatch => {
                "repository-diagnostics adapter returned mixed source and memory state"
            }
            Self::InvalidMemoryCoverage => {
                "repository-diagnostics adapter returned invalid memory coverage"
            }
        })
    }
}

impl Error for RepositoryDiagnosticsPortOutputError {}

/// Application failure for one diagnostics read.
#[derive(Debug)]
pub enum RepositoryDiagnosticsError<E> {
    /// The operation was cancelled before a complete result.
    Cancelled,
    /// The operation deadline elapsed.
    DeadlineExceeded,
    /// The storage-neutral adapter failed.
    Port(E),
    /// The adapter violated the shared result contract.
    InvalidPortOutput(RepositoryDiagnosticsPortOutputError),
}

impl<E: fmt::Display> fmt::Display for RepositoryDiagnosticsError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => formatter.write_str("repository diagnostics cancelled"),
            Self::DeadlineExceeded => {
                formatter.write_str("repository diagnostics deadline exceeded")
            }
            Self::Port(error) => {
                write!(formatter, "repository diagnostics adapter failed: {error}")
            }
            Self::InvalidPortOutput(error) => write!(formatter, "{error}"),
        }
    }
}

impl<E: Error + 'static> Error for RepositoryDiagnosticsError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Port(error) => Some(error),
            Self::InvalidPortOutput(error) => Some(error),
            Self::Cancelled | Self::DeadlineExceeded => None,
        }
    }
}

/// Reads and validates one exact active repository state.
pub type RepositoryDiagnosticsUseCaseResult<G, P, E> =
    Result<RepositoryDiagnosticsResult<G, P>, RepositoryDiagnosticsError<E>>;

/// Reads and validates one exact active repository state.
pub fn repository_diagnostics<Port>(
    port: &Port,
    request: RepositoryDiagnosticsRequest,
) -> RepositoryDiagnosticsUseCaseResult<Port::Generation, Port::Projection, Port::Error>
where
    Port: RepositoryDiagnosticsPort,
{
    check_control(&request.cancelled, request.deadline)?;
    let result = port
        .diagnose(
            request.repository,
            Arc::clone(&request.cancelled),
            request.deadline,
        )
        .map_err(RepositoryDiagnosticsError::Port)?;
    check_control(&request.cancelled, request.deadline)?;
    validate_port_result(&result)?;
    Ok(RepositoryDiagnosticsResult {
        snapshot: result.snapshot,
        generation: result.generation,
        source_epoch: result.source_epoch,
        producer_manifest: result.producer_manifest,
        index_coverage: result.index_coverage,
        parser_diagnostics: result.parser_diagnostics,
        memory_projection: result.memory_projection,
    })
}

fn validate_port_result<G, P, E>(
    result: &RepositoryDiagnosticsPortResult<G, P>,
) -> Result<(), RepositoryDiagnosticsError<E>> {
    if result.parser_diagnostics.known_parser_limitation_nodes()
        > result.parser_diagnostics.syntax_error_nodes()
    {
        return Err(RepositoryDiagnosticsError::InvalidPortOutput(
            RepositoryDiagnosticsPortOutputError::InvalidParserDiagnostics,
        ));
    }
    let Some(memory) = result.memory_projection.as_ref() else {
        return Ok(());
    };
    if memory.snapshot != result.snapshot || memory.source_epoch != result.source_epoch {
        return Err(RepositoryDiagnosticsError::InvalidPortOutput(
            RepositoryDiagnosticsPortOutputError::MemorySourceMismatch,
        ));
    }
    if !valid_memory_coverage(memory.coverage) {
        return Err(RepositoryDiagnosticsError::InvalidPortOutput(
            RepositoryDiagnosticsPortOutputError::InvalidMemoryCoverage,
        ));
    }
    Ok(())
}

fn valid_memory_coverage(coverage: MemoryRecallProjectionCoverage) -> bool {
    let states = [
        MemoryEffectiveState::Current,
        MemoryEffectiveState::NotApplicable,
        MemoryEffectiveState::Stale,
        MemoryEffectiveState::NeedsReview,
        MemoryEffectiveState::Indeterminate,
        MemoryEffectiveState::Conflicted,
        MemoryEffectiveState::Contradicted,
        MemoryEffectiveState::Superseded,
        MemoryEffectiveState::Quarantined,
        MemoryEffectiveState::Tombstoned,
    ];
    let state_total = states.into_iter().try_fold(0_u64, |total, state| {
        total.checked_add(coverage.state_count(state))
    });
    let unresolved = coverage
        .state_count(MemoryEffectiveState::NeedsReview)
        .checked_add(coverage.state_count(MemoryEffectiveState::Indeterminate))
        .and_then(|count| {
            count.checked_add(coverage.state_count(MemoryEffectiveState::Conflicted))
        });
    coverage.searched() == coverage.total()
        && state_total == Some(coverage.total())
        && unresolved == Some(coverage.unresolved())
}

fn check_control<E>(
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), RepositoryDiagnosticsError<E>> {
    if cancelled.load(Ordering::Acquire) {
        Err(RepositoryDiagnosticsError::Cancelled)
    } else if Instant::now() >= deadline {
        Err(RepositoryDiagnosticsError::DeadlineExceeded)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
