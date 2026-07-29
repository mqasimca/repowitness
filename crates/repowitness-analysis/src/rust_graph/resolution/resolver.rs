use std::collections::BTreeMap;

use repowitness_domain::{RepositoryPath, SourceSlotId};

use super::model::{
    RustGraphDefinitionOccurrence, RustGraphResolutionControl, RustGraphResolutionError,
    RustGraphResolutionLimits, RustGraphSiteOccurrence,
};
use super::outcome::{RustGraphResolution, RustGraphResolutionEvidence, RustGraphUnresolvedReason};
use super::support::{
    OutputBuilder, add_text, definition_ordering, file_key, is_eligible, parse_import, parse_path,
    same_definition_identity, same_site_identity, site_ordering, strip_terminal_turbofish,
};
use crate::{RustGraphSiteKind, RustSymbolKind};

pub(super) type FileKey<'a> = (
    SourceSlotId,
    repowitness_domain::AnalysisArtifactDigest,
    &'a RepositoryPath,
);

struct AdmittedInput {
    definition_order: Vec<usize>,
    site_order: Vec<usize>,
    input_text_bytes: u64,
}

struct DefinitionIndex<'a> {
    by_qualified: BTreeMap<&'a str, Vec<usize>>,
    by_name: BTreeMap<&'a str, Vec<usize>>,
}

#[derive(Clone, Copy)]
pub(super) struct ImportBinding<'a> {
    pub(super) alias: &'a str,
    pub(super) target: &'a str,
    pub(super) same_slot: bool,
    pub(super) module_scope: Option<&'a str>,
}

pub(super) struct ParsedImport<'a> {
    pub(super) target: &'a str,
    pub(super) alias: Option<&'a str>,
    pub(super) same_slot: bool,
}

#[derive(Clone, Copy)]
pub(super) struct CandidateRef {
    pub(super) definition: usize,
    pub(super) evidence: RustGraphResolutionEvidence,
}

#[derive(Clone, Copy)]
pub(super) enum EligibleDefinitions {
    Any,
    FreeFunctions,
}

pub(super) struct CandidateSet {
    pub(super) candidates: Vec<CandidateRef>,
    pub(super) unresolved_reason: RustGraphUnresolvedReason,
}

/// Resolves a complete immutable set of Rust definitions and raw sites.
///
/// The function performs no I/O and returns no partial output after any
/// validation, limit, cancellation, or deadline failure.
pub fn resolve_rust_graph_sites(
    definitions: &[RustGraphDefinitionOccurrence],
    sites: &[RustGraphSiteOccurrence],
    limits: RustGraphResolutionLimits,
    control: RustGraphResolutionControl<'_>,
) -> Result<RustGraphResolution, RustGraphResolutionError> {
    check_control(control)?;
    let admitted = admit_inputs(definitions, sites, limits, control)?;
    let index = build_definition_index(definitions, &admitted.definition_order, control)?;
    let imports = build_import_index(sites, &admitted.site_order, control)?;
    let mut output = OutputBuilder::new(definitions, sites, limits, admitted.input_text_bytes)?;

    for site_index in admitted.site_order {
        check_control(control)?;
        let candidates = resolve_site(site_index, definitions, sites, &index, &imports, control)?;
        output.push(site_index, candidates, control)?;
    }
    check_control(control)?;
    output.finish(control)
}

