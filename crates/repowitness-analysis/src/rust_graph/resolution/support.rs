use std::cmp::Ordering;

use super::model::{
    RustGraphDefinitionOccurrence, RustGraphResolutionControl, RustGraphResolutionError,
    RustGraphResolutionLimits, RustGraphSiteOccurrence,
};
use super::outcome::{
    RustGraphResolution, RustGraphResolutionCandidate, RustGraphResolutionCoverage,
    RustGraphResolutionOutcome, RustGraphSiteResolution, RustGraphUnresolvedReason,
};
use super::resolver::{CandidateSet, EligibleDefinitions, FileKey, ParsedImport};
use crate::RustSymbolKind;

const RESOLUTION_OUTPUT_BYTES: u64 = 64;
const SITE_OUTPUT_FIXED_BYTES: u64 = 121;
const CANDIDATE_OUTPUT_FIXED_BYTES: u64 = 106;

pub(super) struct OutputBuilder<'a> {
    definitions: &'a [RustGraphDefinitionOccurrence],
    sites: &'a [RustGraphSiteOccurrence],
    limits: RustGraphResolutionLimits,
    outcomes: Vec<RustGraphSiteResolution>,
    input_text_bytes: u64,
    output_bytes: u64,
    unresolved: u32,
    unique: u32,
    ambiguous: u32,
    unsupported: u32,
    truncated_sites: u32,
    retained_candidates: u64,
}

impl<'a> OutputBuilder<'a> {
    pub(super) fn new(
        definitions: &'a [RustGraphDefinitionOccurrence],
        sites: &'a [RustGraphSiteOccurrence],
        limits: RustGraphResolutionLimits,
        input_text_bytes: u64,
    ) -> Result<Self, RustGraphResolutionError> {
        if RESOLUTION_OUTPUT_BYTES > limits.max_output_bytes() {
            return Err(RustGraphResolutionError::OutputLimitExceeded);
        }
        Ok(Self {
            definitions,
            sites,
            limits,
            outcomes: Vec::with_capacity(sites.len()),
            input_text_bytes,
            output_bytes: RESOLUTION_OUTPUT_BYTES,
            unresolved: 0,
            unique: 0,
            ambiguous: 0,
            unsupported: 0,
            truncated_sites: 0,
            retained_candidates: 0,
        })
    }

    pub(super) fn push(
        &mut self,
        site_index: usize,
        candidates: CandidateSet,
        control: RustGraphResolutionControl<'_>,
    ) -> Result<(), RustGraphResolutionError> {
        check_control(control)?;
        let total = u32::try_from(candidates.candidates.len())
            .map_err(|_| RustGraphResolutionError::CountOverflow)?;
        let retained_limit = usize::try_from(self.limits.max_candidates_per_site())
            .map_err(|_| RustGraphResolutionError::CountOverflow)?;
        let truncated = candidates.candidates.len() > retained_limit;
        let retained = candidates
            .candidates
            .into_iter()
            .take(retained_limit)
            .collect::<Vec<_>>();
        let retained_count =
            u64::try_from(retained.len()).map_err(|_| RustGraphResolutionError::CountOverflow)?;
        self.retained_candidates = self
            .retained_candidates
            .checked_add(retained_count)
            .ok_or(RustGraphResolutionError::CountOverflow)?;
        if self.retained_candidates > self.limits.max_total_candidates() {
            return Err(RustGraphResolutionError::CandidateLimitExceeded);
        }

        let site = &self.sites[site_index];
        self.add_output_bytes(SITE_OUTPUT_FIXED_BYTES)?;
        self.add_output_bytes(len_u64(site.path().as_bytes().len())?)?;
        let mut owned = Vec::with_capacity(retained.len());
        for candidate in retained {
            check_control(control)?;
            let definition = &self.definitions[candidate.definition];
            self.add_output_bytes(CANDIDATE_OUTPUT_FIXED_BYTES)?;
            self.add_output_bytes(len_u64(definition.path().as_bytes().len())?)?;
            owned.push(RustGraphResolutionCandidate::new(
                definition.identity(),
                candidate.evidence,
            ));
        }

        let outcome = match owned.len() {
            0 => {
                self.unresolved = increment(self.unresolved)?;
                if candidates.unresolved_reason != RustGraphUnresolvedReason::NoCandidate {
                    self.unsupported = increment(self.unsupported)?;
                }
                RustGraphResolutionOutcome::Unresolved {
                    reason: candidates.unresolved_reason,
                }
            }
            1 if !truncated => {
                self.unique = increment(self.unique)?;
                RustGraphResolutionOutcome::Unique {
                    candidate: owned.pop().expect("one retained candidate"),
                }
            }
            _ => {
                self.ambiguous = increment(self.ambiguous)?;
                RustGraphResolutionOutcome::Ambiguous { candidates: owned }
            }
        };
        if truncated {
            self.truncated_sites = increment(self.truncated_sites)?;
        }
        self.outcomes.push(RustGraphSiteResolution::new(
            site.identity(),
            outcome,
            total,
            truncated,
        ));
        Ok(())
    }

