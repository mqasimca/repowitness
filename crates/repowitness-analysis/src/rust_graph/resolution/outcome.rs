use super::model::{
    RUST_GRAPH_RESOLVER_PROFILE_VERSION, RustGraphDefinitionIdentity, RustGraphSiteIdentity,
};

/// Attributed evidence supporting one candidate relationship.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum RustGraphResolutionEvidence {
    /// Exact qualified spelling under the local resolver's syntax rules.
    QualifiedSyntax,
    /// Exact nearest lexical qualified path under the local syntax model.
    LexicalSyntax,
    /// Exact target of one supported simple import or alias.
    ImportSyntax,
    /// Incomplete exact-name matching only; never semantic proof.
    ExactNameHeuristic,
}

impl RustGraphResolutionEvidence {
    /// Returns the stable categorical spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::QualifiedSyntax => "qualified_syntax",
            Self::LexicalSyntax => "lexical_syntax",
            Self::ImportSyntax => "import_syntax",
            Self::ExactNameHeuristic => "exact_name_heuristic",
        }
    }
}

/// Why complete bounded resolution produced no candidates.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RustGraphUnresolvedReason {
    /// No candidate matched a supported rule.
    NoCandidate,
    /// Macro and test-marker sites require another evidence provider.
    UnsupportedSiteKind,
    /// The import uses a nested, glob, relative, or otherwise unsupported form.
    UnsupportedImportShape,
    /// A method, field, UFCS, closure, or other dynamic call was not emulated.
    DynamicOrMethodCall,
    /// The qualified spelling is outside the conservative local path grammar.
    UnsupportedQualifiedSyntax,
}

impl RustGraphUnresolvedReason {
    /// Returns the stable categorical spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NoCandidate => "no_candidate",
            Self::UnsupportedSiteKind => "unsupported_site_kind",
            Self::UnsupportedImportShape => "unsupported_import_shape",
            Self::DynamicOrMethodCall => "dynamic_or_method_call",
            Self::UnsupportedQualifiedSyntax => "unsupported_qualified_syntax",
        }
    }
}

/// One exact candidate with its non-upgraded evidence class.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RustGraphResolutionCandidate {
    target: RustGraphDefinitionIdentity,
    evidence: RustGraphResolutionEvidence,
}

impl RustGraphResolutionCandidate {
    pub(super) const fn new(
        target: RustGraphDefinitionIdentity,
        evidence: RustGraphResolutionEvidence,
    ) -> Self {
        Self { target, evidence }
    }

    /// Returns the exact target identity.
    #[must_use]
    pub const fn target(&self) -> &RustGraphDefinitionIdentity {
        &self.target
    }

    /// Returns the attributed resolution evidence.
    #[must_use]
    pub const fn evidence(&self) -> RustGraphResolutionEvidence {
        self.evidence
    }
}

/// Categorical zero, one, or many result for one site.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RustGraphResolutionOutcome {
    /// Complete supported work found no candidate.
    Unresolved {
        /// Stable abstention or miss reason.
        reason: RustGraphUnresolvedReason,
    },
    /// Exactly one candidate remains, without upgrading its evidence.
    Unique {
        /// The sole candidate.
        candidate: RustGraphResolutionCandidate,
    },
    /// Two or more deterministically ordered candidates remain.
    Ambiguous {
        /// At least two retained candidates.
        candidates: Vec<RustGraphResolutionCandidate>,
    },
}

/// Complete result for one exact site.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RustGraphSiteResolution {
    site: RustGraphSiteIdentity,
    outcome: RustGraphResolutionOutcome,
    candidate_count: u32,
    candidates_truncated: bool,
}

impl RustGraphSiteResolution {
    pub(super) const fn new(
        site: RustGraphSiteIdentity,
        outcome: RustGraphResolutionOutcome,
        candidate_count: u32,
        candidates_truncated: bool,
    ) -> Self {
        Self {
            site,
            outcome,
            candidate_count,
            candidates_truncated,
        }
    }

    /// Returns the exact originating site identity.
    #[must_use]
    pub const fn site(&self) -> &RustGraphSiteIdentity {
        &self.site
    }