fn admit_inputs(
    definitions: &[RustGraphDefinitionOccurrence],
    sites: &[RustGraphSiteOccurrence],
    limits: RustGraphResolutionLimits,
    control: RustGraphResolutionControl<'_>,
) -> Result<AdmittedInput, RustGraphResolutionError> {
    let definition_count = u32::try_from(definitions.len())
        .map_err(|_| RustGraphResolutionError::DefinitionLimitExceeded)?;
    if definition_count > limits.max_definitions() {
        return Err(RustGraphResolutionError::DefinitionLimitExceeded);
    }
    let site_count =
        u32::try_from(sites.len()).map_err(|_| RustGraphResolutionError::SiteLimitExceeded)?;
    if site_count > limits.max_sites() {
        return Err(RustGraphResolutionError::SiteLimitExceeded);
    }

    let mut text_bytes = 0_u64;
    for definition in definitions {
        check_control(control)?;
        add_text(&mut text_bytes, definition.path().as_bytes().len(), limits)?;
        add_text(&mut text_bytes, definition.fact().name().len(), limits)?;
        add_text(
            &mut text_bytes,
            definition.fact().qualified_name().len(),
            limits,
        )?;
    }
    for site in sites {
        check_control(control)?;
        let raw = site.site();
        if raw.target_span().start() < raw.occurrence_span().start()
            || raw.target_span().end() > raw.occurrence_span().end()
        {
            return Err(RustGraphResolutionError::InvalidOccurrence);
        }
        add_text(&mut text_bytes, site.path().as_bytes().len(), limits)?;
        add_text(&mut text_bytes, raw.raw_target().len(), limits)?;
        if let Some(enclosing) = raw.enclosing_definition() {
            add_text(&mut text_bytes, enclosing.name().len(), limits)?;
            add_text(&mut text_bytes, enclosing.qualified_name().len(), limits)?;
        }
    }

    let mut definition_order = (0..definitions.len()).collect::<Vec<_>>();
    definition_order
        .sort_by(|left, right| definition_ordering(&definitions[*left], &definitions[*right]));
    for pair in definition_order.windows(2) {
        check_control(control)?;
        if same_definition_identity(&definitions[pair[0]], &definitions[pair[1]]) {
            return Err(RustGraphResolutionError::DuplicateDefinition);
        }
    }

    let mut site_order = (0..sites.len()).collect::<Vec<_>>();
    site_order.sort_by(|left, right| site_ordering(&sites[*left], &sites[*right]));
    for pair in site_order.windows(2) {
        check_control(control)?;
        if same_site_identity(&sites[pair[0]], &sites[pair[1]]) {
            return Err(RustGraphResolutionError::DuplicateSite);
        }
    }
    check_control(control)?;
    Ok(AdmittedInput {
        definition_order,
        site_order,
        input_text_bytes: text_bytes,
    })
}

fn build_definition_index<'a>(
    definitions: &'a [RustGraphDefinitionOccurrence],
    order: &[usize],
    control: RustGraphResolutionControl<'_>,
) -> Result<DefinitionIndex<'a>, RustGraphResolutionError> {
    let mut index = DefinitionIndex {
        by_qualified: BTreeMap::new(),
        by_name: BTreeMap::new(),
    };
    for definition_index in order {
        check_control(control)?;
        let definition = &definitions[*definition_index];
        index
            .by_qualified
            .entry(definition.fact().qualified_name())
            .or_default()
            .push(*definition_index);
        index
            .by_name
            .entry(definition.fact().name())
            .or_default()
            .push(*definition_index);
    }
    check_control(control)?;
    Ok(index)
}

fn build_import_index<'a>(
    sites: &'a [RustGraphSiteOccurrence],
    order: &[usize],
    control: RustGraphResolutionControl<'_>,
) -> Result<BTreeMap<FileKey<'a>, Vec<ImportBinding<'a>>>, RustGraphResolutionError> {
    let mut imports = BTreeMap::<FileKey<'a>, Vec<ImportBinding<'a>>>::new();
    for site_index in order {
        check_control(control)?;
        let occurrence = &sites[*site_index];
        if occurrence.site().kind() != RustGraphSiteKind::Import {
            continue;
        }
        let Some(parsed) = parse_import(occurrence.site().raw_target()) else {
            continue;
        };
        let Some(alias) = parsed.alias else {
            continue;
        };
        let module_scope = match occurrence.site().enclosing_definition() {
            None => None,
            Some(enclosing) if enclosing.kind() == RustSymbolKind::Module => {
                Some(enclosing.qualified_name())
            }
            Some(_) => continue,
        };
        imports
            .entry(file_key(occurrence))
            .or_default()
            .push(ImportBinding {
                alias,
                target: parsed.target,
                same_slot: parsed.same_slot,
                module_scope,
            });
    }
    for bindings in imports.values_mut() {
        check_control(control)?;
        bindings.sort_by(|left, right| {
            left.alias
                .cmp(right.alias)
                .then_with(|| left.target.cmp(right.target))
                .then_with(|| left.same_slot.cmp(&right.same_slot))
                .then_with(|| left.module_scope.cmp(&right.module_scope))
        });
        bindings.dedup_by(|left, right| {
            left.alias == right.alias
                && left.target == right.target
                && left.same_slot == right.same_slot
                && left.module_scope == right.module_scope
        });
    }
    check_control(control)?;
    Ok(imports)
}

