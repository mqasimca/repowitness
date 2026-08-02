//! Strict closed-union wire contract for bounded code discovery.

use std::{fmt, time::Duration};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{
    ArchitectureMapInput, ArchitectureMapOutput, ArchitectureMapServiceRequest,
    ArchitectureOverviewInput, ArchitectureOverviewOutput, ArchitectureOverviewServiceRequest,
    OutboundSitesInput, OutboundSitesOutput, OutboundSitesServiceRequest, RelevantPathsInput,
    RelevantPathsOutput, RelevantPathsServiceRequest, SymbolSearchInput, SymbolSearchOutput,
    SymbolSearchServiceRequest, SyntaxSiteSearchInput, SyntaxSiteSearchOutput,
    SyntaxSiteSearchServiceRequest, TestMarkersInput, TestMarkersOutput, TestMarkersServiceRequest,
};

/// Native tool name for the finite code-discovery operation algebra.
pub const CODE_GRAPH_QUERY_TOOL_NAME: &str = "code_graph_query";
/// Wire schema version for the finite code-discovery operation algebra.
pub const CODE_GRAPH_QUERY_SCHEMA_VERSION: u16 = 1;
/// Application profile version for the finite code-discovery operation algebra.
pub const CODE_GRAPH_QUERY_PROFILE_VERSION: u16 = 1;
const FIXED_CODE_GRAPH_QUERY_OUTPUT_BYTES: u64 = 512;

/// Strict tagged union of the only admitted code-discovery operations.
#[derive(Deserialize, JsonSchema)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum CodeGraphQueryInput {
    /// Typed direct declaration facts.
    Symbols {
        /// Exact declaration name or deterministic name prefix.
        name: String,
        /// `exact` (default) or `prefix`.
        match_mode: Option<String>,
        /// Optional persisted syntax-adapter language.
        language: Option<String>,
        /// Optional direct syntax declaration kind.
        kind: Option<String>,
        /// Optional repository-relative byte prefix.
        path_prefix: Option<String>,
        /// Maximum returned declaration receipts.
        max_results: Option<u16>,
        /// End-to-end deadline in milliseconds.
        timeout_ms: Option<u64>,
    },
    /// Exact raw parser observations physically contained in one declaration.
    OutboundSites {
        /// Exact source snapshot digest from typed discovery.
        snapshot_sha256: String,
        /// Exact immutable generation identifier.
        generation: i64,
        /// Canonical byte-preserving repository path.
        path: String,
        /// Exact source-content digest.
        content_sha256: String,
        /// Exact source artifact digest.
        artifact_sha256: String,
        /// Exact declaration fact ordinal.
        fact_ordinal: u64,
        /// Maximum raw observations.
        max_sites: Option<u16>,
        /// End-to-end deadline in milliseconds.
        timeout_ms: Option<u64>,
    },
    /// Exact raw target observations across the active immutable generation.
    SyntaxSiteSearch {
        /// Exact parser-emitted raw target spelling.
        target: String,
        /// Maximum raw observations.
        max_sites: Option<u16>,
        /// End-to-end deadline in milliseconds.
        timeout_ms: Option<u64>,
    },
    /// Source-only structural orientation.
    Architecture {
        /// Maximum source-root summaries.
        max_roots: Option<u16>,
        /// Maximum function-named-`main` candidates.
        max_entry_point_candidates: Option<u16>,
        /// Maximum per-file declaration receipts.
        max_files: Option<u16>,
        /// End-to-end deadline in milliseconds.
        timeout_ms: Option<u64>,
    },
    /// Exact indexed source-file inventory.
    Files {
        /// Maximum retained indexed files.
        max_files: Option<u16>,
        /// End-to-end deadline in milliseconds.
        timeout_ms: Option<u64>,
    },
    /// Repository-scoped raw parser test-marker observations.
    TestMarkers {
        /// Optional exact built-in syntax language.
        language: Option<String>,
        /// Optional repository-relative byte prefix.
        path_prefix: Option<String>,
        /// Maximum retained marker observations.
        max_results: Option<u16>,
        /// End-to-end deadline in milliseconds.
        timeout_ms: Option<u64>,
    },
    /// Bounded lexical declaration path navigation.
    RelevantPaths {
        /// Literal Rust, Go, TypeScript, TSX, or Python declaration terms.
        query: String,
        /// Maximum returned source-path summaries.
        max_paths: Option<u16>,
        /// End-to-end deadline in milliseconds.
        timeout_ms: Option<u64>,
    },
}

