//! Versioned wire types for evidence-backed typed declaration discovery.

use std::{fmt, time::Duration};

use repowitness_application::{
    DEFAULT_CODE_SEARCH_RESULTS, MAX_CODE_SEARCH_RESULTS, RustSymbolKind, SourceLanguage,
    SymbolSearchNameMatch, SymbolSearchQuery,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{McpCoverage, McpSearchMatch, validate_timeout};

/// Native tool name for typed direct-declaration discovery.
pub const SYMBOL_SEARCH_TOOL_NAME: &str = "symbol_search";

/// Version-1 wire input for `symbol_search`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SymbolSearchInput {
    /// Exact declaration name or deterministic name prefix; never a regular expression.
    pub name: String,
    /// `exact` (default) or `prefix`.
    pub match_mode: Option<String>,
    /// Optional persisted adapter language: `rust`, `go`, `typescript`, `tsx`, or `python`.
    pub language: Option<String>,
    /// Optional persisted direct declaration kind.
    pub kind: Option<String>,
    /// Optional repository-relative byte path prefix, for example `crates/`.
    pub path_prefix: Option<String>,
    /// Maximum returned declaration receipts, from 1 through 100.
    pub max_results: Option<u16>,
    /// End-to-end operation deadline in milliseconds.
    pub timeout_ms: Option<u64>,
}

impl fmt::Debug for SymbolSearchInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SymbolSearchInput")
            .field("name", &"<redacted-symbol>")
            .field("match_mode", &self.match_mode)
            .field("language", &self.language)
            .field("kind", &self.kind)
            .field(
                "path_prefix",
                &self.path_prefix.as_ref().map(|_| "<redacted-path>"),
            )
            .field("max_results", &self.max_results)
            .field("timeout_ms", &self.timeout_ms)
            .finish()
    }
}

impl SymbolSearchInput {
    pub(crate) fn validate(self) -> Result<SymbolSearchServiceRequest, &'static str> {
        let match_mode = match self.match_mode.as_deref().unwrap_or("exact") {
            "exact" => SymbolSearchNameMatch::Exact,
            "prefix" => SymbolSearchNameMatch::Prefix,
            _ => return Err("match_mode must be exact or prefix"),
        };
        let language = match self.language.as_deref() {
            Some(value) => SourceLanguage::from_stable_str(value)
                .map(Some)
                .ok_or("language must be rust, go, typescript, tsx, or python")?,
            None => None,
        };
        let kind = match self.kind.as_deref() {
            Some(value) => RustSymbolKind::from_stable_str(value)
                .map(Some)
                .ok_or("kind must be a supported direct declaration kind")?,
            None => None,
        };
        SymbolSearchQuery::try_new_with_filters(
            &self.name,
            match_mode,
            language,
            kind,
            self.path_prefix.as_deref(),
        )
        .map_err(|_| "symbol selector does not satisfy the bounded typed discovery profile")?;
        let max_results = self.max_results.unwrap_or(DEFAULT_CODE_SEARCH_RESULTS);
        if !(1..=MAX_CODE_SEARCH_RESULTS).contains(&max_results) {
            return Err("max_results must be between 1 and 100");
        }
        Ok(SymbolSearchServiceRequest {
            name: self.name,
            match_mode,
            language,
            kind,
            path_prefix: self.path_prefix,
            max_results,
            timeout: validate_timeout(self.timeout_ms)?,
        })
    }
}

/// Validated, owned typed declaration request passed to the composition root.
pub struct SymbolSearchServiceRequest {
    name: String,
    match_mode: SymbolSearchNameMatch,
    language: Option<SourceLanguage>,
    kind: Option<RustSymbolKind>,
    path_prefix: Option<String>,
    max_results: u16,
    timeout: Duration,
}

