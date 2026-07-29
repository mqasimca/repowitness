use std::{error::Error, fmt};

use repowitness_analysis::{
    RustGraphEdgeKinds, RustGraphSiteKind, RustGraphTraceDirection, RustGraphTraceLimits,
    RustSymbolKind,
};
use repowitness_domain::{
    AnalysisArtifactDigest, ByteSpan, ConnectedWorkspaceId, RepositoryPath, SourceContentDigest,
    SourceSlotId,
};

/// Largest exact symbol query accepted at the application boundary.
pub const MAX_RUST_GRAPH_QUERY_BYTES: usize = 16_384;
const MAX_SYMBOL_NAME_BYTES: usize = 1_024;
const MAX_QUALIFIED_NAME_BYTES: usize = 4_096;

/// Active or exact immutable graph context selected by one request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RustGraphReadSelection {
    /// Resolve and pin the current published view once.
    Active {
        /// Expected connected workspace.
        connected_workspace: ConnectedWorkspaceId,
        /// Explicit source slot when the workspace has more than one member.
        source_slot: Option<SourceSlotId>,
    },
    /// Reopen one exact immutable view and graph-owning generation.
    Exact {
        /// Expected connected workspace.
        connected_workspace: ConnectedWorkspaceId,
        /// Positive database-local workspace-view identity.
        workspace_view: i64,
        /// Positive database-local graph-generation identity.
        graph_generation: i64,
        /// Explicit source slot when the workspace has more than one member.
        source_slot: Option<SourceSlotId>,
    },
}

impl RustGraphReadSelection {
    /// Selects the current immutable published view.
    #[must_use]
    pub const fn active(connected_workspace: ConnectedWorkspaceId) -> Self {
        Self::Active {
            connected_workspace,
            source_slot: None,
        }
    }

    /// Selects the current immutable view for one explicit source slot.
    #[must_use]
    pub const fn active_source_slot(
        connected_workspace: ConnectedWorkspaceId,
        source_slot: SourceSlotId,
    ) -> Self {
        Self::Active {
            connected_workspace,
            source_slot: Some(source_slot),
        }
    }

    /// Selects one exact immutable published view and graph generation.
    pub const fn exact(
        connected_workspace: ConnectedWorkspaceId,
        workspace_view: i64,
        graph_generation: i64,
    ) -> Result<Self, RustGraphSelectorError> {
        if workspace_view <= 0 || graph_generation <= 0 {
            return Err(RustGraphSelectorError::InvalidGeneration);
        }
        Ok(Self::Exact {
            connected_workspace,
            workspace_view,
            graph_generation,
            source_slot: None,
        })
    }

    /// Selects one exact immutable view, source slot, and graph generation.
    pub const fn exact_source_slot(
        connected_workspace: ConnectedWorkspaceId,
        source_slot: SourceSlotId,
        workspace_view: i64,
        graph_generation: i64,
    ) -> Result<Self, RustGraphSelectorError> {
        if workspace_view <= 0 || graph_generation <= 0 {
            return Err(RustGraphSelectorError::InvalidGeneration);
        }
        Ok(Self::Exact {
            connected_workspace,
            workspace_view,
            graph_generation,
            source_slot: Some(source_slot),
        })
    }

    /// Returns the expected connected workspace.
    #[must_use]
    pub const fn connected_workspace(self) -> ConnectedWorkspaceId {
        match self {
            Self::Active {
                connected_workspace,
                ..
            }
            | Self::Exact {
                connected_workspace,
                ..
            } => connected_workspace,
        }
    }

    /// Returns the exact requested view and generation, when supplied.
    #[must_use]
    pub const fn exact_pin(self) -> Option<(i64, i64)> {
        match self {
            Self::Active { .. } => None,
            Self::Exact {
                workspace_view,
                graph_generation,
                ..
            } => Some((workspace_view, graph_generation)),
        }
    }

    /// Returns the explicitly selected source slot, when one was supplied.
    #[must_use]
    pub const fn source_slot(self) -> Option<SourceSlotId> {
        match self {
            Self::Active { source_slot, .. } | Self::Exact { source_slot, .. } => source_slot,
        }
    }
}

/// Validated literal graph-symbol query.
#[derive(Clone, Eq, PartialEq)]
pub struct RustGraphSymbolQuery(String);

impl RustGraphSymbolQuery {
    /// Validates untrusted query text without interpreting query syntax.
    pub fn try_new(value: &str) -> Result<Self, RustGraphSymbolQueryError> {
        if value.is_empty() {
            return Err(RustGraphSymbolQueryError::Empty);
        }
        if value.len() > MAX_RUST_GRAPH_QUERY_BYTES {
            return Err(RustGraphSymbolQueryError::TooLong);
        }
        if value.chars().any(char::is_control) {
            return Err(RustGraphSymbolQueryError::InvalidCharacter);
        }
        Ok(Self(value.to_owned()))
    }

    /// Returns the exact admitted literal query.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for RustGraphSymbolQuery {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RustGraphSymbolQuery")
            .field("bytes", &self.0.len())
            .field("text", &"<redacted-query>")
            .finish()
    }
}

