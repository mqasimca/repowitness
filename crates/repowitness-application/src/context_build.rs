//! Deterministic, bounded Phase 0 context compilation.

use std::{error::Error, fmt};

use repowitness_domain::{CoverageSummary, RepositoryIdentityDigest, SourceSnapshotDigest};

use crate::{
    CodeSearchQueryDigest, MAX_CODE_SEARCH_RESULTS, MAX_SYMBOL_GET_DECLARATION_BYTES,
    MemoryRecallProducer, MemoryRecallProjectionCoverage, MemoryRecallRecord, RustSymbolOccurrence,
    SymbolGetSelector,
};

/// Version of the Phase 0 context-fusion and admission profile.
pub const CONTEXT_BUILD_PROFILE_VERSION: u16 = 1;
/// Fixed reciprocal-rank-fusion constant for profile version 1.
pub const CONTEXT_BUILD_RRF_K: u16 = 60;
/// Default conservative context-content budget.
pub const DEFAULT_CONTEXT_BUILD_BUDGET_UNITS: u64 = 64 * 1024;
/// Hard Phase 0 conservative context-content budget ceiling.
pub const MAX_CONTEXT_BUILD_BUDGET_UNITS: u64 = 1024 * 1024;

/// Stable failure to admit or compile one context request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContextBuildError {
    /// The conservative content budget was zero or exceeded the Phase 0 ceiling.
    InvalidBudget,
    /// Source-provider counts, ranks, or candidate data were inconsistent.
    InvalidSourceInput,
    /// An exact source declaration did not agree with its persisted selector.
    InvalidSourceCandidate,
    /// Source and memory inputs did not describe the same snapshot and generation.
    ContextMismatch,
    /// A memory row marked current did not contain a complete selected record.
    InvalidMemoryCandidate,
    /// A fixed-width count or budget calculation overflowed.
    CountNotRepresentable,
    /// Cancellation was visible before a complete pack existed.
    Cancelled,
    /// The request deadline elapsed before a complete pack existed.
    DeadlineExceeded,
}

impl fmt::Display for ContextBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidBudget => "context budget is zero or exceeds the Phase 0 ceiling",
            Self::InvalidSourceInput => "context source-provider input is inconsistent",
            Self::InvalidSourceCandidate => {
                "context source candidate does not match its exact selector"
            }
            Self::ContextMismatch => "context providers do not describe the same source generation",
            Self::InvalidMemoryCandidate => "current context memory is missing its selected record",
            Self::CountNotRepresentable => "context count or budget is not representable safely",
            Self::Cancelled => "context build was cancelled",
            Self::DeadlineExceeded => "context build deadline exceeded",
        })
    }
}

impl Error for ContextBuildError {}

/// Validated conservative content budget.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContextBuildBudget(u64);

impl ContextBuildBudget {
    /// Validates a positive byte-estimator budget against the hard ceiling.
    pub const fn try_new(units: u64) -> Result<Self, ContextBuildError> {
        if units == 0 || units > MAX_CONTEXT_BUILD_BUDGET_UNITS {
            Err(ContextBuildError::InvalidBudget)
        } else {
            Ok(Self(units))
        }
    }

    /// Returns admitted conservative budget units.
    #[must_use]
    pub const fn units(self) -> u64 {
        self.0
    }
}

impl Default for ContextBuildBudget {
    fn default() -> Self {
        Self(DEFAULT_CONTEXT_BUILD_BUDGET_UNITS)
    }
}

/// Versioned estimator used for context admission.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContextBudgetEstimator {
    /// One UTF-8 content byte consumes one conservative budget unit.
    Utf8BytesUpperBoundV1,
}

impl ContextBudgetEstimator {
    /// Returns the stable wire and diagnostics label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Utf8BytesUpperBoundV1 => "utf8_bytes_upper_bound_v1",
        }
    }
}