fn resolve_site(
    site_index: usize,
    definitions: &[RustGraphDefinitionOccurrence],
    sites: &[RustGraphSiteOccurrence],
    index: &DefinitionIndex<'_>,
    imports: &BTreeMap<FileKey<'_>, Vec<ImportBinding<'_>>>,
    control: RustGraphResolutionControl<'_>,
) -> Result<CandidateSet, RustGraphResolutionError> {
    let site = &sites[site_index];
    match site.site().kind() {
        RustGraphSiteKind::MacroCall | RustGraphSiteKind::TestMarker => Ok(CandidateSet {
            candidates: Vec::new(),
            unresolved_reason: RustGraphUnresolvedReason::UnsupportedSiteKind,
        }),
        RustGraphSiteKind::Import => resolve_import(site, definitions, index, control),
        RustGraphSiteKind::Reference => resolve_reference(
            site,
            definitions,
            index,
            imports,
            EligibleDefinitions::Any,
            control,
        ),
        RustGraphSiteKind::Call => resolve_call(site, definitions, index, imports, control),
    }
}

fn resolve_import(
    site: &RustGraphSiteOccurrence,
    definitions: &[RustGraphDefinitionOccurrence],
    index: &DefinitionIndex<'_>,
    control: RustGraphResolutionControl<'_>,
) -> Result<CandidateSet, RustGraphResolutionError> {
    let Some(import) = parse_import(site.site().raw_target()) else {
        return Ok(CandidateSet {
            candidates: Vec::new(),
            unresolved_reason: RustGraphUnresolvedReason::UnsupportedImportShape,
        });
    };
    let mut candidates = Vec::new();
    add_qualified_matches(
        &mut candidates,
        import.target,
        import.same_slot.then_some(site.source_slot()),
        RustGraphResolutionEvidence::ImportSyntax,
        EligibleDefinitions::Any,
        definitions,
        index,
        control,
    )?;
    finish_candidates(
        candidates,
        RustGraphUnresolvedReason::NoCandidate,
        definitions,
        control,
    )
}

fn resolve_call(
    site: &RustGraphSiteOccurrence,
    definitions: &[RustGraphDefinitionOccurrence],
    index: &DefinitionIndex<'_>,
    imports: &BTreeMap<FileKey<'_>, Vec<ImportBinding<'_>>>,
    control: RustGraphResolutionControl<'_>,
) -> Result<CandidateSet, RustGraphResolutionError> {
    let raw = site.site().raw_target();
    if raw.contains('.') || raw.starts_with('<') || raw.contains('(') || raw.contains('[') {
        return Ok(CandidateSet {
            candidates: Vec::new(),
            unresolved_reason: RustGraphUnresolvedReason::DynamicOrMethodCall,
        });
    }
    let target = strip_terminal_turbofish(raw).unwrap_or(raw);
    let mut resolved = resolve_reference_target(
        site,
        target,
        definitions,
        index,
        imports,
        EligibleDefinitions::FreeFunctions,
        control,
    )?;
    if resolved.candidates.is_empty()
        && exact_method_candidate_exists(site, target, definitions, index, control)?
    {
        resolved.unresolved_reason = RustGraphUnresolvedReason::DynamicOrMethodCall;
    }
    Ok(resolved)
}

fn resolve_reference(
    site: &RustGraphSiteOccurrence,
    definitions: &[RustGraphDefinitionOccurrence],
    index: &DefinitionIndex<'_>,
    imports: &BTreeMap<FileKey<'_>, Vec<ImportBinding<'_>>>,
    eligible: EligibleDefinitions,
    control: RustGraphResolutionControl<'_>,
) -> Result<CandidateSet, RustGraphResolutionError> {
    resolve_reference_target(
        site,
        site.site().raw_target(),
        definitions,
        index,
        imports,
        eligible,
        control,
    )
}