/// Stable graph-query validation failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RustGraphSymbolQueryError {
    /// No query text was supplied.
    Empty,
    /// The query exceeds the compiled byte ceiling.
    TooLong,
    /// The query contains a control character.
    InvalidCharacter,
}

impl fmt::Display for RustGraphSymbolQueryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Empty => "Rust graph symbol query must not be empty",
            Self::TooLong => "Rust graph symbol query exceeds the byte limit",
            Self::InvalidCharacter => "Rust graph symbol query contains an invalid character",
        })
    }
}

impl Error for RustGraphSymbolQueryError {}

/// Exact declaration identity and display fields echoed from graph search.
#[derive(Clone, Eq, PartialEq)]
pub struct RustGraphDefinitionSelector {
    source_slot: SourceSlotId,
    source_generation: i64,
    path: RepositoryPath,
    content_digest: SourceContentDigest,
    artifact: AnalysisArtifactDigest,
    fact_ordinal: u64,
    kind: RustSymbolKind,
    name: String,
    qualified_name: String,
    name_span: ByteSpan,
    declaration_span: ByteSpan,
}

impl RustGraphDefinitionSelector {
    /// Constructs one exact selector after enforcing boundary invariants.
    #[allow(
        clippy::too_many_arguments,
        reason = "every exact declaration identity field remains explicit"
    )]
    pub fn try_new(
        source_slot: SourceSlotId,
        source_generation: i64,
        path: RepositoryPath,
        content_digest: SourceContentDigest,
        artifact: AnalysisArtifactDigest,
        fact_ordinal: u64,
        kind: RustSymbolKind,
        name: String,
        qualified_name: String,
        name_span: ByteSpan,
        declaration_span: ByteSpan,
    ) -> Result<Self, RustGraphSelectorError> {
        if source_generation <= 0 || fact_ordinal > i64::MAX as u64 {
            return Err(RustGraphSelectorError::InvalidGeneration);
        }
        if name.is_empty()
            || name.len() > MAX_SYMBOL_NAME_BYTES
            || qualified_name.is_empty()
            || qualified_name.len() > MAX_QUALIFIED_NAME_BYTES
            || name.chars().any(char::is_control)
            || qualified_name.chars().any(char::is_control)
        {
            return Err(RustGraphSelectorError::InvalidText);
        }
        if name_span.start() < declaration_span.start()
            || name_span.end() > declaration_span.end()
            || name_span.len().get() != u64::try_from(name.len()).unwrap_or(u64::MAX)
            || !persistable_span(name_span)
            || !persistable_span(declaration_span)
        {
            return Err(RustGraphSelectorError::InvalidSpan);
        }
        Ok(Self {
            source_slot,
            source_generation,
            path,
            content_digest,
            artifact,
            fact_ordinal,
            kind,
            name,
            qualified_name,
            name_span,
            declaration_span,
        })
    }

    /// Returns the source slot.
    #[must_use]
    pub const fn source_slot(&self) -> SourceSlotId {
        self.source_slot
    }

    /// Returns the exact source generation selected by the view.
    #[must_use]
    pub const fn source_generation(&self) -> i64 {
        self.source_generation
    }

    /// Returns the canonical repository path.
    #[must_use]
    pub const fn path(&self) -> &RepositoryPath {
        &self.path
    }

    /// Returns the exact source-content digest.
    #[must_use]
    pub const fn content_digest(&self) -> SourceContentDigest {
        self.content_digest
    }

    /// Returns the exact analysis-artifact digest.
    #[must_use]
    pub const fn artifact(&self) -> AnalysisArtifactDigest {
        self.artifact
    }

    /// Returns the artifact-local fact ordinal.
    #[must_use]
    pub const fn fact_ordinal(&self) -> u64 {
        self.fact_ordinal
    }

    /// Returns the stable declaration category.
    #[must_use]
    pub const fn kind(&self) -> RustSymbolKind {
        self.kind
    }

    /// Returns the exact declaration name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the deterministic syntax-qualified name.
    #[must_use]
    pub fn qualified_name(&self) -> &str {
        &self.qualified_name
    }

    /// Returns the exact name span.
    #[must_use]
    pub const fn name_span(&self) -> ByteSpan {
        self.name_span
    }

    /// Returns the exact declaration span.
    #[must_use]
    pub const fn declaration_span(&self) -> ByteSpan {
        self.declaration_span
    }
}

impl fmt::Debug for RustGraphDefinitionSelector {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RustGraphDefinitionSelector")
            .field("source_slot", &self.source_slot)
            .field("source_generation", &self.source_generation)
            .field("path", &self.path)
            .field("artifact", &self.artifact)
            .field("fact_ordinal", &self.fact_ordinal)
            .field("kind", &self.kind)
            .field("name", &"<redacted>")
            .field("qualified_name", &"<redacted>")
            .field("name_span", &self.name_span)
            .field("declaration_span", &self.declaration_span)
            .finish()
    }
}

