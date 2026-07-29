use repowitness_analysis::{
    RustGraphResolutionEvidence, RustGraphSiteEvidence, RustGraphUnresolvedReason,
};

use crate::sqlite::graph::{
    RustGraphCandidateRecord, RustGraphDefinitionRecord, RustGraphEdgeKind, RustGraphOutcomeRecord,
    RustGraphPublicationSummary,
};

const GRAPH_DEFINITION_FIXED_OUTPUT_BYTES: u64 = 224;
const GRAPH_EVIDENCE_FIXED_OUTPUT_BYTES: u64 = 192;

struct PersistedGraphEvidence {
    content_digest: Vec<u8>,
    extraction_evidence: String,
    outcome_kind: String,
    unresolved_reason: Option<String>,
    candidate_count: i64,
    candidates_truncated: i64,
}

impl PersistedGraphEvidence {
    fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            content_digest: row.get(0)?,
            extraction_evidence: row.get(1)?,
            outcome_kind: row.get(2)?,
            unresolved_reason: row.get(3)?,
            candidate_count: row.get(4)?,
            candidates_truncated: row.get(5)?,
        })
    }
}

struct RawGraphPublication {
    lifecycle: String,
    connected_workspace: Vec<u8>,
    resolver_profile: i64,
    input_digest: Vec<u8>,
    output_digest: Vec<u8>,
    source_count: i64,
    artifact_count: i64,
    definition_count: i64,
    site_count: i64,
    unresolved_count: i64,
    unique_count: i64,
    ambiguous_count: i64,
    unsupported_count: i64,
    truncated_site_count: i64,
    retained_candidate_count: i64,
    edge_count: i64,
    input_text_bytes: i64,
    output_bytes: i64,
    syntax_error_nodes: i64,
    macro_sites: i64,
    test_marker_sites: i64,
    heuristic_sites: i64,
}

impl RawGraphPublication {
    fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            lifecycle: row.get(0)?,
            connected_workspace: row.get(1)?,
            resolver_profile: row.get(2)?,
            input_digest: row.get(3)?,
            output_digest: row.get(4)?,
            source_count: row.get(5)?,
            artifact_count: row.get(6)?,
            definition_count: row.get(7)?,
            site_count: row.get(8)?,
            unresolved_count: row.get(9)?,
            unique_count: row.get(10)?,
            ambiguous_count: row.get(11)?,
            unsupported_count: row.get(12)?,
            truncated_site_count: row.get(13)?,
            retained_candidate_count: row.get(14)?,
            edge_count: row.get(15)?,
            input_text_bytes: row.get(16)?,
            output_bytes: row.get(17)?,
            syntax_error_nodes: row.get(18)?,
            macro_sites: row.get(19)?,
            test_marker_sites: row.get(20)?,
            heuristic_sites: row.get(21)?,
        })
    }

    fn decode(
        self,
        generation: GenerationId,
        required_profile: i64,
    ) -> Result<RustGraphPublicationSummary, GraphFailure> {
        if self.lifecycle != "complete"
            || self.resolver_profile != required_profile
            || self.resolver_profile
                != i64::from(repowitness_analysis::RUST_GRAPH_RESOLVER_PROFILE_VERSION)
        {
            return Err(corrupt_graph());
        }
        let connected_workspace = repowitness_domain::ConnectedWorkspaceId::try_from_slice(
            &self.connected_workspace,
        )
        .map_err(|_| corrupt_graph())?;
        let input_digest = persisted_digest(self.input_digest)?;
        let output_digest = persisted_digest(self.output_digest)?;
        let source_count = u16::try_from(self.source_count).map_err(|_| corrupt_graph())?;
        Ok(RustGraphPublicationSummary::new(
            generation,
            connected_workspace,
            u32::try_from(self.resolver_profile).map_err(|_| corrupt_graph())?,
            input_digest,
            output_digest,
            source_count,
            persisted_u64(self.artifact_count)?,
            persisted_u64(self.definition_count)?,
            persisted_u64(self.site_count)?,
            persisted_u64(self.unresolved_count)?,
            persisted_u64(self.unique_count)?,
            persisted_u64(self.ambiguous_count)?,
            persisted_u64(self.unsupported_count)?,
            persisted_u64(self.truncated_site_count)?,
            persisted_u64(self.retained_candidate_count)?,
            persisted_u64(self.edge_count)?,
            persisted_u64(self.input_text_bytes)?,
            persisted_u64(self.output_bytes)?,
            persisted_u64(self.syntax_error_nodes)?,
            persisted_u64(self.macro_sites)?,
            persisted_u64(self.test_marker_sites)?,
            persisted_u64(self.heuristic_sites)?,
        ))
    }
}

#[derive(Clone, Copy)]
struct ActualGraphCounts {
    sources: u64,
    artifacts: u64,
    definitions: u64,
    sites: u64,
    unresolved: u64,
    unique: u64,
    ambiguous: u64,
    unsupported: u64,
    truncated_sites: u64,
    candidates: u64,
    edges: u64,
    syntax_errors: u64,
    macro_sites: u64,
    test_markers: u64,
    heuristic_sites: u64,
    invalid_artifacts: u64,
    invalid_definitions: u64,
    invalid_resolutions: u64,
    invalid_candidates: u64,
    invalid_edges: u64,
}