    pub(super) fn finish(
        self,
        control: RustGraphResolutionControl<'_>,
    ) -> Result<RustGraphResolution, RustGraphResolutionError> {
        check_control(control)?;
        let definitions = u32::try_from(self.definitions.len())
            .map_err(|_| RustGraphResolutionError::CountOverflow)?;
        let sites =
            u32::try_from(self.sites.len()).map_err(|_| RustGraphResolutionError::CountOverflow)?;
        let coverage = RustGraphResolutionCoverage::new(
            definitions,
            sites,
            self.unresolved,
            self.unique,
            self.ambiguous,
            self.unsupported,
            self.truncated_sites,
            self.retained_candidates,
        );
        Ok(RustGraphResolution::new(
            self.outcomes,
            coverage,
            self.input_text_bytes,
            self.output_bytes,
        ))
    }

    fn add_output_bytes(&mut self, bytes: u64) -> Result<(), RustGraphResolutionError> {
        self.output_bytes = self
            .output_bytes
            .checked_add(bytes)
            .ok_or(RustGraphResolutionError::CountOverflow)?;
        if self.output_bytes > self.limits.max_output_bytes() {
            return Err(RustGraphResolutionError::OutputLimitExceeded);
        }
        Ok(())
    }
}

pub(super) fn parse_import(raw: &str) -> Option<ParsedImport<'_>> {
    let fields = raw.split_ascii_whitespace().collect::<Vec<_>>();
    let (raw_target, explicit_alias) = match fields.as_slice() {
        [target] => (*target, None),
        [target, "as", alias] => (*target, Some(*alias)),
        _ => return None,
    };
    let (target, same_slot, _) = parse_path(raw_target)?;
    let alias = match explicit_alias {
        Some("_") => None,
        Some(alias) if is_identifier(alias) => Some(alias),
        Some(_) => return None,
        None => target.rsplit("::").next().filter(|alias| *alias != "self"),
    };
    Some(ParsedImport {
        target,
        alias,
        same_slot,
    })
}

pub(super) fn parse_path(raw: &str) -> Option<(&str, bool, bool)> {
    if raw.is_empty()
        || raw.contains(char::is_whitespace)
        || raw.contains(['{', '}', '*', '!', '.', '(', ')', '[', ']', '&', '='])
        || raw.starts_with("self::")
        || raw.starts_with("super::")
    {
        return None;
    }
    let (path, same_slot) = if let Some(path) = raw.strip_prefix("crate::") {
        (path, true)
    } else if let Some(path) = raw.strip_prefix("::") {
        (path, false)
    } else {
        (raw, false)
    };
    if path.is_empty() || !path.split("::").all(is_identifier) {
        return None;
    }
    Some((path, same_slot, !path.contains("::")))
}

pub(super) fn strip_terminal_turbofish(raw: &str) -> Option<&str> {
    let marker = raw.rfind("::<")?;
    let suffix = raw.get(marker + 2..)?;
    if !suffix.ends_with('>') {
        return None;
    }
    let mut depth = 0_u32;
    for character in suffix.chars() {
        match character {
            '<' => depth = depth.checked_add(1)?,
            '>' => depth = depth.checked_sub(1)?,
            _ => {}
        }
    }
    (depth == 0 && marker != 0).then(|| &raw[..marker])
}