#[allow(
    clippy::too_many_arguments,
    reason = "resolver inputs are distinct immutable evidence"
)]
fn resolve_reference_target(
    site: &RustGraphSiteOccurrence,
    target: &str,
    definitions: &[RustGraphDefinitionOccurrence],
    index: &DefinitionIndex<'_>,
    imports: &BTreeMap<FileKey<'_>, Vec<ImportBinding<'_>>>,
    eligible: EligibleDefinitions,
    control: RustGraphResolutionControl<'_>,
) -> Result<CandidateSet, RustGraphResolutionError> {
    let Some((path, same_slot, bare)) = parse_path(target) else {
        return Ok(CandidateSet {
            candidates: Vec::new(),
            unresolved_reason: RustGraphUnresolvedReason::UnsupportedQualifiedSyntax,
        });
    };
    if !bare {
        let mut candidates = Vec::new();
        add_qualified_matches(
            &mut candidates,
            path,
            same_slot.then_some(site.source_slot()),
            RustGraphResolutionEvidence::QualifiedSyntax,
            eligible,
            definitions,
            index,
            control,
        )?;
        return finish_candidates(
            candidates,
            RustGraphUnresolvedReason::NoCandidate,
            definitions,
            control,
        );
    }

    let mut candidates = lexical_matches(site, path, eligible, definitions, index, control)?;
    if let Some(bindings) = imports.get(&file_key(site)) {
        let module_scope = reference_module_scope(site, definitions, index, control)?;
        for binding in bindings
            .iter()
            .filter(|binding| binding.alias == path && binding.module_scope == module_scope)
        {
            check_control(control)?;
            add_qualified_matches(
                &mut candidates,
                binding.target,
                binding.same_slot.then_some(site.source_slot()),
                RustGraphResolutionEvidence::ImportSyntax,
                eligible,
                definitions,
                index,
                control,
            )?;
        }
    }
    if candidates.is_empty() {
        add_name_matches(
            &mut candidates,
            path,
            RustGraphResolutionEvidence::ExactNameHeuristic,
            eligible,
            definitions,
            index,
            control,
        )?;
    }
    finish_candidates(
        candidates,
        RustGraphUnresolvedReason::NoCandidate,
        definitions,
        control,
    )
}

fn reference_module_scope<'a>(
    site: &'a RustGraphSiteOccurrence,
    definitions: &[RustGraphDefinitionOccurrence],
    index: &DefinitionIndex<'_>,
    control: RustGraphResolutionControl<'_>,
) -> Result<Option<&'a str>, RustGraphResolutionError> {
    let Some(enclosing) = site.site().enclosing_definition() else {
        return Ok(None);
    };
    if enclosing.kind() == RustSymbolKind::Module {
        return Ok(Some(enclosing.qualified_name()));
    }
    let mut prefix = enclosing
        .qualified_name()
        .rsplit_once("::")
        .map(|(prefix, _)| prefix);
    while let Some(candidate_scope) = prefix {
        check_control(control)?;
        if index
            .by_qualified
            .get(candidate_scope)
            .is_some_and(|matches| {
                matches.iter().any(|definition_index| {
                    let definition = &definitions[*definition_index];
                    definition.source_slot() == site.source_slot()
                        && definition.path() == site.path()
                        && definition.fact().kind() == RustSymbolKind::Module
                })
            })
        {
            return Ok(Some(candidate_scope));
        }
        prefix = candidate_scope.rsplit_once("::").map(|(parent, _)| parent);
    }
    Ok(None)
}

fn lexical_matches(
    site: &RustGraphSiteOccurrence,
    name: &str,
    eligible: EligibleDefinitions,
    definitions: &[RustGraphDefinitionOccurrence],
    index: &DefinitionIndex<'_>,
    control: RustGraphResolutionControl<'_>,
) -> Result<Vec<CandidateRef>, RustGraphResolutionError> {
    let mut scope = site
        .site()
        .enclosing_definition()
        .map(|enclosing| enclosing.qualified_name());
    while let Some(current) = scope {
        check_control(control)?;
        let mut qualified = String::with_capacity(current.len() + name.len() + 2);
        qualified.push_str(current);
        qualified.push_str("::");
        qualified.push_str(name);
        let mut candidates = Vec::new();
        add_qualified_matches(
            &mut candidates,
            &qualified,
            Some(site.source_slot()),
            RustGraphResolutionEvidence::LexicalSyntax,
            eligible,
            definitions,
            index,
            control,
        )?;
        if !candidates.is_empty() {
            return Ok(candidates);
        }
        scope = current.rsplit_once("::").map(|(parent, _)| parent);
    }

    let mut top_level = Vec::new();
    add_qualified_matches(
        &mut top_level,
        name,
        Some(site.source_slot()),
        RustGraphResolutionEvidence::LexicalSyntax,
        eligible,
        definitions,
        index,
        control,
    )?;
    Ok(top_level)
}

#[allow(
    clippy::too_many_arguments,
    reason = "matching requires exact scope and evidence"
)]
fn add_qualified_matches(
    candidates: &mut Vec<CandidateRef>,
    qualified_name: &str,
    source_slot: Option<SourceSlotId>,
    evidence: RustGraphResolutionEvidence,
    eligible: EligibleDefinitions,
    definitions: &[RustGraphDefinitionOccurrence],
    index: &DefinitionIndex<'_>,
    control: RustGraphResolutionControl<'_>,
) -> Result<(), RustGraphResolutionError> {
    let Some(matches) = index.by_qualified.get(qualified_name) else {
        return Ok(());
    };
    for definition_index in matches {
        check_control(control)?;
        let definition = &definitions[*definition_index];
        if source_slot.is_none_or(|slot| definition.source_slot() == slot)
            && is_eligible(definition, eligible)
        {
            candidates.push(CandidateRef {
                definition: *definition_index,
                evidence,
            });
        }
    }
    Ok(())
}