impl ActualGraphCounts {
    fn from_row(row: &rusqlite::Row<'_>) -> Result<Self, GraphFailure> {
        Ok(Self {
            sources: persisted_u64(row.get(0)?)?,
            artifacts: persisted_u64(row.get(1)?)?,
            definitions: persisted_u64(row.get(2)?)?,
            sites: persisted_u64(row.get(3)?)?,
            unresolved: persisted_u64(row.get(4)?)?,
            unique: persisted_u64(row.get(5)?)?,
            ambiguous: persisted_u64(row.get(6)?)?,
            unsupported: persisted_u64(row.get(7)?)?,
            truncated_sites: persisted_u64(row.get(8)?)?,
            candidates: persisted_u64(row.get(9)?)?,
            edges: persisted_u64(row.get(10)?)?,
            syntax_errors: persisted_u64(row.get(11)?)?,
            macro_sites: persisted_u64(row.get(12)?)?,
            test_markers: persisted_u64(row.get(13)?)?,
            heuristic_sites: persisted_u64(row.get(14)?)?,
            invalid_artifacts: persisted_u64(row.get(15)?)?,
            invalid_definitions: persisted_u64(row.get(16)?)?,
            invalid_resolutions: persisted_u64(row.get(17)?)?,
            invalid_candidates: persisted_u64(row.get(18)?)?,
            invalid_edges: persisted_u64(row.get(19)?)?,
        })
    }

    fn matches(self, publication: &RustGraphPublicationSummary) -> bool {
        self.sources == u64::from(publication.source_count())
            && self.artifacts == publication.artifact_count()
            && self.definitions == publication.definition_count()
            && self.sites == publication.site_count()
            && self.unresolved == publication.unresolved_count()
            && self.unique == publication.unique_count()
            && self.ambiguous == publication.ambiguous_count()
            && self.unsupported == publication.unsupported_count()
            && self.truncated_sites == publication.truncated_site_count()
            && self.candidates == publication.retained_candidate_count()
            && self.edges == publication.edge_count()
            && self.syntax_errors == publication.syntax_error_nodes()
            && self.macro_sites == publication.macro_sites()
            && self.test_markers == publication.test_marker_sites()
            && self.heuristic_sites == publication.heuristic_sites()
            && self.invalid_artifacts == 0
            && self.invalid_definitions == 0
            && self.invalid_resolutions == 0
            && self.invalid_candidates == 0
            && self.invalid_edges == 0
            && self.sites
                == self
                    .unresolved
                    .checked_add(self.unique)
                    .and_then(|value| value.checked_add(self.ambiguous))
                    .unwrap_or(u64::MAX)
            && self.edges == self.unique
    }
}

fn decode_graph_definition(
    row: &rusqlite::Row<'_>,
    offset: usize,
) -> Result<RustGraphDefinitionRecord, GraphFailure> {
    let source_slot: Vec<u8> = row.get(offset)?;
    let source_generation: i64 = row.get(offset + 1)?;
    let path: Vec<u8> = row.get(offset + 2)?;
    let content_digest: Vec<u8> = row.get(offset + 3)?;
    let artifact: Vec<u8> = row.get(offset + 4)?;
    let fact_ordinal: i64 = row.get(offset + 5)?;
    let kind: String = row.get(offset + 6)?;
    let name: String = row.get(offset + 7)?;
    let qualified_name: String = row.get(offset + 8)?;
    let name_start: i64 = row.get(offset + 9)?;
    let name_end: i64 = row.get(offset + 10)?;
    let declaration_start: i64 = row.get(offset + 11)?;
    let declaration_end: i64 = row.get(offset + 12)?;
    if source_generation <= 0
        || name.is_empty()
        || name.len() > 1_024
        || qualified_name.is_empty()
        || qualified_name.len() > 16_384
    {
        return Err(corrupt_graph());
    }
    let source_slot = repowitness_domain::SourceSlotId::try_from_slice(&source_slot)
        .map_err(|_| corrupt_graph())?;
    let path = RepositoryPath::try_from_vec(path, PERSISTED_PATH_LIMITS)
        .map_err(|_| corrupt_graph())?;
    let content_digest =
        SourceContentDigest::try_from_slice(&content_digest).map_err(|_| corrupt_graph())?;
    let artifact =
        AnalysisArtifactDigest::try_from_slice(&artifact).map_err(|_| corrupt_graph())?;
    let fact_ordinal = u64::try_from(fact_ordinal).map_err(|_| corrupt_graph())?;
    let kind = parse_rust_graph_symbol_kind(&kind).ok_or_else(corrupt_graph)?;
    let name_span = persisted_graph_span(name_start, name_end)?;
    let declaration_span = persisted_graph_span(declaration_start, declaration_end)?;
    if name_span.start().get() < declaration_span.start().get()
        || name_span.end().get() > declaration_span.end().get()
    {
        return Err(corrupt_graph());
    }
    Ok(RustGraphDefinitionRecord::new(
        source_slot,
        GenerationId::from_database(source_generation),
        path,
        content_digest,
        artifact,
        fact_ordinal,
        kind,
        name,
        qualified_name,
        name_span,
        declaration_span,
    ))
}