/// Exact raw-site identity echoed from graph evidence or trace output.
#[derive(Clone, Eq, PartialEq)]
pub struct RustGraphSiteSelector {
    source_slot: SourceSlotId,
    path: RepositoryPath,
    artifact: AnalysisArtifactDigest,
    ordinal: u32,
    kind: RustGraphSiteKind,
    occurrence_span: ByteSpan,
    target_span: ByteSpan,
}

impl RustGraphSiteSelector {
    /// Constructs one exact raw-site selector.
    #[allow(
        clippy::too_many_arguments,
        reason = "every exact raw-site identity field remains explicit"
    )]
    pub fn try_new(
        source_slot: SourceSlotId,
        path: RepositoryPath,
        artifact: AnalysisArtifactDigest,
        ordinal: u32,
        kind: RustGraphSiteKind,
        occurrence_span: ByteSpan,
        target_span: ByteSpan,
    ) -> Result<Self, RustGraphSelectorError> {
        if target_span.start() < occurrence_span.start()
            || target_span.end() > occurrence_span.end()
            || !persistable_span(occurrence_span)
            || !persistable_span(target_span)
        {
            return Err(RustGraphSelectorError::InvalidSpan);
        }
        Ok(Self {
            source_slot,
            path,
            artifact,
            ordinal,
            kind,
            occurrence_span,
            target_span,
        })
    }

    /// Returns the source slot.
    #[must_use]
    pub const fn source_slot(&self) -> SourceSlotId {
        self.source_slot
    }

    /// Returns the canonical repository path.
    #[must_use]
    pub const fn path(&self) -> &RepositoryPath {
        &self.path
    }

    /// Returns the exact graph-artifact digest.
    #[must_use]
    pub const fn artifact(&self) -> AnalysisArtifactDigest {
        self.artifact
    }

    /// Returns the artifact-local source-order ordinal.
    #[must_use]
    pub const fn ordinal(&self) -> u32 {
        self.ordinal
    }

    /// Returns the raw-site category.
    #[must_use]
    pub const fn kind(&self) -> RustGraphSiteKind {
        self.kind
    }

    /// Returns the exact enclosing construct span.
    #[must_use]
    pub const fn occurrence_span(&self) -> ByteSpan {
        self.occurrence_span
    }

    /// Returns the exact target spelling span.
    #[must_use]
    pub const fn target_span(&self) -> ByteSpan {
        self.target_span
    }
}

impl fmt::Debug for RustGraphSiteSelector {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RustGraphSiteSelector")
            .field("source_slot", &self.source_slot)
            .field("path", &self.path)
            .field("artifact", &self.artifact)
            .field("ordinal", &self.ordinal)
            .field("kind", &self.kind)
            .field("occurrence_span", &self.occurrence_span)
            .field("target_span", &self.target_span)
            .finish()
    }
}

/// Exact declaration or raw-site traversal start.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RustGraphTraceStartSelector {
    /// Begin from one exact declaration returned by graph search.
    Definition(RustGraphDefinitionSelector),
    /// Begin from one exact raw site returned by graph evidence or trace.
    Site(RustGraphSiteSelector),
}

/// One validated canonical graph-read operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RustGraphReadOperation {
    /// Inspect categorical publication availability.
    Status,
    /// Search exact definition names and qualified names.
    Search {
        /// Literal query.
        query: RustGraphSymbolQuery,
        /// Caller resource limits.
        limits: RustGraphTraceLimits,
    },
    /// Retrieve one exact site's categorical resolution evidence.
    Evidence {
        /// Exact site selector.
        site: RustGraphSiteSelector,
        /// Caller resource limits.
        limits: RustGraphTraceLimits,
    },
    /// Summarize exact definition and relationship counts.
    Architecture {
        /// Caller resource limits.
        limits: RustGraphTraceLimits,
    },
    /// Traverse retained graph relationships.
    Trace {
        /// Exact starting occurrence.
        start: RustGraphTraceStartSelector,
        /// Explicit traversal direction.
        direction: RustGraphTraceDirection,
        /// Non-empty relationship allow-list.
        edge_kinds: RustGraphEdgeKinds,
        /// Caller resource limits.
        limits: RustGraphTraceLimits,
    },
    /// Compute conservative inbound impact.
    Impact {
        /// Exact starting declaration.
        start: RustGraphDefinitionSelector,
        /// Non-empty relationship allow-list.
        edge_kinds: RustGraphEdgeKinds,
        /// Caller resource limits.
        limits: RustGraphTraceLimits,
    },
}

/// Stable exact-selector validation failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RustGraphSelectorError {
    /// A database-local generation or view identity is not positive.
    InvalidGeneration,
    /// A selector string is empty, oversized, or contains control text.
    InvalidText,
    /// A selector span is not nested or persistable.
    InvalidSpan,
}

impl fmt::Display for RustGraphSelectorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidGeneration => "Rust graph generation selector is invalid",
            Self::InvalidText => "Rust graph text selector is invalid",
            Self::InvalidSpan => "Rust graph span selector is invalid",
        })
    }
}

impl Error for RustGraphSelectorError {}

fn persistable_span(span: ByteSpan) -> bool {
    i64::try_from(span.start().get()).is_ok() && i64::try_from(span.end().get()).is_ok()
}