#[allow(
    clippy::too_many_arguments,
    reason = "matching requires exact scope and evidence"
)]
fn add_name_matches(
    candidates: &mut Vec<CandidateRef>,
    name: &str,
    evidence: RustGraphResolutionEvidence,
    eligible: EligibleDefinitions,
    definitions: &[RustGraphDefinitionOccurrence],
    index: &DefinitionIndex<'_>,
    control: RustGraphResolutionControl<'_>,
) -> Result<(), RustGraphResolutionError> {
    let Some(matches) = index.by_name.get(name) else {
        return Ok(());
    };
    for definition_index in matches {
        check_control(control)?;
        if is_eligible(&definitions[*definition_index], eligible) {
            candidates.push(CandidateRef {
                definition: *definition_index,
                evidence,
            });
        }
    }
    Ok(())
}

fn exact_method_candidate_exists(
    site: &RustGraphSiteOccurrence,
    target: &str,
    definitions: &[RustGraphDefinitionOccurrence],
    index: &DefinitionIndex<'_>,
    control: RustGraphResolutionControl<'_>,
) -> Result<bool, RustGraphResolutionError> {
    let Some((path, same_slot, bare)) = parse_path(target) else {
        return Ok(false);
    };
    let matches = if bare {
        index.by_name.get(path)
    } else {
        index.by_qualified.get(path)
    };
    let Some(matches) = matches else {
        return Ok(false);
    };
    for definition_index in matches {
        check_control(control)?;
        let definition = &definitions[*definition_index];
        if definition.fact().kind() == RustSymbolKind::Method
            && (!same_slot || definition.source_slot() == site.source_slot())
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn finish_candidates(
    mut candidates: Vec<CandidateRef>,
    unresolved_reason: RustGraphUnresolvedReason,
    definitions: &[RustGraphDefinitionOccurrence],
    control: RustGraphResolutionControl<'_>,
) -> Result<CandidateSet, RustGraphResolutionError> {
    candidates.sort_by(|left, right| {
        definition_ordering(
            &definitions[left.definition],
            &definitions[right.definition],
        )
        .then_with(|| left.evidence.cmp(&right.evidence))
    });
    let mut previous = None;
    candidates.retain(|candidate| {
        let keep = previous != Some(candidate.definition);
        previous = Some(candidate.definition);
        keep
    });
    check_control(control)?;
    Ok(CandidateSet {
        candidates,
        unresolved_reason,
    })
}

fn check_control(control: RustGraphResolutionControl<'_>) -> Result<(), RustGraphResolutionError> {
    control.outcome().map_or(Ok(()), Err)
}

#[cfg(test)]
mod phase_control_tests {
    use std::{
        sync::atomic::AtomicBool,
        time::{Duration, Instant},
    };

    use super::{
        RustGraphResolutionControl, RustGraphResolutionError, RustGraphResolutionLimits,
        admit_inputs, build_definition_index, build_import_index, finish_candidates,
    };

    fn assert_empty_phases_stop(
        control: RustGraphResolutionControl<'_>,
        expected: RustGraphResolutionError,
    ) {
        assert_eq!(
            admit_inputs(&[], &[], RustGraphResolutionLimits::DEFAULT, control).err(),
            Some(expected)
        );
        assert_eq!(
            build_definition_index(&[], &[], control).err(),
            Some(expected)
        );
        assert_eq!(build_import_index(&[], &[], control).err(), Some(expected));
        assert_eq!(
            finish_candidates(
                Vec::new(),
                super::RustGraphUnresolvedReason::NoCandidate,
                &[],
                control
            )
            .err(),
            Some(expected)
        );
    }

    #[test]
    fn admission_indexing_and_candidate_phases_check_both_controls() {
        let cancelled = AtomicBool::new(true);
        let future = Instant::now()
            .checked_add(Duration::from_secs(5))
            .expect("short deadline must fit");
        assert_empty_phases_stop(
            RustGraphResolutionControl::new(&cancelled, future),
            RustGraphResolutionError::Cancelled,
        );

        let active = AtomicBool::new(false);
        assert_empty_phases_stop(
            RustGraphResolutionControl::new(&active, Instant::now()),
            RustGraphResolutionError::DeadlineExceeded,
        );
    }
}
