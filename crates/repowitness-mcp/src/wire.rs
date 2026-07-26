use std::{
    fmt,
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};

use repowitness_application::{
    CodeSearchQuery, DEFAULT_CODE_SEARCH_RESULTS, MAX_CODE_SEARCH_RESULTS,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// MCP tool name for bounded lexical Rust-symbol search.
pub const CODE_SEARCH_TOOL_NAME: &str = "code_search";
/// MCP tool name for exact verified declaration retrieval.
pub const SYMBOL_GET_TOOL_NAME: &str = "symbol_get";

pub(crate) const DEFAULT_MCP_TIMEOUT_MS: u64 = 5_000;
pub(crate) const MAX_MCP_TIMEOUT_MS: u64 = 30_000;
pub(crate) const MAX_PATH_TEXT_BYTES: usize = 2_097_160;
pub(crate) const MAX_MCP_SEARCH_OUTPUT_BYTES: usize = 3 * 1024 * 1024;
pub(crate) const MAX_MCP_SYMBOL_OUTPUT_BYTES: usize = 40 * 1024 * 1024;

/// Version-1 wire input for `code_search`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CodeSearchInput {
    /// Literal Rust symbol terms. FTS syntax is never accepted.
    pub query: String,
    /// Maximum returned candidates, from 1 through 100.
    pub max_results: Option<u16>,
    /// End-to-end operation deadline in milliseconds.
    pub timeout_ms: Option<u64>,
}

impl fmt::Debug for CodeSearchInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CodeSearchInput")
            .field("query", &"<redacted-query>")
            .field("max_results", &self.max_results)
            .field("timeout_ms", &self.timeout_ms)
            .finish()
    }
}

impl CodeSearchInput {
    pub(crate) fn validate(self) -> Result<CodeSearchServiceRequest, &'static str> {
        let query = CodeSearchQuery::try_new(&self.query)
            .map_err(|_| "query does not satisfy the bounded literal search profile")?;
        let max_results = self.max_results.unwrap_or(DEFAULT_CODE_SEARCH_RESULTS);
        if !(1..=MAX_CODE_SEARCH_RESULTS).contains(&max_results) {
            return Err("max_results must be between 1 and 100");
        }
        let timeout = validate_timeout(self.timeout_ms)?;
        Ok(CodeSearchServiceRequest {
            query: query.as_str().to_owned(),
            max_results,
            timeout,
        })
    }
}

/// Validated, owned request passed from the MCP adapter to the composition root.
pub struct CodeSearchServiceRequest {
    query: String,
    max_results: u16,
    timeout: Duration,
}

impl CodeSearchServiceRequest {
    /// Returns the canonical literal query.
    #[must_use]
    pub fn query(&self) -> &str {
        &self.query
    }

    /// Returns the inclusive result-count bound.
    #[must_use]
    pub const fn max_results(&self) -> u16 {
        self.max_results
    }

    /// Returns the remaining end-to-end deadline duration.
    #[must_use]
    pub const fn timeout(&self) -> Duration {
        self.timeout
    }

    pub(crate) const fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

impl fmt::Debug for CodeSearchServiceRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CodeSearchServiceRequest")
            .field("query", &"<redacted-query>")
            .field("max_results", &self.max_results)
            .field("timeout", &self.timeout)
            .finish()
    }
}

/// Version-1 wire input for `symbol_get`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SymbolGetInput {
    /// Exact snapshot SHA-256 from a `code_search` result.
    pub snapshot_sha256: String,
    /// Exact positive active-generation identifier.
    pub generation: i64,
    /// Canonical byte-preserving repository path from a search match.
    pub path: String,
    /// Exact source-content SHA-256 from a search match.
    pub content_sha256: String,
    /// Exact analysis-artifact SHA-256 from a search match.
    pub artifact_sha256: String,
    /// Exact fact ordinal from a search match.
    pub fact_ordinal: u64,
    /// End-to-end operation deadline in milliseconds.
    pub timeout_ms: Option<u64>,
}