impl fmt::Debug for CodeGraphQueryInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Symbols { .. } => formatter
                .debug_struct("CodeGraphQueryInput::Symbols")
                .field("name", &"<redacted-symbol>")
                .finish(),
            Self::OutboundSites { .. } => formatter
                .debug_struct("CodeGraphQueryInput::OutboundSites")
                .field("selector", &"<redacted>")
                .finish(),
            Self::SyntaxSiteSearch { .. } => formatter
                .debug_struct("CodeGraphQueryInput::SyntaxSiteSearch")
                .field("target", &"<redacted-raw-target>")
                .finish(),
            Self::Architecture { .. } => formatter
                .debug_struct("CodeGraphQueryInput::Architecture")
                .finish(),
            Self::Files { .. } => formatter
                .debug_struct("CodeGraphQueryInput::Files")
                .finish(),
            Self::TestMarkers { .. } => formatter
                .debug_struct("CodeGraphQueryInput::TestMarkers")
                .field("filters", &"<redacted>")
                .finish(),
            Self::RelevantPaths { .. } => formatter
                .debug_struct("CodeGraphQueryInput::RelevantPaths")
                .field("query", &"<redacted-query>")
                .finish(),
        }
    }
}

impl CodeGraphQueryInput {
    /// Validates exactly one operation before invoking the repository service.
    pub(crate) fn validate(self) -> Result<CodeGraphQueryServiceRequest, &'static str> {
        match self {
            Self::Symbols {
                name,
                match_mode,
                language,
                kind,
                path_prefix,
                max_results,
                timeout_ms,
            } => SymbolSearchInput {
                name,
                match_mode,
                language,
                kind,
                path_prefix,
                max_results,
                timeout_ms,
            }
            .validate()
            .map(CodeGraphQueryServiceRequest::Symbols),
            Self::OutboundSites {
                snapshot_sha256,
                generation,
                path,
                content_sha256,
                artifact_sha256,
                fact_ordinal,
                max_sites,
                timeout_ms,
            } => OutboundSitesInput {
                snapshot_sha256,
                generation,
                path,
                content_sha256,
                artifact_sha256,
                fact_ordinal,
                max_sites,
                timeout_ms,
            }
            .validate()
            .map(CodeGraphQueryServiceRequest::OutboundSites),
            Self::SyntaxSiteSearch {
                target,
                max_sites,
                timeout_ms,
            } => SyntaxSiteSearchInput {
                target,
                max_sites,
                timeout_ms,
            }
            .validate()
            .map(CodeGraphQueryServiceRequest::SyntaxSiteSearch),
            Self::Architecture {
                max_roots,
                max_entry_point_candidates,
                max_files,
                timeout_ms,
            } => ArchitectureOverviewInput {
                max_roots,
                max_entry_point_candidates,
                max_files,
                timeout_ms,
            }
            .validate()
            .map(CodeGraphQueryServiceRequest::Architecture),
            Self::Files {
                max_files,
                timeout_ms,
            } => ArchitectureMapInput {
                max_files,
                timeout_ms,
            }
            .validate()
            .map(CodeGraphQueryServiceRequest::Files),
            Self::TestMarkers {
                language,
                path_prefix,
                max_results,
                timeout_ms,
            } => TestMarkersInput {
                language,
                path_prefix,
                max_results,
                timeout_ms,
            }
            .validate()
            .map(CodeGraphQueryServiceRequest::TestMarkers),
            Self::RelevantPaths {
                query,
                max_paths,
                timeout_ms,
            } => RelevantPathsInput {
                query,
                max_paths,
                timeout_ms,
            }
            .validate()
            .map(CodeGraphQueryServiceRequest::RelevantPaths),
        }
    }
}

