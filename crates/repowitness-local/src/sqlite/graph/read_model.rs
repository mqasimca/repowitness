use std::fmt;

use repowitness_analysis::{
    RustGraphDefinitionIdentity, RustGraphSiteIdentity, RustGraphSiteKind, RustGraphSiteOrdinal,
    RustSymbolKind,
};
use repowitness_domain::{
    AnalysisArtifactDigest, ByteSpan, ConnectedWorkspaceId, RepositoryPath, SourceContentDigest,
    SourceSlotId,
};

use crate::sqlite::GenerationId;

/// Complete immutable graph-publication receipt decoded from SQLite.
#[derive(Clone, Eq, PartialEq)]
pub struct RustGraphPublicationSummary {
    generation: GenerationId,
    connected_workspace: ConnectedWorkspaceId,
    resolver_profile_version: u32,
    input_digest: [u8; 32],
    output_digest: [u8; 32],
    source_count: u16,
    artifact_count: u64,
    definition_count: u64,
    site_count: u64,
    unresolved_count: u64,
    unique_count: u64,
    ambiguous_count: u64,
    unsupported_count: u64,
    truncated_site_count: u64,
    retained_candidate_count: u64,
    edge_count: u64,
    input_text_bytes: u64,
    output_bytes: u64,
    syntax_error_nodes: u64,
    macro_sites: u64,
    test_marker_sites: u64,
    heuristic_sites: u64,
}

impl RustGraphPublicationSummary {
    #[allow(
        clippy::too_many_arguments,
        reason = "the persisted receipt has fixed semantic coverage fields"
    )]
    pub(crate) const fn new(
        generation: GenerationId,
        connected_workspace: ConnectedWorkspaceId,
        resolver_profile_version: u32,
        input_digest: [u8; 32],
        output_digest: [u8; 32],
        source_count: u16,
        artifact_count: u64,
        definition_count: u64,
        site_count: u64,
        unresolved_count: u64,
        unique_count: u64,
        ambiguous_count: u64,
        unsupported_count: u64,
        truncated_site_count: u64,
        retained_candidate_count: u64,
        edge_count: u64,
        input_text_bytes: u64,
        output_bytes: u64,
        syntax_error_nodes: u64,
        macro_sites: u64,
        test_marker_sites: u64,
        heuristic_sites: u64,
    ) -> Self {
        Self {
            generation,
            connected_workspace,
            resolver_profile_version,
            input_digest,
            output_digest,
            source_count,
            artifact_count,
            definition_count,
            site_count,
            unresolved_count,
            unique_count,
            ambiguous_count,
            unsupported_count,
            truncated_site_count,
            retained_candidate_count,
            edge_count,
            input_text_bytes,
            output_bytes,
            syntax_error_nodes,
            macro_sites,
            test_marker_sites,
            heuristic_sites,
        }
    }

    /// Returns the immutable graph-owning generation.
    #[must_use]
    pub const fn generation(&self) -> GenerationId {
        self.generation
    }

    /// Returns the connected workspace resolved by this graph.
    #[must_use]
    pub const fn connected_workspace(&self) -> ConnectedWorkspaceId {
        self.connected_workspace
    }

    /// Returns the exact resolver profile.
    #[must_use]
    pub const fn resolver_profile_version(&self) -> u32 {
        self.resolver_profile_version
    }

    /// Returns the canonical complete input digest.
    #[must_use]
    pub const fn input_digest(&self) -> &[u8; 32] {
        &self.input_digest
    }

    /// Returns the canonical complete output digest.
    #[must_use]
    pub const fn output_digest(&self) -> &[u8; 32] {
        &self.output_digest
    }

    /// Returns graph source count.
    #[must_use]
    pub const fn source_count(&self) -> u16 {
        self.source_count
    }

    /// Returns reusable graph artifact count.
    #[must_use]
    pub const fn artifact_count(&self) -> u64 {
        self.artifact_count
    }

    /// Returns definition count.
    #[must_use]
    pub const fn definition_count(&self) -> u64 {
        self.definition_count
    }

    /// Returns raw-site and categorical outcome count.
    #[must_use]
    pub const fn site_count(&self) -> u64 {
        self.site_count
    }

    /// Returns unresolved outcome count.
    #[must_use]
    pub const fn unresolved_count(&self) -> u64 {
        self.unresolved_count
    }

    /// Returns unique outcome count.
    #[must_use]
    pub const fn unique_count(&self) -> u64 {
        self.unique_count
    }

    /// Returns ambiguous outcome count.
    #[must_use]
    pub const fn ambiguous_count(&self) -> u64 {
        self.ambiguous_count
    }

    /// Returns explicitly unsupported outcome count.
    #[must_use]
    pub const fn unsupported_count(&self) -> u64 {
        self.unsupported_count
    }

    /// Returns truncated-site count.
    #[must_use]
    pub const fn truncated_site_count(&self) -> u64 {
        self.truncated_site_count
    }

    /// Returns retained candidate count.
    #[must_use]
    pub const fn retained_candidate_count(&self) -> u64 {
        self.retained_candidate_count
    }

    /// Returns unique typed edge count.
    #[must_use]
    pub const fn edge_count(&self) -> u64 {
        self.edge_count
    }

    /// Returns resolver input text accounting.
    #[must_use]
    pub const fn input_text_bytes(&self) -> u64 {
        self.input_text_bytes
    }

    /// Returns resolver output accounting.
    #[must_use]
    pub const fn output_bytes(&self) -> u64 {
        self.output_bytes
    }

    /// Returns artifact parser error coverage.
    #[must_use]
    pub const fn syntax_error_nodes(&self) -> u64 {
        self.syntax_error_nodes
    }

    /// Returns raw macro-call coverage.
    #[must_use]
    pub const fn macro_sites(&self) -> u64 {
        self.macro_sites
    }

    /// Returns raw test-marker coverage.
    #[must_use]
    pub const fn test_marker_sites(&self) -> u64 {
        self.test_marker_sites
    }

    /// Returns syntax-heuristic raw-site coverage.
    #[must_use]
    pub const fn heuristic_sites(&self) -> u64 {
        self.heuristic_sites
    }
}

