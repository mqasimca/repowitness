use std::collections::BTreeSet;

use repowitness_analysis::{
    RUST_GRAPH_RESOLVER_PROFILE_VERSION, RUST_GRAPH_SITE_PROFILE_VERSION,
    RustGraphDefinitionIdentity, RustGraphDefinitionOccurrence, RustGraphResolution,
    RustGraphResolutionCandidate, RustGraphResolutionOutcome, RustGraphSiteEvidence,
    RustGraphSiteIdentity, RustGraphSiteKind, RustGraphUnresolvedReason,
};
use repowitness_application::{CanonicalAnalysisArtifactKey, hash_analysis_artifact_key};
use repowitness_domain::{
    AnalysisArtifactDigest, ConnectedWorkspaceId, RepositoryPath, SourceSlotId,
};
use sha2::{Digest, Sha256};

use super::{
    PreparedRustGraphArtifact, PreparedRustGraphGeneration, RustGraphPreparationControl,
    RustGraphPreparationError, RustGraphSource,
};

const MAX_SOURCES: usize = 256;
const ARTIFACT_PAYLOAD_VERSION: u16 = 1;
const GENERATION_INPUT_VERSION: u16 = 1;
const GENERATION_OUTPUT_VERSION: u16 = 1;

/// Validates and canonically assembles one complete generation-owned graph.
pub fn prepare_rust_graph_generation(
    connected_workspace: ConnectedWorkspaceId,
    mut sources: Vec<RustGraphSource>,
    artifact_inputs: Vec<(
        SourceSlotId,
        RepositoryPath,
        CanonicalAnalysisArtifactKey,
        repowitness_analysis::RustGraphSiteAnalysis,
    )>,
    mut definitions: Vec<RustGraphDefinitionOccurrence>,
    resolution: RustGraphResolution,
    control: RustGraphPreparationControl<'_>,
) -> Result<PreparedRustGraphGeneration, RustGraphPreparationError> {
    control.check()?;
    validate_sources(&mut sources, control)?;
    let mut artifacts = prepare_artifacts(artifact_inputs, &sources, control)?;
    canonicalize_definitions(&mut definitions, &sources, control)?;
    validate_resolution(&artifacts, &definitions, &resolution, control)?;
    control.check()?;
    artifacts.sort_unstable_by(artifact_order);
    let (syntax_error_nodes, macro_sites, test_marker_sites, heuristic_sites) =
        artifact_coverage(&artifacts, control)?;
    let edge_count = resolution.coverage().unique().into();
    let input_digest = generation_input_digest(
        connected_workspace,
        &sources,
        &artifacts,
        &definitions,
        control,
    )?;
    let output_digest = generation_output_digest(&resolution, control)?;
    control.check()?;
    Ok(PreparedRustGraphGeneration {
        connected_workspace,
        sources: sources.into_boxed_slice(),
        artifacts: artifacts.into_boxed_slice(),
        definitions: definitions.into_boxed_slice(),
        resolution,
        input_digest,
        output_digest,
        edge_count,
        syntax_error_nodes,
        macro_sites,
        test_marker_sites,
        heuristic_sites,
    })
}

fn validate_sources(
    sources: &mut [RustGraphSource],
    control: RustGraphPreparationControl<'_>,
) -> Result<(), RustGraphPreparationError> {
    control.check()?;
    if sources.is_empty() || sources.len() > MAX_SOURCES {
        return Err(RustGraphPreparationError::InvalidSources);
    }
    sources.sort_unstable();
    control.check()?;
    if sources
        .windows(2)
        .any(|pair| pair[0].source_slot() == pair[1].source_slot())
    {
        return Err(RustGraphPreparationError::InvalidSources);
    }
    Ok(())
}