/// One validated finite-algebra request passed to the local composition root.
pub enum CodeGraphQueryServiceRequest {
    /// Typed declaration discovery.
    Symbols(SymbolSearchServiceRequest),
    /// Exact declaration-contained raw sites.
    OutboundSites(OutboundSitesServiceRequest),
    /// Exact raw-target syntax observations.
    SyntaxSiteSearch(SyntaxSiteSearchServiceRequest),
    /// Source-only architecture orientation.
    Architecture(ArchitectureOverviewServiceRequest),
    /// Exact indexed source-file inventory.
    Files(ArchitectureMapServiceRequest),
    /// Repository-scoped raw test-marker observations.
    TestMarkers(TestMarkersServiceRequest),
    /// Bounded lexical declaration path navigation.
    RelevantPaths(RelevantPathsServiceRequest),
}

impl CodeGraphQueryServiceRequest {
    /// Returns the selected operation's original end-to-end timeout.
    #[must_use]
    pub const fn timeout(&self) -> Duration {
        match self {
            Self::Symbols(request) => request.timeout(),
            Self::OutboundSites(request) => request.timeout(),
            Self::SyntaxSiteSearch(request) => request.timeout(),
            Self::Architecture(request) => request.timeout(),
            Self::Files(request) => request.timeout(),
            Self::TestMarkers(request) => request.timeout(),
            Self::RelevantPaths(request) => request.timeout(),
        }
    }

    pub(crate) fn with_timeout(self, timeout: Duration) -> Self {
        match self {
            Self::Symbols(request) => Self::Symbols(request.with_timeout(timeout)),
            Self::OutboundSites(request) => Self::OutboundSites(request.with_timeout(timeout)),
            Self::SyntaxSiteSearch(request) => {
                Self::SyntaxSiteSearch(request.with_timeout(timeout))
            }
            Self::Architecture(request) => Self::Architecture(request.with_timeout(timeout)),
            Self::Files(request) => Self::Files(request.with_timeout(timeout)),
            Self::TestMarkers(request) => Self::TestMarkers(request.with_timeout(timeout)),
            Self::RelevantPaths(request) => Self::RelevantPaths(request.with_timeout(timeout)),
        }
    }
}

impl fmt::Debug for CodeGraphQueryServiceRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CodeGraphQueryServiceRequest")
            .field(
                "operation",
                &match self {
                    Self::Symbols(_) => "symbols",
                    Self::OutboundSites(_) => "outbound_sites",
                    Self::SyntaxSiteSearch(_) => "syntax_site_search",
                    Self::Architecture(_) => "architecture",
                    Self::Files(_) => "files",
                    Self::TestMarkers(_) => "test_markers",
                    Self::RelevantPaths(_) => "relevant_paths",
                },
            )
            .finish()
    }
}

/// Structurally tagged result payload for exactly one finite operation.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "operation", content = "result", rename_all = "snake_case")]
pub enum CodeGraphQueryResultOutput {
    /// Typed declaration discovery output.
    Symbols(SymbolSearchOutput),
    /// Exact declaration-contained raw sites output.
    OutboundSites(OutboundSitesOutput),
    /// Exact raw-target syntax-observation output.
    SyntaxSiteSearch(SyntaxSiteSearchOutput),
    /// Source-only architecture orientation output.
    Architecture(ArchitectureOverviewOutput),
    /// Exact indexed source-file inventory output.
    Files(ArchitectureMapOutput),
    /// Repository-scoped raw test-marker output.
    TestMarkers(TestMarkersOutput),
    /// Bounded lexical declaration path-navigation output.
    RelevantPaths(RelevantPathsOutput),
}

