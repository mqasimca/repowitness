//! Closed, bounded code-discovery operation dispatch.
//!
//! This is deliberately a finite algebra over validated application use cases,
//! not an embedded graph query language or a storage escape hatch.

use std::{
    error::Error,
    fmt,
    sync::{Arc, atomic::AtomicBool},
    time::Instant,
};

use repowitness_domain::{RepositoryIdentityDigest, SourceSnapshotDigest};

use crate::{
    ArchitectureMapError, ArchitectureMapLimits, ArchitectureMapPort, ArchitectureMapRequest,
    ArchitectureMapResult, ArchitectureOverviewError, ArchitectureOverviewLimits,
    ArchitectureOverviewPort, ArchitectureOverviewRequest, ArchitectureOverviewResult,
    CodeSearchError, CodeSearchLimits, CodeSearchPort, CodeSearchQuery, CodeSearchRequest,
    OutboundSitesError, OutboundSitesLimits, OutboundSitesPort, OutboundSitesRequest,
    OutboundSitesResult, RelevantPathsError, RelevantPathsLimits, RelevantPathsResult,
    SymbolGetSelector, SymbolSearchError, SymbolSearchPort, SymbolSearchQuery, SymbolSearchRequest,
    SymbolSearchResult, SyntaxSiteSearchError, SyntaxSiteSearchLimits, SyntaxSiteSearchPort,
    SyntaxSiteSearchQuery, SyntaxSiteSearchRequest, SyntaxSiteSearchResult, TestMarkersError,
    TestMarkersLimits, TestMarkersPort, TestMarkersQuery, TestMarkersRequest, TestMarkersResult,
    architecture_map, architecture_overview, code_search, locate_relevant_paths, outbound_sites,
    symbol_search, syntax_site_search, test_markers,
};

/// Version of the finite code-discovery operation algebra.
pub const CODE_GRAPH_QUERY_PROFILE_VERSION: u16 = 1;

/// Exactly one admitted code-discovery operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CodeGraphQueryOperation<G> {
    /// Typed direct-declaration discovery.
    Symbols {
        /// Exact or prefix declaration selector.
        query: SymbolSearchQuery,
        /// Independent bounded result/output limits.
        limits: crate::CodeSearchLimits,
    },
    /// Exact parser observations physically contained in one declaration.
    OutboundSites {
        /// Required immutable source snapshot.
        expected_snapshot: SourceSnapshotDigest,
        /// Required immutable active generation.
        expected_generation: G,
        /// Exact declaration selector emitted by typed discovery.
        selector: SymbolGetSelector,
        /// Independent bounded result/output limits.
        limits: OutboundSitesLimits,
    },
    /// Exact raw target observations across the active immutable generation.
    SyntaxSiteSearch {
        /// Exact parser-emitted raw target spelling.
        query: SyntaxSiteSearchQuery,
        /// Independent bounded result/output limits.
        limits: SyntaxSiteSearchLimits,
    },
    /// Source-only structural orientation.
    Architecture {
        /// Independent bounded overview receipt limits.
        limits: ArchitectureOverviewLimits,
    },
    /// Exact indexed source-file inventory.
    Files {
        /// Independent bounded file-inventory limits.
        limits: ArchitectureMapLimits,
    },
    /// Repository-scoped parser-attributed test-marker observations.
    TestMarkers {
        /// Optional direct-fact language and path filters.
        query: TestMarkersQuery,
        /// Independent bounded result/output limits.
        limits: TestMarkersLimits,
    },
    /// Bounded source-path navigation over lexical declaration evidence.
    RelevantPaths {
        /// Validated literal declaration terms.
        query: CodeSearchQuery,
        /// Independent bounded lexical candidate and output limits.
        search_limits: CodeSearchLimits,
        /// Independent bounded path-presentation limit.
        path_limits: RelevantPathsLimits,
    },
}

/// One validated finite-algebra request.
pub struct CodeGraphQueryRequest<G> {
    repository: RepositoryIdentityDigest,
    operation: CodeGraphQueryOperation<G>,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
}