/// Provider classes represented by or explicitly omitted from a context pack.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ContextProvider {
    /// Active engineering-memory projection.
    Memory,
    /// Exact declarations expanded from lexical source matches.
    Source,
    /// Structural dependency or containment expansion.
    Structural,
    /// Reference or caller/callee expansion.
    References,
    /// Git-history retrieval.
    History,
}

/// Exact expanded source candidate with its provider-local rank.
#[derive(Clone, Eq, PartialEq)]
pub struct ContextSourceCandidate {
    provider_rank: u16,
    selector: SymbolGetSelector,
    occurrence: RustSymbolOccurrence,
    declaration: Box<[u8]>,
}

impl ContextSourceCandidate {
    /// Validates declaration bytes against the exact persisted occurrence.
    pub fn try_new(
        provider_rank: u16,
        selector: SymbolGetSelector,
        occurrence: RustSymbolOccurrence,
        declaration: Box<[u8]>,
    ) -> Result<Self, ContextBuildError> {
        let span = occurrence.declaration_span();
        let name_span = occurrence.name_span();
        let declaration_len = u64::try_from(declaration.len())
            .map_err(|_| ContextBuildError::CountNotRepresentable)?;
        let relative_start = name_span.start().get().checked_sub(span.start().get());
        let relative_end = name_span.end().get().checked_sub(span.start().get());
        let name_matches = relative_start
            .zip(relative_end)
            .and_then(|(start, end)| {
                let start = usize::try_from(start).ok()?;
                let end = usize::try_from(end).ok()?;
                declaration.get(start..end)
            })
            .is_some_and(|bytes| bytes == occurrence.name().as_bytes());
        if provider_rank == 0
            || selector.artifact_digest() != occurrence.artifact_digest()
            || selector.fact_ordinal() != occurrence.fact_ordinal()
            || !occurrence
                .language()
                .matches_repository_path(selector.path())
            || span.len().get() != declaration_len
            || declaration_len == 0
            || declaration_len > MAX_SYMBOL_GET_DECLARATION_BYTES
            || !name_matches
        {
            return Err(ContextBuildError::InvalidSourceCandidate);
        }
        Ok(Self {
            provider_rank,
            selector,
            occurrence,
            declaration,
        })
    }

    /// Returns the original lexical-provider rank.
    #[must_use]
    pub const fn provider_rank(&self) -> u16 {
        self.provider_rank
    }

    /// Returns the exact generation-local selector.
    #[must_use]
    pub const fn selector(&self) -> &SymbolGetSelector {
        &self.selector
    }

    /// Returns the validated syntax occurrence.
    #[must_use]
    pub const fn occurrence(&self) -> &RustSymbolOccurrence {
        &self.occurrence
    }

    /// Returns exact declaration bytes verified by the local source adapter.
    #[must_use]
    pub const fn declaration(&self) -> &[u8] {
        &self.declaration
    }
}

impl fmt::Debug for ContextSourceCandidate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ContextSourceCandidate")
            .field("provider_rank", &self.provider_rank)
            .field("selector", &self.selector)
            .field("occurrence", &self.occurrence)
            .field("declaration_bytes", &self.declaration.len())
            .field("declaration", &"<redacted-source>")
            .finish()
    }
}

/// Validated source-provider input for one compiler invocation.
pub struct ContextSourceInput<G> {
    repository: RepositoryIdentityDigest,
    query: CodeSearchQueryDigest,
    snapshot: SourceSnapshotDigest,
    generation: G,
    coverage: CoverageSummary,
    total_matches: u64,
    returned_matches: u64,
    candidates: Vec<ContextSourceCandidate>,
}