impl fmt::Debug for RustGraphPublicationSummary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RustGraphPublicationSummary")
            .field("generation", &self.generation)
            .field("connected_workspace", &self.connected_workspace)
            .field("resolver_profile_version", &self.resolver_profile_version)
            .field("input_digest", &"<redacted-digest>")
            .field("output_digest", &"<redacted-digest>")
            .field("definition_count", &self.definition_count)
            .field("site_count", &self.site_count)
            .field("edge_count", &self.edge_count)
            .finish()
    }
}

/// Categorical graph availability for a pinned legacy or graph-enabled generation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RustGraphAvailability {
    /// The generation predates graph production or did not request it.
    NotProduced {
        /// Concrete generation that remains readable without graph facts.
        generation: GenerationId,
    },
    /// A complete immutable graph receipt exists and matches the pinned view.
    Complete(Box<RustGraphPublicationSummary>),
}

/// Exact declaration selector and display metadata from one pinned graph.
#[derive(Clone, Eq, PartialEq)]
pub struct RustGraphDefinitionRecord {
    source_slot: SourceSlotId,
    source_generation: GenerationId,
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

impl RustGraphDefinitionRecord {
    #[allow(
        clippy::too_many_arguments,
        reason = "the exact evidence selector keeps all identity fields explicit"
    )]
    pub(crate) fn new(
        source_slot: SourceSlotId,
        source_generation: GenerationId,
        path: RepositoryPath,
        content_digest: SourceContentDigest,
        artifact: AnalysisArtifactDigest,
        fact_ordinal: u64,
        kind: RustSymbolKind,
        name: String,
        qualified_name: String,
        name_span: ByteSpan,
        declaration_span: ByteSpan,
    ) -> Self {
        Self {
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
        }
    }

    /// Returns the source slot.
    #[must_use]
    pub const fn source_slot(&self) -> SourceSlotId {
        self.source_slot
    }

    /// Returns the immutable source generation selected by the view.
    #[must_use]
    pub const fn source_generation(&self) -> GenerationId {
        self.source_generation
    }

    /// Returns the exact repository path.
    #[must_use]
    pub const fn path(&self) -> &RepositoryPath {
        &self.path
    }

    /// Returns the exact source-content digest for capability-backed retrieval.
    #[must_use]
    pub const fn content_digest(&self) -> SourceContentDigest {
        self.content_digest
    }

    /// Returns the declaration artifact.
    #[must_use]
    pub const fn artifact(&self) -> AnalysisArtifactDigest {
        self.artifact
    }

    /// Returns the artifact-local fact ordinal.
    #[must_use]
    pub const fn fact_ordinal(&self) -> u64 {
        self.fact_ordinal
    }

    /// Returns the declaration kind.
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

    pub(crate) fn identity(&self) -> Option<RustGraphDefinitionIdentity> {
        RustGraphDefinitionIdentity::try_new(
            self.source_slot,
            self.path.clone(),
            self.artifact,
            self.fact_ordinal,
            self.kind,
            self.name_span,
            self.declaration_span,
        )
        .ok()
    }
}

