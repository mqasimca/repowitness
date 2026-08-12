use std::{collections::BTreeSet, fmt, time::Duration};

use repowitness_application::{
    CodeSearchQuery, DEFAULT_CODE_SEARCH_RESULTS, MAX_CODE_SEARCH_RESULTS,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{CodeSearchOutput, CodeSearchServiceRequest, McpCoverage, validate_timeout};

/// MCP tool name for bounded FTI search across a local catalog.
pub const CROSS_REPOSITORY_SEARCH_TOOL_NAME: &str = "cross_repository_search";
/// Maximum repositories selected by one cross-repository search.
pub const MAX_CROSS_REPOSITORY_SELECTIONS: usize = 32;
/// Maximum total matches returned by one cross-repository search.
pub const MAX_CROSS_REPOSITORY_RESULTS: u16 = 100;

/// Version-1 catalog FTI search input.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CrossRepositorySearchInput {
    /// Literal terms passed to the existing FTI-backed code search.
    pub query: String,
    /// Exact catalog identities; omission searches every registered repository.
    pub repository_ids: Option<Vec<String>>,
    /// Maximum matches retained from each repository.
    pub max_results_per_repository: Option<u16>,
    /// Maximum matches retained across all repositories.
    pub max_results: Option<u16>,
    /// End-to-end deadline in milliseconds.
    pub timeout_ms: Option<u64>,
}

impl fmt::Debug for CrossRepositorySearchInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CrossRepositorySearchInput")
            .field("query", &"<redacted-query>")
            .field(
                "repository_ids",
                &self.repository_ids.as_ref().map(Vec::len),
            )
            .field(
                "max_results_per_repository",
                &self.max_results_per_repository,
            )
            .field("max_results", &self.max_results)
            .field("timeout_ms", &self.timeout_ms)
            .finish()
    }
}

impl CrossRepositorySearchInput {
    pub(crate) fn validate(self) -> Result<CrossRepositorySearchServiceRequest, &'static str> {
        CodeSearchQuery::try_new(&self.query)
            .map_err(|_| "query does not satisfy the bounded literal search profile")?;
        let repository_ids = match self.repository_ids {
            Some(ids) => {
                if ids.is_empty() || ids.len() > MAX_CROSS_REPOSITORY_SELECTIONS {
                    return Err("repository_ids must contain between 1 and 32 repositories");
                }
                if ids.iter().any(String::is_empty)
                    || ids.iter().collect::<BTreeSet<_>>().len() != ids.len()
                {
                    return Err("repository_ids must contain unique non-empty identities");
                }
                Some(ids)
            }
            None => None,
        };
        let max_results_per_repository = self
            .max_results_per_repository
            .unwrap_or(DEFAULT_CODE_SEARCH_RESULTS);
        if !(1..=MAX_CODE_SEARCH_RESULTS).contains(&max_results_per_repository) {
            return Err("max_results_per_repository must be between 1 and 100");
        }
        let max_results = self.max_results.unwrap_or(MAX_CROSS_REPOSITORY_RESULTS);
        if !(1..=MAX_CROSS_REPOSITORY_RESULTS).contains(&max_results) {
            return Err("max_results must be between 1 and 100");
        }
        Ok(CrossRepositorySearchServiceRequest {
            query: self.query,
            repository_ids,
            max_results_per_repository,
            max_results,
            timeout: validate_timeout(self.timeout_ms)?,
        })
    }
}

/// Validated catalog FTI search request.
#[derive(Clone)]
pub struct CrossRepositorySearchServiceRequest {
    query: String,
    repository_ids: Option<Vec<String>>,
    max_results_per_repository: u16,
    max_results: u16,
    timeout: Duration,
}

impl CrossRepositorySearchServiceRequest {
    /// Returns the bounded literal query.
    #[must_use]
    pub fn query(&self) -> &str {
        &self.query
    }

    /// Returns the optional exact catalog selection.
    #[must_use]
    pub fn repository_ids(&self) -> Option<&[String]> {
        self.repository_ids.as_deref()
    }

    /// Returns the per-repository result bound.
    #[must_use]
    pub const fn max_results_per_repository(&self) -> u16 {
        self.max_results_per_repository
    }

    /// Returns the aggregate result bound.
    #[must_use]
    pub const fn max_results(&self) -> u16 {
        self.max_results
    }

    /// Returns the end-to-end deadline.
    #[must_use]
    pub const fn timeout(&self) -> Duration {
        self.timeout
    }

    pub(crate) fn code_search_request(&self) -> CodeSearchServiceRequest {
        CodeSearchServiceRequest::new(
            self.query.clone(),
            self.max_results_per_repository,
            self.timeout,
        )
    }
}

/// Version-1 aggregate result for catalog FTI search.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CrossRepositorySearchOutput {
    /// Wire schema version.
    pub schema_version: u16,
    /// Existing FTI/code-search profile version.
    pub query_profile: u16,
    /// Overall categorical result resolution.
    pub resolution: String,
    /// Number of catalog repositories selected.
    pub repositories_requested: u64,
    /// Number of repositories returning a complete search result.
    pub repositories_completed: u64,
    /// Number of repositories that returned no usable result.
    pub repositories_failed: u64,
    /// Number of matches retained after the aggregate bound.
    pub matches_returned: u64,
    /// Number of matches reported before the aggregate bound.
    pub matches_total: u64,
    /// Aggregate search coverage.
    pub coverage: McpCoverage,
    /// Explicit FTI-only limitation.
    pub limitation: String,
    /// Deterministically ordered per-repository results.
    pub repositories: Vec<CrossRepositorySearchRepository>,
}

/// One repository's result or categorical failure.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CrossRepositorySearchRepository {
    /// Opaque catalog repository identity.
    pub repository_id: String,
    /// `complete` or `unavailable`.
    pub status: String,
    /// FTI result when the repository completed successfully.
    pub result: Option<CodeSearchOutput>,
}

#[cfg(test)]
mod tests {
    use super::{CrossRepositorySearchInput, MAX_CROSS_REPOSITORY_RESULTS};

    fn input() -> CrossRepositorySearchInput {
        CrossRepositorySearchInput {
            query: "run".to_owned(),
            repository_ids: None,
            max_results_per_repository: None,
            max_results: None,
            timeout_ms: None,
        }
    }

    #[test]
    fn defaults_are_bounded_and_explicit() {
        let request = input().validate().expect("valid input");
        assert_eq!(request.query(), "run");
        assert_eq!(request.max_results(), MAX_CROSS_REPOSITORY_RESULTS);
        assert!(request.repository_ids().is_none());
    }

    #[test]
    fn duplicate_selection_and_invalid_bounds_fail_before_fanout() {
        let mut duplicate = input();
        duplicate.repository_ids = Some(vec!["rwi1:h:1".to_owned(), "rwi1:h:1".to_owned()]);
        assert!(duplicate.validate().is_err());

        let mut too_many = input();
        too_many.max_results = Some(MAX_CROSS_REPOSITORY_RESULTS + 1);
        assert!(too_many.validate().is_err());
    }
}