fn prepare_artifacts(
    inputs: Vec<(
        SourceSlotId,
        RepositoryPath,
        CanonicalAnalysisArtifactKey,
        repowitness_analysis::RustGraphSiteAnalysis,
    )>,
    sources: &[RustGraphSource],
    control: RustGraphPreparationControl<'_>,
) -> Result<Vec<PreparedRustGraphArtifact>, RustGraphPreparationError> {
    let mut source_slots = BTreeSet::new();
    for source in sources {
        control.check()?;
        source_slots.insert(source.source_slot());
    }
    let mut artifacts = Vec::with_capacity(inputs.len());
    for (source_slot, path, key, analysis) in inputs {
        control.check()?;
        if !source_slots.contains(&source_slot) || !is_rust_path(&path) {
            return Err(RustGraphPreparationError::InvalidArtifacts);
        }
        validate_site_ordinals(&analysis, control)?;
        let artifact_digest = hash_analysis_artifact_key(&key);
        let payload_digest = artifact_payload_digest_with_control(&analysis, control)?;
        artifacts.push(PreparedRustGraphArtifact {
            source_slot,
            path,
            key,
            artifact_digest,
            payload_digest,
            analysis,
        });
    }
    control.check()?;
    artifacts.sort_unstable_by(artifact_order);
    control.check()?;
    if artifacts
        .windows(2)
        .any(|pair| pair[0].source_slot == pair[1].source_slot && pair[0].path == pair[1].path)
    {
        return Err(RustGraphPreparationError::InvalidArtifacts);
    }
    let mut canonical_artifacts = std::collections::BTreeMap::new();
    for artifact in &artifacts {
        control.check()?;
        if let Some(prior) = canonical_artifacts.insert(artifact.artifact_digest, artifact)
            && (prior.key != artifact.key
                || prior.payload_digest != artifact.payload_digest
                || prior.analysis != artifact.analysis)
        {
            return Err(RustGraphPreparationError::InvalidArtifacts);
        }
    }
    Ok(artifacts)
}

fn validate_site_ordinals(
    analysis: &repowitness_analysis::RustGraphSiteAnalysis,
    control: RustGraphPreparationControl<'_>,
) -> Result<(), RustGraphPreparationError> {
    for (ordinal, site) in analysis.sites().iter().enumerate() {
        control.check()?;
        let ordinal =
            u32::try_from(ordinal).map_err(|_| RustGraphPreparationError::CountOverflow)?;
        if site.ordinal().get() != ordinal {
            return Err(RustGraphPreparationError::InvalidArtifacts);
        }
    }
    Ok(())
}

fn canonicalize_definitions(
    definitions: &mut [RustGraphDefinitionOccurrence],
    sources: &[RustGraphSource],
    control: RustGraphPreparationControl<'_>,
) -> Result<(), RustGraphPreparationError> {
    let mut slots = BTreeSet::new();
    for source in sources {
        control.check()?;
        slots.insert(source.source_slot());
    }
    for definition in definitions.iter() {
        control.check()?;
        if !slots.contains(&definition.source_slot()) {
            return Err(RustGraphPreparationError::InvalidDefinitions);
        }
    }
    control.check()?;
    definitions.sort_unstable_by(|left, right| definition_key(left).cmp(&definition_key(right)));
    control.check()?;
    if definitions
        .windows(2)
        .any(|pair| definition_key(&pair[0]) == definition_key(&pair[1]))
    {
        return Err(RustGraphPreparationError::InvalidDefinitions);
    }
    Ok(())
}

fn validate_resolution(
    artifacts: &[PreparedRustGraphArtifact],
    definitions: &[RustGraphDefinitionOccurrence],
    resolution: &RustGraphResolution,
    control: RustGraphPreparationControl<'_>,
) -> Result<(), RustGraphPreparationError> {
    control.check()?;
    if resolution.profile_version() != RUST_GRAPH_RESOLVER_PROFILE_VERSION {
        return Err(RustGraphPreparationError::InvalidResolution);
    }
    let mut expected_site_count = 0_usize;
    for artifact in artifacts {
        control.check()?;
        expected_site_count = expected_site_count
            .checked_add(artifact.analysis.sites().len())
            .ok_or(RustGraphPreparationError::CountOverflow)?;
    }
    if expected_site_count != resolution.outcomes().len()
        || usize::try_from(resolution.coverage().definitions()).ok() != Some(definitions.len())
        || usize::try_from(resolution.coverage().sites()).ok() != Some(expected_site_count)
    {
        return Err(RustGraphPreparationError::InvalidResolution);
    }
    let mut definition_set = BTreeSet::new();
    for definition in definitions {
        control.check()?;
        definition_set.insert(definition_key(definition));
    }
    let mut counts = OutcomeCounts::default();
    let expected_sites = artifacts.iter().flat_map(|artifact| {
        artifact.analysis.sites().iter().map(move |site| {
            (
                artifact.source_slot,
                &artifact.path,
                artifact.artifact_digest,
                site,
            )
        })
    });
    for (expected, outcome) in expected_sites.zip(resolution.outcomes()) {
        control.check()?;
        if !site_matches(&expected, outcome.site()) {
            return Err(RustGraphPreparationError::InvalidResolution);
        }
        counts.observe(outcome.outcome(), outcome.candidates_truncated())?;
        for candidate in candidates(outcome.outcome()) {
            control.check()?;
            if !definition_set.contains(&definition_identity_key(candidate.target())) {
                return Err(RustGraphPreparationError::InvalidResolution);
            }
        }
    }
    counts.matches(resolution)
}

