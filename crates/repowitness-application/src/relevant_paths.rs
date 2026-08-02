//! Deterministic path-level navigation over bounded lexical declaration evidence.
//!
//! This module deliberately projects a completed [`CodeSearchResult`] instead
//! of performing another search. The projection therefore cannot mix
//! generations, makes no new semantic claim, and preserves the complete
//! evidence and coverage receipt that established each path match.

use std::{
    collections::{BTreeMap, btree_map::Entry},
    error::Error,
    fmt,
};

use repowitness_domain::{
    BoundedResultItems, EvidenceLocation, RepositoryPath, ResultItemLimit, ResultItemsError,
    SourceContentDigest,
};

use crate::{CodeSearchResult, MAX_CODE_SEARCH_RESULTS};

/// Version of the deterministic lexical path-presentation profile.
pub const RELEVANT_PATHS_PROFILE_VERSION: u16 = 1;
/// Default maximum number of paths returned by the path navigator.
pub const DEFAULT_RELEVANT_PATHS: u16 = 12;
/// Hard Phase 0 ceiling for returned paths.
pub const MAX_RELEVANT_PATHS: u16 = 50;
/// Returned declaration candidates budgeted for each requested path.
///
/// Path navigation groups already-returned lexical declaration evidence. A
/// small multiple gives one path more than one opportunity to be represented
/// while retaining the code-search hard ceiling.
pub const RELEVANT_PATHS_CANDIDATES_PER_PATH: u16 = 4;

/// Stable failure to construct a bounded path result limit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RelevantPathsLimitError;

impl fmt::Display for RelevantPathsLimitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("relevant-path limit is zero or exceeds the supported ceiling")
    }
}

impl Error for RelevantPathsLimitError {}

/// Inclusive bound for paths projected from one lexical evidence receipt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RelevantPathsLimits {
    max_paths: u16,
}

impl RelevantPathsLimits {
    /// Constructs an inclusive path-output bound.
    pub const fn try_new(max_paths: u16) -> Result<Self, RelevantPathsLimitError> {
        if max_paths == 0 || max_paths > MAX_RELEVANT_PATHS {
            return Err(RelevantPathsLimitError);
        }
        Ok(Self { max_paths })
    }

    /// Returns the inclusive path-output bound.
    #[must_use]
    pub const fn max_paths(self) -> u16 {
        self.max_paths
    }

    /// Returns the bounded declaration-candidate surface used for this path limit.
    ///
    /// This is deliberately a candidate limit, not a claim about the number of
    /// matching paths. The embedded lexical receipt remains authoritative for
    /// candidate truncation and total-match coverage.
    #[must_use]
    pub const fn candidate_limit(self) -> u16 {
        let candidate_limit = self
            .max_paths
            .saturating_mul(RELEVANT_PATHS_CANDIDATES_PER_PATH);
        if candidate_limit > MAX_CODE_SEARCH_RESULTS {
            MAX_CODE_SEARCH_RESULTS
        } else {
            candidate_limit
        }
    }
}

impl Default for RelevantPathsLimits {
    fn default() -> Self {
        Self {
            max_paths: DEFAULT_RELEVANT_PATHS,
        }
    }
}

/// One canonical path and its direct lexical-declaration evidence count.
#[derive(Clone, Eq, PartialEq)]
pub struct RelevantPath {
    path: RepositoryPath,
    content_digest: SourceContentDigest,
    matching_declarations: u16,
    first_fact_ordinal: u64,
}

impl RelevantPath {
    /// Returns the exact canonical repository path from the source receipt.
    #[must_use]
    pub const fn path(&self) -> &RepositoryPath {
        &self.path
    }

    /// Returns the exact content digest shared by this path's matched facts.
    #[must_use]
    pub const fn content_digest(&self) -> SourceContentDigest {
        self.content_digest
    }

    /// Returns the number of returned declarations that matched this path.
    #[must_use]
    pub const fn matching_declarations(&self) -> u16 {
        self.matching_declarations
    }

    /// Returns the smallest matching fact ordinal in this path.
    #[must_use]
    pub const fn first_fact_ordinal(&self) -> u64 {
        self.first_fact_ordinal
    }
}

impl fmt::Debug for RelevantPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RelevantPath")
            .field("path", &"<repository-path>")
            .field("content_digest", &self.content_digest)
            .field("matching_declarations", &self.matching_declarations)
            .field("first_fact_ordinal", &self.first_fact_ordinal)
            .finish()
    }
}

/// The immutable source-search receipt plus its bounded path projection.
#[derive(Debug, Eq, PartialEq)]
pub struct RelevantPathsResult<G> {
    search: CodeSearchResult<G>,
    paths: BoundedResultItems<RelevantPath>,
    returned_match_paths_total: u64,
    returned_match_paths_truncated: bool,
}

impl<G> RelevantPathsResult<G> {
    /// Returns the complete evidence-bearing lexical search receipt.
    #[must_use]
    pub const fn search(&self) -> &CodeSearchResult<G> {
        &self.search
    }

    /// Returns paths ordered by returned exact-match count, then canonical path.
    #[must_use]
    pub const fn paths(&self) -> &BoundedResultItems<RelevantPath> {
        &self.paths
    }