impl fmt::Debug for SymbolGetInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SymbolGetInput")
            .field("snapshot_sha256", &"<redacted-digest>")
            .field("generation", &self.generation)
            .field("path", &"<redacted-path>")
            .field("content_sha256", &"<redacted-digest>")
            .field("artifact_sha256", &"<redacted-digest>")
            .field("fact_ordinal", &self.fact_ordinal)
            .field("timeout_ms", &self.timeout_ms)
            .finish()
    }
}

impl SymbolGetInput {
    pub(crate) fn validate(self) -> Result<SymbolGetServiceRequest, &'static str> {
        if self.generation <= 0 {
            return Err("generation must be a positive identifier");
        }
        if !is_lowercase_sha256(&self.snapshot_sha256)
            || !is_lowercase_sha256(&self.content_sha256)
            || !is_lowercase_sha256(&self.artifact_sha256)
        {
            return Err("digest fields must be lowercase SHA-256 text");
        }
        if !is_canonical_path_text(&self.path) {
            return Err("path must be bounded canonical rwp1:h: text");
        }
        let timeout = validate_timeout(self.timeout_ms)?;
        Ok(SymbolGetServiceRequest {
            snapshot_sha256: self.snapshot_sha256,
            generation: self.generation,
            path: self.path,
            content_sha256: self.content_sha256,
            artifact_sha256: self.artifact_sha256,
            fact_ordinal: self.fact_ordinal,
            timeout,
        })
    }
}

/// Validated, owned exact-symbol request passed to the composition root.
pub struct SymbolGetServiceRequest {
    snapshot_sha256: String,
    generation: i64,
    path: String,
    content_sha256: String,
    artifact_sha256: String,
    fact_ordinal: u64,
    timeout: Duration,
}

impl SymbolGetServiceRequest {
    /// Returns the exact snapshot SHA-256 text.
    #[must_use]
    pub fn snapshot_sha256(&self) -> &str {
        &self.snapshot_sha256
    }

    /// Returns the exact positive generation identifier.
    #[must_use]
    pub const fn generation(&self) -> i64 {
        self.generation
    }

    /// Returns the canonical exact repository path.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Returns the exact source-content SHA-256 text.
    #[must_use]
    pub fn content_sha256(&self) -> &str {
        &self.content_sha256
    }

    /// Returns the exact analysis-artifact SHA-256 text.
    #[must_use]
    pub fn artifact_sha256(&self) -> &str {
        &self.artifact_sha256
    }

    /// Returns the exact generation-local fact ordinal.
    #[must_use]
    pub const fn fact_ordinal(&self) -> u64 {
        self.fact_ordinal
    }

    /// Returns the remaining end-to-end deadline duration.
    #[must_use]
    pub const fn timeout(&self) -> Duration {
        self.timeout
    }

    pub(crate) const fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

impl fmt::Debug for SymbolGetServiceRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SymbolGetServiceRequest")
            .field("snapshot_sha256", &"<redacted-digest>")
            .field("generation", &self.generation)
            .field("path", &"<redacted-path>")
            .field("content_sha256", &"<redacted-digest>")
            .field("artifact_sha256", &"<redacted-digest>")
            .field("fact_ordinal", &self.fact_ordinal)
            .field("timeout", &self.timeout)
            .finish()
    }
}

/// Stable categorical failure returned by the injected repository service.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RepositoryServiceError {
    /// Local code search failed without a usable result.
    CodeSearch,
    /// Exact symbol retrieval failed without a usable result.
    SymbolGet,
}

impl fmt::Display for RepositoryServiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::CodeSearch => "code search failed",
            Self::SymbolGet => "symbol retrieval failed",
        })
    }
}

impl std::error::Error for RepositoryServiceError {}

/// Synchronous repository operations injected by the CLI composition root.
///
/// Implementations must honor both the request timeout and cancellation flag.
/// They must return only bounded output DTOs and stable, redacted errors.
pub trait RepositoryService: Send + Sync + 'static {
    /// Runs one bounded lexical search.
    fn code_search(
        &self,
        request: CodeSearchServiceRequest,
        cancelled: Arc<AtomicBool>,
    ) -> Result<CodeSearchOutput, RepositoryServiceError>;

    /// Retrieves one exact, verified source declaration.
    fn symbol_get(
        &self,
        request: SymbolGetServiceRequest,
        cancelled: Arc<AtomicBool>,
    ) -> Result<SymbolGetOutput, RepositoryServiceError>;
}