fn decode_graph_candidate(
    row: &rusqlite::Row<'_>,
    evidence_offset: usize,
    definition_offset: usize,
) -> Result<RustGraphCandidateRecord, GraphFailure> {
    let evidence: String = row.get(evidence_offset)?;
    Ok(RustGraphCandidateRecord {
        target: decode_graph_definition(row, definition_offset)?,
        evidence: parse_resolution_evidence(&evidence).ok_or_else(corrupt_graph)?,
    })
}

fn graph_definition_output_bytes(
    current: u64,
    definition: &RustGraphDefinitionRecord,
    limit: u64,
) -> Result<u64, GraphFailure> {
    let row_bytes = GRAPH_DEFINITION_FIXED_OUTPUT_BYTES
        .checked_add(definition.path().byte_count().get())
        .and_then(|value| value.checked_add(definition.name().len().try_into().ok()?))
        .and_then(|value| {
            value.checked_add(definition.qualified_name().len().try_into().ok()?)
        })
        .ok_or_else(output_limit)?;
    bounded_graph_output(current, row_bytes, limit)
}

fn graph_evidence_output_bytes(
    candidates: &[RustGraphCandidateRecord],
    limit: u64,
) -> Result<u64, GraphFailure> {
    candidates
        .iter()
        .try_fold(GRAPH_EVIDENCE_FIXED_OUTPUT_BYTES, |current, candidate| {
            graph_definition_output_bytes(current, candidate.target(), limit)
        })
}

fn bounded_graph_output(
    current: u64,
    additional: u64,
    limit: u64,
) -> Result<u64, GraphFailure> {
    let total = current.checked_add(additional).ok_or_else(output_limit)?;
    if total > limit {
        Err(output_limit())
    } else {
        Ok(total)
    }
}

fn persisted_graph_span(start: i64, end: i64) -> Result<ByteSpan, GraphFailure> {
    let start = u64::try_from(start).map_err(|_| corrupt_graph())?;
    let end = u64::try_from(end).map_err(|_| corrupt_graph())?;
    ByteSpan::try_new(ByteOffset::new(start), ByteOffset::new(end))
        .map_err(|_| corrupt_graph())
}

fn persisted_u64(value: i64) -> Result<u64, GraphFailure> {
    u64::try_from(value).map_err(|_| corrupt_graph())
}

fn persisted_digest(value: Vec<u8>) -> Result<[u8; 32], GraphFailure> {
    value.try_into().map_err(|_| corrupt_graph())
}

fn parse_rust_graph_symbol_kind(value: &str) -> Option<RustSymbolKind> {
    match RustSymbolKind::from_stable_str(value)? {
        kind @ (RustSymbolKind::Function
        | RustSymbolKind::Method
        | RustSymbolKind::Struct
        | RustSymbolKind::Enum
        | RustSymbolKind::Union
        | RustSymbolKind::Trait
        | RustSymbolKind::Module
        | RustSymbolKind::TypeAlias
        | RustSymbolKind::Constant
        | RustSymbolKind::Static
        | RustSymbolKind::Macro) => Some(kind),
        RustSymbolKind::Interface
        | RustSymbolKind::DefinedType
        | RustSymbolKind::Variable
        | RustSymbolKind::Class => None,
    }
}

fn parse_resolution_evidence(value: &str) -> Option<RustGraphResolutionEvidence> {
    match value {
        "qualified_syntax" => Some(RustGraphResolutionEvidence::QualifiedSyntax),
        "lexical_syntax" => Some(RustGraphResolutionEvidence::LexicalSyntax),
        "import_syntax" => Some(RustGraphResolutionEvidence::ImportSyntax),
        "exact_name_heuristic" => Some(RustGraphResolutionEvidence::ExactNameHeuristic),
        _ => None,
    }
}

fn parse_unresolved_reason(value: &str) -> Option<RustGraphUnresolvedReason> {
    match value {
        "no_candidate" => Some(RustGraphUnresolvedReason::NoCandidate),
        "unsupported_site_kind" => Some(RustGraphUnresolvedReason::UnsupportedSiteKind),
        "unsupported_import_shape" => Some(RustGraphUnresolvedReason::UnsupportedImportShape),
        "dynamic_or_method_call" => Some(RustGraphUnresolvedReason::DynamicOrMethodCall),
        "unsupported_qualified_syntax" => {
            Some(RustGraphUnresolvedReason::UnsupportedQualifiedSyntax)
        }
        _ => None,
    }
}

fn corrupt_graph() -> GraphFailure {
    GraphFailure::Read(RustGraphReadError::CorruptGraph)
}

fn output_limit() -> GraphFailure {
    GraphFailure::Read(RustGraphReadError::OutputLimitExceeded)
}