    /// Returns the categorical outcome.
    #[must_use]
    pub const fn outcome(&self) -> &RustGraphResolutionOutcome {
        &self.outcome
    }

    /// Returns the complete candidate count before deterministic truncation.
    #[must_use]
    pub const fn candidate_count(&self) -> u32 {
        self.candidate_count
    }

    /// Reports whether the retained candidate vector was truncated.
    #[must_use]
    pub const fn candidates_truncated(&self) -> bool {
        self.candidates_truncated
    }
}

/// Exact coverage counters for one complete resolver run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RustGraphResolutionCoverage {
    definitions: u32,
    sites: u32,
    unresolved: u32,
    unique: u32,
    ambiguous: u32,
    unsupported: u32,
    truncated_sites: u32,
    retained_candidates: u64,
}

impl RustGraphResolutionCoverage {
    #[allow(
        clippy::too_many_arguments,
        reason = "all fixed coverage fields are semantic"
    )]
    pub(super) const fn new(
        definitions: u32,
        sites: u32,
        unresolved: u32,
        unique: u32,
        ambiguous: u32,
        unsupported: u32,
        truncated_sites: u32,
        retained_candidates: u64,
    ) -> Self {
        Self {
            definitions,
            sites,
            unresolved,
            unique,
            ambiguous,
            unsupported,
            truncated_sites,
            retained_candidates,
        }
    }

    /// Returns admitted definition occurrences.
    #[must_use]
    pub const fn definitions(self) -> u32 {
        self.definitions
    }

    /// Returns admitted site occurrences.
    #[must_use]
    pub const fn sites(self) -> u32 {
        self.sites
    }

    /// Returns unresolved outcomes.
    #[must_use]
    pub const fn unresolved(self) -> u32 {
        self.unresolved
    }

    /// Returns unique outcomes.
    #[must_use]
    pub const fn unique(self) -> u32 {
        self.unique
    }

    /// Returns ambiguous outcomes.
    #[must_use]
    pub const fn ambiguous(self) -> u32 {
        self.ambiguous
    }

    /// Returns explicit unsupported/abstaining outcomes.
    #[must_use]
    pub const fn unsupported(self) -> u32 {
        self.unsupported
    }

    /// Returns outcomes whose candidate vector was truncated.
    #[must_use]
    pub const fn truncated_sites(self) -> u32 {
        self.truncated_sites
    }

    /// Returns candidates retained in the immutable output.
    #[must_use]
    pub const fn retained_candidates(self) -> u64 {
        self.retained_candidates
    }
}

/// Complete deterministic output for one immutable generation input.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RustGraphResolution {
    outcomes: Vec<RustGraphSiteResolution>,
    coverage: RustGraphResolutionCoverage,
    input_text_bytes: u64,
    output_bytes: u64,
}

impl RustGraphResolution {
    pub(super) const fn new(
        outcomes: Vec<RustGraphSiteResolution>,
        coverage: RustGraphResolutionCoverage,
        input_text_bytes: u64,
        output_bytes: u64,
    ) -> Self {
        Self {
            outcomes,
            coverage,
            input_text_bytes,
            output_bytes,
        }
    }

    /// Returns the resolver profile used for every outcome.
    #[must_use]
    pub const fn profile_version(&self) -> u32 {
        RUST_GRAPH_RESOLVER_PROFILE_VERSION
    }

    /// Returns outcomes in deterministic exact-site order.
    #[must_use]
    pub fn outcomes(&self) -> &[RustGraphSiteResolution] {
        &self.outcomes
    }

    /// Returns exact run coverage.
    #[must_use]
    pub const fn coverage(&self) -> RustGraphResolutionCoverage {
        self.coverage
    }

    /// Returns aggregate admitted path, target, and descriptor text bytes.
    #[must_use]
    pub const fn input_text_bytes(&self) -> u64 {
        self.input_text_bytes
    }

    /// Returns fixed-width identity and variable-path output budget bytes.
    #[must_use]
    pub const fn output_bytes(&self) -> u64 {
        self.output_bytes
    }
}