/// Versioned categorical coverage counts.
#[derive(Clone, Copy, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpCoverage {
    /// Work completed within the selected scope.
    pub searched: u64,
    /// Work intentionally omitted by the index profile.
    pub skipped: u64,
    /// Work that could not be resolved.
    pub unresolved: u64,
    /// Work omitted by an explicit result bound.
    pub truncated: u64,
}

/// Versioned half-open byte span.
#[derive(Clone, Copy, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpSpan {
    /// Inclusive starting byte offset.
    pub start: u64,
    /// Exclusive ending byte offset.
    pub end: u64,
}

/// One attributed Rust-symbol match in a `code_search` response.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpSearchMatch {
    /// Canonical byte-preserving repository path.
    pub path: String,
    /// Exact generation-local fact ordinal.
    pub fact_ordinal: u64,
    /// Exact source-content SHA-256.
    pub content_sha256: String,
    /// Exact analysis-artifact SHA-256.
    pub artifact_sha256: String,
    /// Producer-manifest SHA-256.
    pub producer_manifest_sha256: String,
    /// Evidence strength; currently `syntax`.
    pub evidence_tier: String,
    /// Rust declaration kind.
    pub kind: String,
    /// Unqualified declaration name.
    pub name: String,
    /// Deterministic lexical qualified name.
    pub qualified_name: String,
    /// Exact declaration-name byte span.
    pub name_span: McpSpan,
    /// Exact complete declaration byte span.
    pub declaration_span: McpSpan,
}

/// Version-1 structured response for `code_search`.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CodeSearchOutput {
    /// Wire schema version.
    pub schema_version: u16,
    /// Search-profile version.
    pub query_profile: u16,
    /// Concrete source snapshot SHA-256.
    pub snapshot_sha256: String,
    /// Opaque active-generation identifier.
    pub generation: i64,
    /// Categorical material-result resolution.
    pub resolution: String,
    /// Domain-separated canonical query SHA-256.
    pub query_sha256: String,
    /// Number of returned matches.
    pub matches_returned: u64,
    /// Exact number of matches before result truncation.
    pub matches_total: u64,
    /// Explicit coverage categories.
    pub coverage: McpCoverage,
    /// Explicit result limitation.
    pub limitation: String,
    /// Deterministically ordered attributed matches.
    pub matches: Vec<McpSearchMatch>,
}

/// Exact selector echoed in a `symbol_get` response.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SymbolSelectorOutput {
    /// Canonical byte-preserving repository path.
    pub path: String,
    /// Exact source-content SHA-256.
    pub content_sha256: String,
    /// Exact analysis-artifact SHA-256.
    pub artifact_sha256: String,
    /// Exact generation-local fact ordinal.
    pub fact_ordinal: u64,
}

/// One exact verified Rust declaration.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpSymbol {
    /// Producer-manifest SHA-256.
    pub producer_manifest_sha256: String,
    /// Evidence strength; currently `syntax`.
    pub evidence_tier: String,
    /// Rust declaration kind.
    pub kind: String,
    /// Unqualified declaration name.
    pub name: String,
    /// Deterministic lexical qualified name.
    pub qualified_name: String,
    /// Exact declaration-name byte span.
    pub name_span: McpSpan,
    /// Exact complete declaration byte span.
    pub declaration_span: McpSpan,
    /// Safe source-byte representation.
    pub declaration_encoding: String,
    /// Exact declaration bytes encoded as lowercase hexadecimal.
    pub declaration_hex: String,
}