impl SymbolSearchServiceRequest {
    /// Returns the admitted declaration-name selector.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Returns exact or prefix selection semantics.
    #[must_use]
    pub const fn match_mode(&self) -> SymbolSearchNameMatch {
        self.match_mode
    }
    /// Returns the optional persisted language filter.
    #[must_use]
    pub const fn language(&self) -> Option<SourceLanguage> {
        self.language
    }
    /// Returns the optional persisted declaration-kind filter.
    #[must_use]
    pub const fn kind(&self) -> Option<RustSymbolKind> {
        self.kind
    }
    /// Returns the optional validated repository-relative path prefix.
    #[must_use]
    pub fn path_prefix(&self) -> Option<&str> {
        self.path_prefix.as_deref()
    }
    /// Returns the inclusive receipt limit.
    #[must_use]
    pub const fn max_results(&self) -> u16 {
        self.max_results
    }
    /// Returns the remaining operation deadline.
    #[must_use]
    pub const fn timeout(&self) -> Duration {
        self.timeout
    }
    pub(crate) const fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

impl fmt::Debug for SymbolSearchServiceRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SymbolSearchServiceRequest")
            .field("name", &"<redacted-symbol>")
            .field("match_mode", &self.match_mode)
            .field("language", &self.language)
            .field("kind", &self.kind)
            .field(
                "path_prefix",
                &self.path_prefix.as_ref().map(|_| "<redacted-path>"),
            )
            .field("max_results", &self.max_results)
            .field("timeout", &self.timeout)
            .finish()
    }
}

/// Version-1 structured response for `symbol_search`.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SymbolSearchOutput {
    /// Wire schema version.
    pub schema_version: u16,
    /// Typed declaration profile version.
    pub query_profile: u16,
    /// Exact selected connected-workspace identity.
    pub connected_workspace: String,
    /// Exact immutable workspace view used to select the source slot.
    pub workspace_view: i64,
    /// Exact selected source-slot identity.
    pub source_slot: String,
    /// Concrete source snapshot SHA-256.
    pub snapshot_sha256: String,
    /// Opaque active-generation identifier.
    pub generation: i64,
    /// Categorical material-result resolution.
    pub resolution: String,
    /// Domain-separated admitted selector SHA-256.
    pub query_sha256: String,
    /// Exact/prefix selector mode retained in the claim profile.
    pub match_mode: String,
    /// Number of returned declaration receipts.
    pub matches_returned: u64,
    /// Exact number of matched declarations before result truncation.
    pub matches_total: u64,
    /// Explicit coverage categories.
    pub coverage: McpCoverage,
    /// Explicit scope limitations in stable order.
    pub limitations: Vec<String>,
    /// Deterministically ordered attributed direct declaration receipts.
    pub matches: Vec<McpSearchMatch>,
}

#[cfg(test)]
mod tests {
    use super::SymbolSearchInput;

    #[test]
    fn input_is_bounded_typed_and_redacted() {
        let request = SymbolSearchInput {
            name: "private_symbol".to_owned(),
            match_mode: Some("prefix".to_owned()),
            language: Some("typescript".to_owned()),
            kind: Some("function".to_owned()),
            path_prefix: Some("web".to_owned()),
            max_results: Some(100),
            timeout_ms: Some(1),
        }
        .validate()
        .expect("bounded selector should be accepted");
        assert_eq!(request.name(), "private_symbol");
        assert_eq!(request.match_mode().as_str(), "prefix");
        assert_eq!(
            request.language().map(|language| language.as_str()),
            Some("typescript")
        );
        assert_eq!(request.kind().map(|kind| kind.as_str()), Some("function"));
        assert_eq!(request.path_prefix(), Some("web"));
        assert_eq!(request.max_results(), 100);
        let debug = format!("{request:?}");
        assert!(!debug.contains("private_symbol"));
        assert!(!debug.contains("web"));
    }

    #[test]
    fn invalid_modes_filters_and_path_prefix_fail_before_service_access() {
        for input in [
            SymbolSearchInput {
                name: "run".to_owned(),
                match_mode: Some("regex".to_owned()),
                language: None,
                kind: None,
                path_prefix: None,
                max_results: None,
                timeout_ms: None,
            },
            SymbolSearchInput {
                name: "run".to_owned(),
                match_mode: None,
                language: Some("java".to_owned()),
                kind: None,
                path_prefix: None,
                max_results: None,
                timeout_ms: None,
            },
            SymbolSearchInput {
                name: "run".to_owned(),
                match_mode: None,
                language: None,
                kind: Some("route".to_owned()),
                path_prefix: None,
                max_results: None,
                timeout_ms: None,
            },
            SymbolSearchInput {
                name: "run".to_owned(),
                match_mode: None,
                language: None,
                kind: None,
                path_prefix: Some("../private".to_owned()),
                max_results: None,
                timeout_ms: None,
            },
        ] {
            assert!(input.validate().is_err());
        }
    }
}