fn is_identifier(value: &str) -> bool {
    let value = value.strip_prefix("r#").unwrap_or(value);
    let mut characters = value.chars();
    characters
        .next()
        .is_some_and(|first| first == '_' || first.is_ascii_alphabetic())
        && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

pub(super) fn is_eligible(
    definition: &RustGraphDefinitionOccurrence,
    eligible: EligibleDefinitions,
) -> bool {
    match eligible {
        EligibleDefinitions::Any => true,
        EligibleDefinitions::FreeFunctions => definition.fact().kind() == RustSymbolKind::Function,
    }
}

pub(super) fn file_key(site: &RustGraphSiteOccurrence) -> FileKey<'_> {
    (site.source_slot(), site.artifact(), site.path())
}

pub(super) fn definition_ordering(
    left: &RustGraphDefinitionOccurrence,
    right: &RustGraphDefinitionOccurrence,
) -> Ordering {
    left.source_slot()
        .cmp(&right.source_slot())
        .then_with(|| left.path().cmp(right.path()))
        .then_with(|| left.artifact().cmp(&right.artifact()))
        .then_with(|| left.fact_ordinal().cmp(&right.fact_ordinal()))
        .then_with(|| left.fact().kind().cmp(&right.fact().kind()))
        .then_with(|| span_order(left.fact().name_span(), right.fact().name_span()))
        .then_with(|| {
            span_order(
                left.fact().declaration_span(),
                right.fact().declaration_span(),
            )
        })
}

pub(super) fn site_ordering(
    left: &RustGraphSiteOccurrence,
    right: &RustGraphSiteOccurrence,
) -> Ordering {
    left.source_slot()
        .cmp(&right.source_slot())
        .then_with(|| left.path().cmp(right.path()))
        .then_with(|| left.artifact().cmp(&right.artifact()))
        .then_with(|| left.site().ordinal().cmp(&right.site().ordinal()))
        .then_with(|| left.site().kind().cmp(&right.site().kind()))
        .then_with(|| {
            span_order(
                left.site().occurrence_span(),
                right.site().occurrence_span(),
            )
        })
        .then_with(|| span_order(left.site().target_span(), right.site().target_span()))
}

pub(super) fn same_definition_identity(
    left: &RustGraphDefinitionOccurrence,
    right: &RustGraphDefinitionOccurrence,
) -> bool {
    left.source_slot() == right.source_slot()
        && left.path() == right.path()
        && left.artifact() == right.artifact()
        && left.fact_ordinal() == right.fact_ordinal()
}

pub(super) fn same_site_identity(
    left: &RustGraphSiteOccurrence,
    right: &RustGraphSiteOccurrence,
) -> bool {
    left.source_slot() == right.source_slot()
        && left.path() == right.path()
        && left.artifact() == right.artifact()
        && left.site().ordinal() == right.site().ordinal()
}

fn span_order(left: repowitness_domain::ByteSpan, right: repowitness_domain::ByteSpan) -> Ordering {
    left.start()
        .cmp(&right.start())
        .then_with(|| left.end().cmp(&right.end()))
}

pub(super) fn add_text(
    total: &mut u64,
    bytes: usize,
    limits: RustGraphResolutionLimits,
) -> Result<(), RustGraphResolutionError> {
    *total = total
        .checked_add(len_u64(bytes)?)
        .ok_or(RustGraphResolutionError::CountOverflow)?;
    if *total > limits.max_input_text_bytes() {
        return Err(RustGraphResolutionError::InputTextLimitExceeded);
    }
    Ok(())
}

fn len_u64(value: usize) -> Result<u64, RustGraphResolutionError> {
    u64::try_from(value).map_err(|_| RustGraphResolutionError::CountOverflow)
}

fn increment(value: u32) -> Result<u32, RustGraphResolutionError> {
    value
        .checked_add(1)
        .ok_or(RustGraphResolutionError::CountOverflow)
}

fn check_control(control: RustGraphResolutionControl<'_>) -> Result<(), RustGraphResolutionError> {
    control.outcome().map_or(Ok(()), Err)
}