#[derive(Default)]
struct OutcomeCounts {
    unresolved: u32,
    unique: u32,
    ambiguous: u32,
    unsupported: u32,
    truncated: u32,
    candidates: u64,
}

impl OutcomeCounts {
    fn observe(
        &mut self,
        outcome: &RustGraphResolutionOutcome,
        truncated: bool,
    ) -> Result<(), RustGraphPreparationError> {
        match outcome {
            RustGraphResolutionOutcome::Unresolved { reason } => {
                self.unresolved = increment(self.unresolved)?;
                if *reason != RustGraphUnresolvedReason::NoCandidate {
                    self.unsupported = increment(self.unsupported)?;
                }
            }
            RustGraphResolutionOutcome::Unique { .. } => {
                self.unique = increment(self.unique)?;
                self.candidates = add(self.candidates, 1)?;
            }
            RustGraphResolutionOutcome::Ambiguous { candidates } => {
                self.ambiguous = increment(self.ambiguous)?;
                self.candidates = add(
                    self.candidates,
                    u64::try_from(candidates.len())
                        .map_err(|_| RustGraphPreparationError::CountOverflow)?,
                )?;
            }
        }
        if truncated {
            self.truncated = increment(self.truncated)?;
        }
        Ok(())
    }

    fn matches(self, resolution: &RustGraphResolution) -> Result<(), RustGraphPreparationError> {
        let coverage = resolution.coverage();
        if self.unresolved == coverage.unresolved()
            && self.unique == coverage.unique()
            && self.ambiguous == coverage.ambiguous()
            && self.unsupported == coverage.unsupported()
            && self.truncated == coverage.truncated_sites()
            && self.candidates == coverage.retained_candidates()
        {
            Ok(())
        } else {
            Err(RustGraphPreparationError::InvalidResolution)
        }
    }
}

type DefinitionKey<'a> = (
    SourceSlotId,
    &'a RepositoryPath,
    AnalysisArtifactDigest,
    u64,
    repowitness_analysis::RustSymbolKind,
    u64,
    u64,
    u64,
    u64,
);

fn definition_key(definition: &RustGraphDefinitionOccurrence) -> DefinitionKey<'_> {
    (
        definition.source_slot(),
        definition.path(),
        definition.artifact(),
        definition.fact_ordinal(),
        definition.fact().kind(),
        definition.fact().name_span().start().get(),
        definition.fact().name_span().end().get(),
        definition.fact().declaration_span().start().get(),
        definition.fact().declaration_span().end().get(),
    )
}

fn definition_identity_key(identity: &RustGraphDefinitionIdentity) -> DefinitionKey<'_> {
    (
        identity.source_slot(),
        identity.path(),
        identity.artifact(),
        identity.fact_ordinal(),
        identity.kind(),
        identity.name_span().start().get(),
        identity.name_span().end().get(),
        identity.declaration_span().start().get(),
        identity.declaration_span().end().get(),
    )
}

fn site_matches(
    expected: &(
        SourceSlotId,
        &RepositoryPath,
        AnalysisArtifactDigest,
        &repowitness_analysis::RustGraphSite,
    ),
    actual: &RustGraphSiteIdentity,
) -> bool {
    expected.0 == actual.source_slot()
        && expected.1 == actual.path()
        && expected.2 == actual.artifact()
        && expected.3.ordinal() == actual.ordinal()
        && expected.3.kind() == actual.kind()
        && expected.3.occurrence_span() == actual.occurrence_span()
        && expected.3.target_span() == actual.target_span()
}

fn candidates(
    outcome: &RustGraphResolutionOutcome,
) -> Box<dyn Iterator<Item = &RustGraphResolutionCandidate> + '_> {
    match outcome {
        RustGraphResolutionOutcome::Unresolved { .. } => Box::new(std::iter::empty()),
        RustGraphResolutionOutcome::Unique { candidate } => Box::new(std::iter::once(candidate)),
        RustGraphResolutionOutcome::Ambiguous { candidates } => Box::new(candidates.iter()),
    }
}

fn artifact_order(
    left: &PreparedRustGraphArtifact,
    right: &PreparedRustGraphArtifact,
) -> std::cmp::Ordering {
    (left.source_slot, &left.path, left.artifact_digest).cmp(&(
        right.source_slot,
        &right.path,
        right.artifact_digest,
    ))
}