/// Version-1 structured response for `symbol_get`.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SymbolGetOutput {
    /// Wire schema version.
    pub schema_version: u16,
    /// Exact-symbol profile version.
    pub symbol_profile: u16,
    /// Concrete source snapshot SHA-256.
    pub snapshot_sha256: String,
    /// Opaque active-generation identifier.
    pub generation: i64,
    /// Categorical material-result resolution.
    pub resolution: String,
    /// Exact requested occurrence selector.
    pub selector: SymbolSelectorOutput,
    /// Explicit coverage categories.
    pub coverage: McpCoverage,
    /// Explicit result limitation.
    pub limitation: String,
    /// Exact verified symbol, or `null` for an unresolved occurrence.
    pub symbol: Option<McpSymbol>,
}

fn validate_timeout(timeout_ms: Option<u64>) -> Result<Duration, &'static str> {
    let timeout_ms = timeout_ms.unwrap_or(DEFAULT_MCP_TIMEOUT_MS);
    if !(1..=MAX_MCP_TIMEOUT_MS).contains(&timeout_ms) {
        return Err("timeout_ms must be between 1 and 30000");
    }
    Ok(Duration::from_millis(timeout_ms))
}

fn is_lowercase_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn is_canonical_path_text(value: &str) -> bool {
    let Some(encoded) = value.strip_prefix("rwp1:h:") else {
        return false;
    };
    !encoded.is_empty()
        && value.len() <= MAX_PATH_TEXT_BYTES
        && encoded.len().is_multiple_of(2)
        && encoded
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'A'..=b'F').contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inputs_are_bounded_canonical_and_redacted() {
        let search: CodeSearchInput =
            serde_json::from_str(r#"{"query":"  alpha   beta  "}"#).expect("valid input");
        let request = search.validate().expect("valid request");
        assert_eq!(request.query(), "alpha beta");
        assert_eq!(request.max_results(), DEFAULT_CODE_SEARCH_RESULTS);
        assert_eq!(
            format!("{request:?}"),
            "CodeSearchServiceRequest { query: \"<redacted-query>\", max_results: 20, timeout: 5s }"
        );

        let digest = "ab".repeat(32);
        let symbol: SymbolGetInput = serde_json::from_value(serde_json::json!({
            "snapshot_sha256": digest,
            "generation": 7,
            "path": "rwp1:h:7372632F6C69622E7273",
            "content_sha256": "cd".repeat(32),
            "artifact_sha256": "ef".repeat(32),
            "fact_ordinal": 3,
        }))
        .expect("valid input");
        let request = symbol.validate().expect("valid selector");
        assert_eq!(request.generation(), 7);
        assert_eq!(request.fact_ordinal(), 3);
        let debug = format!("{request:?}");
        assert!(!debug.contains("737263"));
        assert!(!debug.contains(&digest));
    }

    #[test]
    fn unknown_fields_and_invalid_bounds_fail_before_service_construction() {
        assert!(
            serde_json::from_str::<CodeSearchInput>(r#"{"query":"run","repository":"/private"}"#)
                .is_err()
        );
        for value in [
            serde_json::json!({"query": ""}),
            serde_json::json!({"query": "run", "max_results": 0}),
            serde_json::json!({"query": "run", "max_results": 101}),
            serde_json::json!({"query": "run", "timeout_ms": 0}),
            serde_json::json!({"query": "run", "timeout_ms": 30001}),
        ] {
            let input: CodeSearchInput = serde_json::from_value(value).expect("wire shape");
            assert!(input.validate().is_err());
        }
    }

    #[test]
    fn symbol_selector_rejects_noncanonical_text() {
        let valid = || SymbolGetInput {
            snapshot_sha256: "11".repeat(32),
            generation: 1,
            path: "rwp1:h:7372632F6C69622E7273".to_owned(),
            content_sha256: "22".repeat(32),
            artifact_sha256: "aa".repeat(32),
            fact_ordinal: 0,
            timeout_ms: None,
        };

        let mut input = valid();
        input.generation = 0;
        assert!(input.validate().is_err());
        let mut input = valid();
        input.snapshot_sha256 = "AA".repeat(32);
        assert!(input.validate().is_err());
        let mut input = valid();
        input.path = "rwp1:h:aa".to_owned();
        assert!(input.validate().is_err());
        let mut input = valid();
        input.path = "rwp1:h:A".to_owned();
        assert!(input.validate().is_err());
    }
}