    /// Returns the exact number of unique paths among returned declaration matches.
    ///
    /// This does not count paths that may occur only in candidates omitted by
    /// the underlying bounded lexical search.
    #[must_use]
    pub const fn returned_match_paths_total(&self) -> u64 {
        self.returned_match_paths_total
    }

    /// Returns whether the path limit omitted paths from the returned-match surface.
    #[must_use]
    pub const fn returned_match_paths_truncated(&self) -> bool {
        self.returned_match_paths_truncated
    }

    /// Decomposes the immutable search receipt and its path projection for an adapter.
    pub fn into_parts(
        self,
    ) -> (
        CodeSearchResult<G>,
        BoundedResultItems<RelevantPath>,
        u64,
        bool,
    ) {
        (
            self.search,
            self.paths,
            self.returned_match_paths_total,
            self.returned_match_paths_truncated,
        )
    }
}

/// Stable failure while deriving a path-level view from syntax evidence.
#[derive(Debug)]
pub enum RelevantPathsError {
    /// The supplied evidence did not contain a syntax occurrence.
    InvalidEvidenceLocation,
    /// One path carried conflicting content identities in one immutable result.
    InconsistentPathContent,
    /// A bounded declaration count could not be represented in the profile.
    MatchingDeclarationsNotRepresentable,
    /// The unique path count could not be represented in the profile.
    PathCountNotRepresentable,
    /// The bounded path collection could not be represented.
    ResultItems(ResultItemsError),
}

impl fmt::Display for RelevantPathsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidEvidenceLocation => "code-search evidence location is invalid",
            Self::InconsistentPathContent => {
                "code-search evidence contains inconsistent content for one path"
            }
            Self::MatchingDeclarationsNotRepresentable => {
                "path matching-declaration count cannot be represented safely"
            }
            Self::PathCountNotRepresentable => "relevant-path count cannot be represented safely",
            Self::ResultItems(error) => return error.fmt(formatter),
        })
    }
}

impl Error for RelevantPathsError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::ResultItems(error) => Some(error),
            Self::InvalidEvidenceLocation
            | Self::InconsistentPathContent
            | Self::MatchingDeclarationsNotRepresentable
            | Self::PathCountNotRepresentable => None,
        }
    }
}

/// Groups a completed lexical search receipt into directly supported source paths.
///
/// A path is not a semantic recommendation. It is ordered solely by the count
/// of already-returned exact declaration matches, then by canonical path. The
/// embedded search receipt remains authoritative for evidence, source snapshot,
/// generation, coverage, and any candidate truncation.
pub fn locate_relevant_paths<G>(
    search: CodeSearchResult<G>,
    limits: RelevantPathsLimits,
) -> Result<RelevantPathsResult<G>, RelevantPathsError> {
    let mut aggregates = BTreeMap::<RepositoryPath, RelevantPathAggregate>::new();
    for evidence in search.evidence().as_slice() {
        let EvidenceLocation::SymbolOccurrence(occurrence) = evidence.identity().location() else {
            return Err(RelevantPathsError::InvalidEvidenceLocation);
        };
        let path = evidence.identity().path();
        let content_digest = *evidence.identity().content_digest();
        match aggregates.entry(path.clone()) {
            Entry::Vacant(entry) => {
                entry.insert(RelevantPathAggregate {
                    content_digest,
                    matching_declarations: 1,
                    first_fact_ordinal: occurrence.fact_ordinal(),
                });
            }
            Entry::Occupied(mut entry) => {
                let aggregate = entry.get_mut();
                if aggregate.content_digest != content_digest {
                    return Err(RelevantPathsError::InconsistentPathContent);
                }
                aggregate.matching_declarations = aggregate
                    .matching_declarations
                    .checked_add(1)
                    .ok_or(RelevantPathsError::MatchingDeclarationsNotRepresentable)?;
                aggregate.first_fact_ordinal =
                    aggregate.first_fact_ordinal.min(occurrence.fact_ordinal());
            }
        }
    }

    let returned_match_paths_total = u64::try_from(aggregates.len())
        .map_err(|_| RelevantPathsError::PathCountNotRepresentable)?;
    let mut paths = aggregates
        .into_iter()
        .map(|(path, aggregate)| RelevantPath {
            path,
            content_digest: aggregate.content_digest,
            matching_declarations: aggregate.matching_declarations,
            first_fact_ordinal: aggregate.first_fact_ordinal,
        })
        .collect::<Vec<_>>();
    paths.sort_unstable_by(|left, right| {
        right
            .matching_declarations
            .cmp(&left.matching_declarations)
            .then_with(|| left.path.cmp(&right.path))
    });
    let returned_match_paths_truncated = returned_match_paths_total > u64::from(limits.max_paths());
    paths.truncate(usize::from(limits.max_paths()));
    let paths = BoundedResultItems::try_from_vec(
        paths,
        ResultItemLimit::new(u64::from(limits.max_paths())),
    )
    .map_err(RelevantPathsError::ResultItems)?;
    Ok(RelevantPathsResult {
        search,
        paths,
        returned_match_paths_total,
        returned_match_paths_truncated,
    })
}

struct RelevantPathAggregate {
    content_digest: SourceContentDigest,
    matching_declarations: u16,
    first_fact_ordinal: u64,
}