fn artifact_coverage(
    artifacts: &[PreparedRustGraphArtifact],
    control: RustGraphPreparationControl<'_>,
) -> Result<(u64, u64, u64, u64), RustGraphPreparationError> {
    let mut syntax_errors = 0_u64;
    let mut macro_sites = 0_u64;
    let mut test_markers = 0_u64;
    let mut heuristic_sites = 0_u64;
    for artifact in artifacts {
        control.check()?;
        syntax_errors = add(
            syntax_errors,
            u64::from(artifact.analysis.syntax_error_nodes()),
        )?;
        for site in artifact.analysis.sites() {
            control.check()?;
            macro_sites = add(
                macro_sites,
                u64::from(site.kind() == RustGraphSiteKind::MacroCall),
            )?;
            test_markers = add(
                test_markers,
                u64::from(site.kind() == RustGraphSiteKind::TestMarker),
            )?;
            heuristic_sites = add(
                heuristic_sites,
                u64::from(site.evidence() == RustGraphSiteEvidence::SyntaxHeuristic),
            )?;
        }
    }
    Ok((syntax_errors, macro_sites, test_markers, heuristic_sites))
}

pub(in crate::sqlite) fn artifact_payload_digest_with_control(
    analysis: &repowitness_analysis::RustGraphSiteAnalysis,
    control: RustGraphPreparationControl<'_>,
) -> Result<[u8; 32], RustGraphPreparationError> {
    control.check()?;
    let mut hash = artifact_payload_hasher(analysis);
    for site in analysis.sites() {
        control.check()?;
        put_graph_site(&mut hash, site);
    }
    control.check()?;
    Ok(hash.finalize().into())
}

fn artifact_payload_hasher(analysis: &repowitness_analysis::RustGraphSiteAnalysis) -> Sha256 {
    let mut hash = Sha256::new();
    hash.update(b"repowitness:rust-graph-site-payload\0");
    hash.update(ARTIFACT_PAYLOAD_VERSION.to_be_bytes());
    hash.update(RUST_GRAPH_SITE_PROFILE_VERSION.to_be_bytes());
    hash.update(analysis.visited_nodes().to_be_bytes());
    hash.update(analysis.syntax_error_nodes().to_be_bytes());
    hash.update(analysis.max_observed_depth().to_be_bytes());
    hash.update(analysis.owned_text_bytes().to_be_bytes());
    put_len(&mut hash, analysis.sites().len());
    hash
}

fn put_graph_site(hash: &mut Sha256, site: &repowitness_analysis::RustGraphSite) {
    hash.update(site.ordinal().get().to_be_bytes());
    put_text(hash, site.kind().as_str());
    put_text(hash, site.evidence().as_str());
    put_span(hash, site.occurrence_span());
    put_span(hash, site.target_span());
    put_text(hash, site.raw_target());
    if let Some(enclosing) = site.enclosing_definition() {
        hash.update([1]);
        put_text(hash, enclosing.kind().as_str());
        put_text(hash, enclosing.name());
        put_text(hash, enclosing.qualified_name());
        put_span(hash, enclosing.name_span());
        put_span(hash, enclosing.declaration_span());
    } else {
        hash.update([0]);
    }
}

fn generation_input_digest(
    workspace: ConnectedWorkspaceId,
    sources: &[RustGraphSource],
    artifacts: &[PreparedRustGraphArtifact],
    definitions: &[RustGraphDefinitionOccurrence],
    control: RustGraphPreparationControl<'_>,
) -> Result<[u8; 32], RustGraphPreparationError> {
    control.check()?;
    let mut hash = Sha256::new();
    hash.update(b"repowitness:rust-graph-generation-input\0");
    hash.update(GENERATION_INPUT_VERSION.to_be_bytes());
    hash.update(workspace.as_bytes());
    put_len(&mut hash, sources.len());
    for source in sources {
        control.check()?;
        hash.update(source.source_slot().as_bytes());
        hash.update(source.generation().get().to_be_bytes());
    }
    put_len(&mut hash, artifacts.len());
    for artifact in artifacts {
        control.check()?;
        hash.update(artifact.source_slot.as_bytes());
        put_bytes(&mut hash, artifact.path.as_bytes());
        hash.update(artifact.artifact_digest.as_bytes());
        hash.update(artifact.payload_digest);
    }
    put_len(&mut hash, definitions.len());
    for definition in definitions {
        control.check()?;
        put_definition(&mut hash, definition);
        put_text(&mut hash, definition.fact().name());
        put_text(&mut hash, definition.fact().qualified_name());
    }
    control.check()?;
    Ok(hash.finalize().into())
}