impl<G: Copy> ContextSourceInput<G> {
    /// Validates source counts and strictly increasing provider ranks.
    #[allow(
        clippy::too_many_arguments,
        reason = "source identity, coverage, counts, and candidates remain explicit"
    )]
    pub fn try_new(
        repository: RepositoryIdentityDigest,
        query: CodeSearchQueryDigest,
        snapshot: SourceSnapshotDigest,
        generation: G,
        coverage: CoverageSummary,
        total_matches: u64,
        returned_matches: u64,
        candidates: Vec<ContextSourceCandidate>,
    ) -> Result<Self, ContextBuildError> {
        let candidate_count = u64::try_from(candidates.len())
            .map_err(|_| ContextBuildError::CountNotRepresentable)?;
        let ranks_valid = candidates
            .windows(2)
            .all(|pair| pair[0].provider_rank() < pair[1].provider_rank())
            && candidates
                .last()
                .is_none_or(|candidate| u64::from(candidate.provider_rank()) <= returned_matches);
        if returned_matches > total_matches
            || returned_matches > u64::from(MAX_CODE_SEARCH_RESULTS)
            || candidate_count > returned_matches
            || !ranks_valid
        {
            return Err(ContextBuildError::InvalidSourceInput);
        }
        Ok(Self {
            repository,
            query,
            snapshot,
            generation,
            coverage,
            total_matches,
            returned_matches,
            candidates,
        })
    }
}

impl<G> fmt::Debug for ContextSourceInput<G> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ContextSourceInput")
            .field("repository", &self.repository)
            .field("query", &self.query)
            .field("snapshot", &self.snapshot)
            .field("generation", &"<opaque-generation>")
            .field("coverage", &self.coverage)
            .field("total_matches", &self.total_matches)
            .field("returned_matches", &self.returned_matches)
            .field("expanded_candidates", &self.candidates.len())
            .finish()
    }
}

/// Component and fused ranks retained on every admitted item.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContextRank {
    provider: ContextProvider,
    provider_rank: u16,
    fused_rank: u16,
    reciprocal_rank_denominator: u16,
}

impl ContextRank {
    /// Returns the contributing provider.
    #[must_use]
    pub const fn provider(self) -> ContextProvider {
        self.provider
    }

    /// Returns the rank assigned by that provider.
    #[must_use]
    pub const fn provider_rank(self) -> u16 {
        self.provider_rank
    }

    /// Returns the pre-budget rank assigned by the fusion profile.
    #[must_use]
    pub const fn fused_rank(self) -> u16 {
        self.fused_rank
    }

    /// Returns the denominator of `1 / (k + provider_rank)`.
    #[must_use]
    pub const fn reciprocal_rank_denominator(self) -> u16 {
        self.reciprocal_rank_denominator
    }
}

/// One admitted exact source declaration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextSourceItem {
    rank: ContextRank,
    estimated_units: u64,
    candidate: ContextSourceCandidate,
}

impl ContextSourceItem {
    /// Returns component and fused ranks.
    #[must_use]
    pub const fn rank(&self) -> ContextRank {
        self.rank
    }

    /// Returns conservative content-budget units consumed.
    #[must_use]
    pub const fn estimated_units(&self) -> u64 {
        self.estimated_units
    }

    /// Returns exact source identity and declaration content.
    #[must_use]
    pub const fn candidate(&self) -> &ContextSourceCandidate {
        &self.candidate
    }
}

/// One admitted current engineering-memory record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextMemoryItem {
    rank: ContextRank,
    estimated_units: u64,
    record: MemoryRecallRecord,
}

impl ContextMemoryItem {
    /// Returns component and fused ranks.
    #[must_use]
    pub const fn rank(&self) -> ContextRank {
        self.rank
    }

    /// Returns conservative content-budget units consumed.
    #[must_use]
    pub const fn estimated_units(&self) -> u64 {
        self.estimated_units
    }

    /// Returns the complete current projected record and its evidence.
    #[must_use]
    pub const fn record(&self) -> &MemoryRecallRecord {
        &self.record
    }
}

/// One heterogeneous context-pack item.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ContextItem {
    /// Current projected engineering memory.
    Memory(ContextMemoryItem),
    /// Exact verified source declaration.
    Source(ContextSourceItem),
}