impl fmt::Debug for RustGraphDefinitionRecord {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RustGraphDefinitionRecord")
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

/// Exact originating raw-site identity used for evidence lookup.
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
    #[allow(
        clippy::too_many_arguments,
        reason = "the exact persisted site selector keeps all identity fields explicit"
    )]
    pub(crate) const fn new(
        source_slot: SourceSlotId,
        path: RepositoryPath,
        artifact: AnalysisArtifactDigest,
        ordinal: u32,
        kind: RustGraphSiteKind,
        occurrence_span: ByteSpan,
        target_span: ByteSpan,
    ) -> Self {
        Self {
            source_slot,
            path,
            artifact,
            ordinal,
            kind,
            occurrence_span,
            target_span,
        }
    }

    /// Copies a validated analysis-layer site identity.
    #[must_use]
    pub fn from_identity(identity: &RustGraphSiteIdentity) -> Self {
        Self {
            source_slot: identity.source_slot(),
            path: identity.path().clone(),
            artifact: identity.artifact(),
            ordinal: identity.ordinal().get(),
            kind: identity.kind(),
            occurrence_span: identity.occurrence_span(),
            target_span: identity.target_span(),
        }
    }

    /// Returns the source slot.
    #[must_use]
    pub const fn source_slot(&self) -> SourceSlotId {
        self.source_slot
    }

    /// Returns the exact repository path.
    #[must_use]
    pub const fn path(&self) -> &RepositoryPath {
        &self.path
    }

    /// Returns the graph-site artifact.
    #[must_use]
    pub const fn artifact(&self) -> AnalysisArtifactDigest {
        self.artifact
    }

    /// Returns the artifact-local source-order ordinal.
    #[must_use]
    pub const fn ordinal(&self) -> u32 {
        self.ordinal
    }

    /// Returns the raw-site kind.
    #[must_use]
    pub const fn kind(&self) -> RustGraphSiteKind {
        self.kind
    }

    /// Returns the exact construct span.
    #[must_use]
    pub const fn occurrence_span(&self) -> ByteSpan {
        self.occurrence_span
    }

    /// Returns the exact target span.
    #[must_use]
    pub const fn target_span(&self) -> ByteSpan {
        self.target_span
    }

    pub(crate) fn identity(&self) -> Option<RustGraphSiteIdentity> {
        RustGraphSiteIdentity::try_new(
            self.source_slot,
            self.path.clone(),
            self.artifact,
            RustGraphSiteOrdinal::new(self.ordinal),
            self.kind,
            self.occurrence_span,
            self.target_span,
        )
        .ok()
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