fn generation_output_digest(
    resolution: &RustGraphResolution,
    control: RustGraphPreparationControl<'_>,
) -> Result<[u8; 32], RustGraphPreparationError> {
    control.check()?;
    let mut hash = Sha256::new();
    hash.update(b"repowitness:rust-graph-generation-output\0");
    hash.update(GENERATION_OUTPUT_VERSION.to_be_bytes());
    hash.update(resolution.profile_version().to_be_bytes());
    hash.update(resolution.input_text_bytes().to_be_bytes());
    hash.update(resolution.output_bytes().to_be_bytes());
    put_len(&mut hash, resolution.outcomes().len());
    for resolved in resolution.outcomes() {
        control.check()?;
        put_site_identity(&mut hash, resolved.site());
        hash.update(resolved.candidate_count().to_be_bytes());
        hash.update([u8::from(resolved.candidates_truncated())]);
        match resolved.outcome() {
            RustGraphResolutionOutcome::Unresolved { reason } => {
                hash.update([0]);
                put_text(&mut hash, reason.as_str());
            }
            RustGraphResolutionOutcome::Unique { candidate } => {
                hash.update([1]);
                put_candidate(&mut hash, candidate);
            }
            RustGraphResolutionOutcome::Ambiguous { candidates } => {
                hash.update([2]);
                put_len(&mut hash, candidates.len());
                for candidate in candidates {
                    control.check()?;
                    put_candidate(&mut hash, candidate);
                }
            }
        }
    }
    control.check()?;
    Ok(hash.finalize().into())
}

fn put_definition(hash: &mut Sha256, definition: &RustGraphDefinitionOccurrence) {
    hash.update(definition.source_slot().as_bytes());
    put_bytes(hash, definition.path().as_bytes());
    hash.update(definition.artifact().as_bytes());
    hash.update(definition.fact_ordinal().to_be_bytes());
    put_text(hash, definition.fact().kind().as_str());
    put_span(hash, definition.fact().name_span());
    put_span(hash, definition.fact().declaration_span());
}

fn put_site_identity(hash: &mut Sha256, site: &RustGraphSiteIdentity) {
    hash.update(site.source_slot().as_bytes());
    put_bytes(hash, site.path().as_bytes());
    hash.update(site.artifact().as_bytes());
    hash.update(site.ordinal().get().to_be_bytes());
    put_text(hash, site.kind().as_str());
    put_span(hash, site.occurrence_span());
    put_span(hash, site.target_span());
}

fn put_candidate(hash: &mut Sha256, candidate: &RustGraphResolutionCandidate) {
    let target = candidate.target();
    hash.update(target.source_slot().as_bytes());
    put_bytes(hash, target.path().as_bytes());
    hash.update(target.artifact().as_bytes());
    hash.update(target.fact_ordinal().to_be_bytes());
    put_text(hash, target.kind().as_str());
    put_span(hash, target.name_span());
    put_span(hash, target.declaration_span());
    put_text(hash, candidate.evidence().as_str());
}

fn put_span(hash: &mut Sha256, span: repowitness_domain::ByteSpan) {
    hash.update(span.start().get().to_be_bytes());
    hash.update(span.end().get().to_be_bytes());
}

fn put_text(hash: &mut Sha256, value: &str) {
    put_bytes(hash, value.as_bytes());
}

fn put_bytes(hash: &mut Sha256, value: &[u8]) {
    hash.update(u64::try_from(value.len()).unwrap_or(u64::MAX).to_be_bytes());
    hash.update(value);
}

fn put_len(hash: &mut Sha256, value: usize) {
    hash.update(u64::try_from(value).unwrap_or(u64::MAX).to_be_bytes());
}

fn increment(value: u32) -> Result<u32, RustGraphPreparationError> {
    value
        .checked_add(1)
        .ok_or(RustGraphPreparationError::CountOverflow)
}

fn add(left: u64, right: u64) -> Result<u64, RustGraphPreparationError> {
    left.checked_add(right)
        .ok_or(RustGraphPreparationError::CountOverflow)
}

fn is_rust_path(path: &RepositoryPath) -> bool {
    path.components()
        .next_back()
        .is_some_and(|component| component.ends_with(b".rs") && component.len() > 3)
}