impl ContextItem {
    /// Returns component and fused ranks.
    #[must_use]
    pub const fn rank(&self) -> ContextRank {
        match self {
            Self::Memory(item) => item.rank(),
            Self::Source(item) => item.rank(),
        }
    }

    /// Returns conservative content-budget units consumed.
    #[must_use]
    pub const fn estimated_units(&self) -> u64 {
        match self {
            Self::Memory(item) => item.estimated_units(),
            Self::Source(item) => item.estimated_units(),
        }
    }
}

/// Exact memory-projection context carried by a compiled pack.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextMemoryProjection<P> {
    projection: P,
    source_epoch: u64,
    producer: MemoryRecallProducer,
    coverage: MemoryRecallProjectionCoverage,
}

impl<P> ContextMemoryProjection<P> {
    /// Returns the immutable projection identity.
    #[must_use]
    pub const fn projection(&self) -> &P {
        &self.projection
    }

    /// Returns the exact source epoch used during revalidation.
    #[must_use]
    pub const fn source_epoch(&self) -> u64 {
        self.source_epoch
    }

    /// Returns correspondence-producer attribution.
    #[must_use]
    pub const fn producer(&self) -> &MemoryRecallProducer {
        &self.producer
    }

    /// Returns complete active projection coverage.
    #[must_use]
    pub const fn coverage(&self) -> MemoryRecallProjectionCoverage {
        self.coverage
    }
}

/// Exact retrieval and admission coverage for one context pack.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContextBuildCoverage {
    source_index: CoverageSummary,
    source_total_matches: u64,
    source_returned_matches: u64,
    source_expansion_omitted: u64,
    source_budget_omitted: u64,
    source_included: u64,
    memory_total_matches: u64,
    memory_returned_matches: u64,
    memory_non_current_omitted: u64,
    memory_budget_omitted: u64,
    memory_included: u64,
}

impl ContextBuildCoverage {
    /// Returns source-index coverage inherited from lexical search.
    #[must_use]
    pub const fn source_index(self) -> CoverageSummary {
        self.source_index
    }

    /// Returns source matches before the search result bound.
    #[must_use]
    pub const fn source_total_matches(self) -> u64 {
        self.source_total_matches
    }

    /// Returns source matches carried by lexical search.
    #[must_use]
    pub const fn source_returned_matches(self) -> u64 {
        self.source_returned_matches
    }

    /// Returns source matches not expanded because a declaration exceeded the pack ceiling.
    #[must_use]
    pub const fn source_expansion_omitted(self) -> u64 {
        self.source_expansion_omitted
    }

    /// Returns expanded source declarations omitted by the final budget.
    #[must_use]
    pub const fn source_budget_omitted(self) -> u64 {
        self.source_budget_omitted
    }

    /// Returns source declarations admitted to the pack.
    #[must_use]
    pub const fn source_included(self) -> u64 {
        self.source_included
    }

    /// Returns memory matches before the recall result bound.
    #[must_use]
    pub const fn memory_total_matches(self) -> u64 {
        self.memory_total_matches
    }

    /// Returns projected memory rows carried by recall.
    #[must_use]
    pub const fn memory_returned_matches(self) -> u64 {
        self.memory_returned_matches
    }

    /// Returns returned memory rows excluded because they were not current.
    #[must_use]
    pub const fn memory_non_current_omitted(self) -> u64 {
        self.memory_non_current_omitted
    }

    /// Returns current memory rows omitted by the final budget.
    #[must_use]
    pub const fn memory_budget_omitted(self) -> u64 {
        self.memory_budget_omitted
    }

    /// Returns current memory rows admitted to the pack.
    #[must_use]
    pub const fn memory_included(self) -> u64 {
        self.memory_included
    }
}