impl<G> CodeGraphQueryRequest<G> {
    /// Constructs one single-operation, bounded request.
    #[must_use]
    pub const fn new(
        repository: RepositoryIdentityDigest,
        operation: CodeGraphQueryOperation<G>,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Self {
        Self {
            repository,
            operation,
            cancelled,
            deadline,
        }
    }
}

impl<G: fmt::Debug> fmt::Debug for CodeGraphQueryRequest<G> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CodeGraphQueryRequest")
            .field("repository", &"<redacted-identity>")
            .field("operation", &self.operation)
            .field(
                "cancelled",
                &self.cancelled.load(std::sync::atomic::Ordering::Acquire),
            )
            .field("deadline", &"<monotonic>")
            .finish()
    }
}

/// One typed result from exactly one finite discovery operation.
#[derive(Debug, Eq, PartialEq)]
pub enum CodeGraphQueryResult<G> {
    /// Typed direct-declaration result.
    Symbols(SymbolSearchResult<G>),
    /// Exact declaration-contained raw parser observations.
    OutboundSites(OutboundSitesResult<G>),
    /// Exact raw target observations without target resolution.
    SyntaxSiteSearch(SyntaxSiteSearchResult<G>),
    /// Source-only architecture orientation.
    Architecture(ArchitectureOverviewResult<G>),
    /// Exact source-file inventory.
    Files(ArchitectureMapResult<G>),
    /// Repository-scoped raw test-marker observations.
    TestMarkers(TestMarkersResult<G>),
    /// Bounded source-path navigation over lexical declaration evidence.
    RelevantPaths(RelevantPathsResult<G>),
}

/// Stable closed-algebra failure preserving the selected operation's contract.
#[derive(Debug)]
pub enum CodeGraphQueryError<E> {
    /// Typed declaration search failed.
    Symbols(SymbolSearchError<E>),
    /// Exact declaration-contained raw-site read failed.
    OutboundSites(OutboundSitesError<E>),
    /// Exact raw target syntax-observation search failed.
    SyntaxSiteSearch(SyntaxSiteSearchError<E>),
    /// Source-only architecture overview failed.
    Architecture(ArchitectureOverviewError<E>),
    /// Exact source-file inventory failed.
    Files(ArchitectureMapError<E>),
    /// Repository-scoped raw test-marker read failed.
    TestMarkers(TestMarkersError<E>),
    /// The underlying bounded lexical path search failed.
    RelevantPathsSearch(CodeSearchError<E>),
    /// A completed lexical receipt could not be projected safely into paths.
    RelevantPathsProjection(RelevantPathsError),
}

impl<E: fmt::Display> fmt::Display for CodeGraphQueryError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Symbols(error) => error.fmt(formatter),
            Self::OutboundSites(error) => error.fmt(formatter),
            Self::SyntaxSiteSearch(error) => error.fmt(formatter),
            Self::Architecture(error) => error.fmt(formatter),
            Self::Files(error) => error.fmt(formatter),
            Self::TestMarkers(error) => error.fmt(formatter),
            Self::RelevantPathsSearch(error) => error.fmt(formatter),
            Self::RelevantPathsProjection(error) => error.fmt(formatter),
        }
    }
}

impl<E: Error + 'static> Error for CodeGraphQueryError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Symbols(error) => Some(error),
            Self::OutboundSites(error) => Some(error),
            Self::SyntaxSiteSearch(error) => Some(error),
            Self::Architecture(error) => Some(error),
            Self::Files(error) => Some(error),
            Self::TestMarkers(error) => Some(error),
            Self::RelevantPathsSearch(error) => Some(error),
            Self::RelevantPathsProjection(error) => Some(error),
        }
    }
}

/// Dispatches exactly one already-admitted code-discovery operation.
///
/// The bounds intentionally require one common generation and error type.  A
/// caller cannot use this envelope to compose operations, join results, or
/// expose a storage-specific query surface.
pub fn code_graph_query<Port>(
    port: &Port,
    request: CodeGraphQueryRequest<<Port as SymbolSearchPort>::Generation>,
) -> Result<
    CodeGraphQueryResult<<Port as SymbolSearchPort>::Generation>,
    CodeGraphQueryError<<Port as SymbolSearchPort>::Error>,