/// Version-1 response for the closed finite code-discovery algebra.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CodeGraphQueryOutput {
    /// Wire schema version.
    pub schema_version: u16,
    /// Closed operation-algebra profile version.
    pub code_graph_query_profile: u16,
    /// Conservative encoded-envelope byte accounting for the selected result.
    pub output_bytes: u64,
    /// Structurally coupled selected operation and its typed output.
    #[serde(flatten)]
    pub result: CodeGraphQueryResultOutput,
}

impl CodeGraphQueryOutput {
    /// Wraps a validated native use-case output without translating its receipts.
    #[must_use]
    pub fn new(result: CodeGraphQueryResultOutput) -> Self {
        let output_bytes = serde_json::to_vec(&result)
            .ok()
            .and_then(|encoded| u64::try_from(encoded.len()).ok())
            .and_then(|encoded| encoded.checked_add(FIXED_CODE_GRAPH_QUERY_OUTPUT_BYTES))
            .unwrap_or(u64::MAX);
        Self {
            schema_version: CODE_GRAPH_QUERY_SCHEMA_VERSION,
            code_graph_query_profile: CODE_GRAPH_QUERY_PROFILE_VERSION,
            output_bytes,
            result,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CodeGraphQueryInput, CodeGraphQueryServiceRequest};

    #[test]
    fn every_closed_operation_validates_without_storage_access() {
        let digest = "ab".repeat(32);
        let inputs = [
            serde_json::json!({"operation":"symbols", "name":"run", "match_mode":"prefix"}),
            serde_json::json!({
                "operation":"outbound_sites", "snapshot_sha256":digest, "generation":1,
                "path":"rwp1:h:7372632F6C69622E7273", "content_sha256":"cd".repeat(32),
                "artifact_sha256":"ef".repeat(32), "fact_ordinal":0
            }),
            serde_json::json!({"operation":"syntax_site_search", "target":"run", "max_sites":1}),
            serde_json::json!({"operation":"architecture", "max_roots":1, "max_entry_point_candidates":1, "max_files":1}),
            serde_json::json!({"operation":"files", "max_files":1}),
            serde_json::json!({"operation":"test_markers", "language":"rust", "path_prefix":"src/", "max_results":1}),
            serde_json::json!({"operation":"relevant_paths", "query":"Widget", "max_paths":1}),
        ];
        for input in inputs {
            let input: CodeGraphQueryInput = serde_json::from_value(input)
                .expect("the closed operation schema should deserialize");
            assert!(input.validate().is_ok());
        }
    }

    #[test]
    fn unknown_or_cross_variant_fields_are_rejected_before_service_access() {
        let unknown = serde_json::json!({"operation":"cypher", "query":"MATCH (n)"});
        assert!(serde_json::from_value::<CodeGraphQueryInput>(unknown).is_err());
        let cross_variant = serde_json::json!({
            "operation":"files", "max_files":1, "name":"must_not_be_accepted"
        });
        assert!(serde_json::from_value::<CodeGraphQueryInput>(cross_variant).is_err());
        let invalid = serde_json::json!({"operation":"test_markers", "path_prefix":"../escape"});
        let input: CodeGraphQueryInput =
            serde_json::from_value(invalid).expect("shape-only decoding should succeed");
        assert!(input.validate().is_err());
    }

    #[test]
    fn validated_request_preserves_one_operation() {
        let input: CodeGraphQueryInput =
            serde_json::from_value(serde_json::json!({"operation":"files", "max_files":1}))
                .expect("files schema should deserialize");
        assert!(matches!(
            input.validate(),
            Ok(CodeGraphQueryServiceRequest::Files(_))
        ));
    }
}