/// Explicit reason that relevant work or a provider is absent from the pack.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContextOmission {
    /// Lexical source matches exceeded the provider result bound.
    SourceSearchLimit(u64),
    /// A source declaration exceeded the complete pack's hard ceiling.
    SourceExpansionLimit(u64),
    /// No active memory projection existed.
    MemoryProjectionUnavailable,
    /// Matching memory rows exceeded the recall result bound.
    MemoryRecallLimit(u64),
    /// Returned memory rows were not current.
    MemoryNotCurrent(u64),
    /// Complete items from one provider did not fit the remaining budget.
    Budget {
        /// Provider whose items were omitted.
        provider: ContextProvider,
        /// Exact number of omitted complete items.
        count: u64,
    },
    /// A Phase 0 provider is not implemented.
    ProviderUnavailable(ContextProvider),
}

/// Complete deterministic context pack.
pub struct ContextBuildResult<G, P> {
    repository: RepositoryIdentityDigest,
    query: CodeSearchQueryDigest,
    snapshot: SourceSnapshotDigest,
    generation: G,
    memory: Option<ContextMemoryProjection<P>>,
    budget: ContextBuildBudget,
    used_units: u64,
    items: Box<[ContextItem]>,
    coverage: ContextBuildCoverage,
    omissions: Box<[ContextOmission]>,
}

impl<G, P> ContextBuildResult<G, P> {
    /// Returns the exact repository identity.
    #[must_use]
    pub const fn repository(&self) -> RepositoryIdentityDigest {
        self.repository
    }

    /// Returns the canonical literal intent identity.
    #[must_use]
    pub const fn query(&self) -> CodeSearchQueryDigest {
        self.query
    }

    /// Returns the exact active source snapshot.
    #[must_use]
    pub const fn snapshot(&self) -> SourceSnapshotDigest {
        self.snapshot
    }

    /// Returns the exact active generation.
    #[must_use]
    pub const fn generation(&self) -> &G {
        &self.generation
    }

    /// Returns active memory-projection context when available.
    #[must_use]
    pub const fn memory(&self) -> Option<&ContextMemoryProjection<P>> {
        self.memory.as_ref()
    }

    /// Returns the admitted conservative budget.
    #[must_use]
    pub const fn budget(&self) -> ContextBuildBudget {
        self.budget
    }

    /// Returns conservative units consumed by admitted item content.
    #[must_use]
    pub const fn used_units(&self) -> u64 {
        self.used_units
    }

    /// Returns items in deterministic pre-budget fusion order.
    #[must_use]
    pub const fn items(&self) -> &[ContextItem] {
        &self.items
    }

    /// Returns exact retrieval and admission coverage.
    #[must_use]
    pub const fn coverage(&self) -> ContextBuildCoverage {
        self.coverage
    }

    /// Returns deterministic explicit omission reasons.
    #[must_use]
    pub const fn omissions(&self) -> &[ContextOmission] {
        &self.omissions
    }

    /// Returns the fusion and admission profile version.
    #[must_use]
    pub const fn profile_version(&self) -> u16 {
        CONTEXT_BUILD_PROFILE_VERSION
    }

    /// Returns the exact budget-estimator identity.
    #[must_use]
    pub const fn budget_estimator(&self) -> ContextBudgetEstimator {
        ContextBudgetEstimator::Utf8BytesUpperBoundV1
    }
}

impl<G: fmt::Debug, P: fmt::Debug> fmt::Debug for ContextBuildResult<G, P> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ContextBuildResult")
            .field("repository", &self.repository)
            .field("query", &self.query)
            .field("snapshot", &self.snapshot)
            .field("generation", &self.generation)
            .field("memory", &self.memory)
            .field("budget", &self.budget)
            .field("used_units", &self.used_units)
            .field("items", &self.items.len())
            .field("coverage", &self.coverage)
            .field("omissions", &self.omissions)
            .finish()
    }
}

include!("context_build/use_case.rs");

#[cfg(test)]
mod tests;