>
where
    Port: SymbolSearchPort,
    Port: OutboundSitesPort<
            Generation = <Port as SymbolSearchPort>::Generation,
            Error = <Port as SymbolSearchPort>::Error,
        > + ArchitectureOverviewPort<
            Generation = <Port as SymbolSearchPort>::Generation,
            Error = <Port as SymbolSearchPort>::Error,
        > + ArchitectureMapPort<
            Generation = <Port as SymbolSearchPort>::Generation,
            Error = <Port as SymbolSearchPort>::Error,
        > + TestMarkersPort<
            Generation = <Port as SymbolSearchPort>::Generation,
            Error = <Port as SymbolSearchPort>::Error,
        > + CodeSearchPort<
            Generation = <Port as SymbolSearchPort>::Generation,
            Error = <Port as SymbolSearchPort>::Error,
        > + SyntaxSiteSearchPort<
            Generation = <Port as SymbolSearchPort>::Generation,
            Error = <Port as SymbolSearchPort>::Error,
        >,
{
    match request.operation {
        CodeGraphQueryOperation::Symbols { query, limits } => symbol_search(
            port,
            SymbolSearchRequest::new(
                request.repository,
                query,
                limits,
                request.cancelled,
                request.deadline,
            ),
        )
        .map(CodeGraphQueryResult::Symbols)
        .map_err(CodeGraphQueryError::Symbols),
        CodeGraphQueryOperation::OutboundSites {
            expected_snapshot,
            expected_generation,
            selector,
            limits,
        } => outbound_sites(
            port,
            OutboundSitesRequest::new(
                request.repository,
                expected_snapshot,
                expected_generation,
                selector,
                limits,
                request.cancelled,
                request.deadline,
            ),
        )
        .map(CodeGraphQueryResult::OutboundSites)
        .map_err(CodeGraphQueryError::OutboundSites),
        CodeGraphQueryOperation::SyntaxSiteSearch { query, limits } => syntax_site_search(
            port,
            SyntaxSiteSearchRequest::new(
                request.repository,
                query,
                limits,
                request.cancelled,
                request.deadline,
            ),
        )
        .map(CodeGraphQueryResult::SyntaxSiteSearch)
        .map_err(CodeGraphQueryError::SyntaxSiteSearch),
        CodeGraphQueryOperation::Architecture { limits } => architecture_overview(
            port,
            ArchitectureOverviewRequest::new(
                request.repository,
                limits,
                request.cancelled,
                request.deadline,
            ),
        )
        .map(CodeGraphQueryResult::Architecture)
        .map_err(CodeGraphQueryError::Architecture),
        CodeGraphQueryOperation::Files { limits } => architecture_map(
            port,
            ArchitectureMapRequest::new(
                request.repository,
                limits,
                request.cancelled,
                request.deadline,
            ),
        )
        .map(CodeGraphQueryResult::Files)
        .map_err(CodeGraphQueryError::Files),
        CodeGraphQueryOperation::TestMarkers { query, limits } => test_markers(
            port,
            TestMarkersRequest::new(
                request.repository,
                query,
                limits,
                request.cancelled,
                request.deadline,
            ),
        )
        .map(CodeGraphQueryResult::TestMarkers)
        .map_err(CodeGraphQueryError::TestMarkers),
        CodeGraphQueryOperation::RelevantPaths {
            query,
            search_limits,
            path_limits,
        } => code_search(
            port,
            CodeSearchRequest::new(
                request.repository,
                query,
                search_limits,
                request.cancelled,
                request.deadline,
            ),
        )
        .map_err(CodeGraphQueryError::RelevantPathsSearch)
        .and_then(|search| {
            locate_relevant_paths(search, path_limits)
                .map(CodeGraphQueryResult::RelevantPaths)
                .map_err(CodeGraphQueryError::RelevantPathsProjection)
        }),
    }
}
