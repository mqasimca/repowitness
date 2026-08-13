use std::fmt;

use super::{MemoryMutationOperation, MemoryMutationRequestScope};

/// Stable categorical failure returned by the injected repository service.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RepositoryServiceError {
    /// Multi-language source architecture map failed without a usable result.
    ArchitectureMap,
    /// Bounded source-only repository orientation failed without a usable result.
    ArchitectureOverview,
    /// Bounded path-only repository topology inventory failed.
    RepositoryTopology,
    /// Local code search failed without a usable result.
    CodeSearch,
    /// Bounded lexical source-path navigation failed without a usable result.
    RelevantPaths,
    /// Typed direct declaration discovery failed without a usable result.
    SymbolSearch,
    /// Exact declaration-contained raw syntax-site read failed without a usable result.
    OutboundSites,
    /// Exact raw-target syntax-site discovery failed without a usable result.
    SyntaxSiteSearch,
    /// One closed bounded code-discovery operation failed without a usable result.
    CodeGraphQuery,
    /// Context compilation failed without a usable result.
    ContextBuild,
    /// Source-fenced revision-pinned change review failed without a receipt.
    ChangeReview,
    /// Repository diagnostics failed without a usable result.
    Diagnostics,
    /// Native Rust graph read failed without a usable result.
    GraphRead,
    /// SCIP evidence read failed without a usable result.
    ScipEvidence,
    /// SCIP relationship trace failed without a usable result.
    ScipRelationshipTrace,
    /// SCIP source-span resolution failed without a usable result.
    ScipSymbolResolve,
    /// Memory recall failed without a usable result.
    MemoryRecall,
    /// Authorized local memory management failed without a usable result.
    MemoryManage,
    /// An admitted memory mutation returned no definitive receipt within its bound.
    MemoryMutationOutcomeUnknown {
        /// Public request scope whose task outcome was uncertain.
        request_scope: MemoryMutationRequestScope,
        /// Durable operation, or `UnknownPhase` only after supervisor outcome loss.
        operation: MemoryMutationOperation,
    },
    /// Exact symbol retrieval failed without a usable result.
    SymbolGet,
}

impl fmt::Display for RepositoryServiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Self::MemoryMutationOutcomeUnknown {
            request_scope,
            operation,
        } = self
        {
            let guidance = if *operation == MemoryMutationOperation::UnknownPhase {
                request_scope.reconciliation_guidance()
            } else {
                operation.reconciliation_guidance()
            };
            return write!(
                formatter,
                "memory mutation outcome could not be determined; request_scope={}; operation={}; \
                 reconciliation_required_before_retry={}; automatic_retry=false",
                request_scope.as_str(),
                operation.as_str(),
                guidance
            );
        }
        formatter.write_str(match self {
            Self::ArchitectureMap => "architecture map failed",
            Self::ArchitectureOverview => "architecture overview failed",
            Self::RepositoryTopology => "repository topology failed",
            Self::CodeSearch => "code search failed",
            Self::RelevantPaths => "relevant-path navigation failed",
            Self::SymbolSearch => "symbol search failed",
            Self::OutboundSites => "outbound-sites read failed",
            Self::SyntaxSiteSearch => "syntax-site search failed",
            Self::CodeGraphQuery => "code-graph-query failed",
            Self::ContextBuild => "context build failed",
            Self::ChangeReview => "change review failed",
            Self::Diagnostics => "repository diagnostics failed",
            Self::GraphRead => "Rust graph read failed",
            Self::ScipEvidence => "SCIP evidence read failed",
            Self::ScipRelationshipTrace => "SCIP relationship trace failed",
            Self::ScipSymbolResolve => "SCIP symbol resolution failed",
            Self::MemoryRecall => "memory recall failed",
            Self::MemoryManage => "memory management failed",
            Self::MemoryMutationOutcomeUnknown { .. } => {
                unreachable!("outcome-unknown errors are rendered above")
            }
            Self::SymbolGet => "symbol retrieval failed",
        })
    }
}

impl std::error::Error for RepositoryServiceError {}

impl RepositoryServiceError {
    /// Constructs an attributed outcome-unknown mutation failure.
    #[must_use]
    pub const fn memory_mutation_outcome_unknown(
        request_scope: MemoryMutationRequestScope,
        operation: MemoryMutationOperation,
    ) -> Self {
        Self::MemoryMutationOutcomeUnknown {
            request_scope,
            operation,
        }
    }

    /// Constructs an outcome-unknown failure when the exact task phase was lost.
    #[must_use]
    pub const fn memory_mutation_phase_unknown(request_scope: MemoryMutationRequestScope) -> Self {
        Self::memory_mutation_outcome_unknown(request_scope, MemoryMutationOperation::UnknownPhase)
    }

    /// Returns the request scope and phase only for outcome-unknown mutations.
    #[must_use]
    pub const fn memory_mutation_attribution(
        self,
    ) -> Option<(MemoryMutationRequestScope, MemoryMutationOperation)> {
        match self {
            Self::MemoryMutationOutcomeUnknown {
                request_scope,
                operation,
            } => Some((request_scope, operation)),
            _ => None,
        }
    }
}
