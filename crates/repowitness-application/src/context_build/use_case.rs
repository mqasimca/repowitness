use std::{
    cmp::Ordering,
    sync::atomic::{AtomicBool, Ordering as AtomicOrdering},
    time::Instant,
};

use crate::{MemoryEffectiveState, MemoryRecallResult};

enum PendingItem {
    Memory {
        provider_rank: u16,
        record: MemoryRecallRecord,
    },
    Source(ContextSourceCandidate),
}

impl PendingItem {
    const fn provider(&self) -> ContextProvider {
        match self {
            Self::Memory { .. } => ContextProvider::Memory,
            Self::Source(_) => ContextProvider::Source,
        }
    }

    const fn provider_rank(&self) -> u16 {
        match self {
            Self::Memory { provider_rank, .. } => *provider_rank,
            Self::Source(candidate) => candidate.provider_rank(),
        }
    }

    fn stable_tie_break(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Memory { record: left, .. }, Self::Memory { record: right, .. }) => {
                left.record_id().cmp(&right.record_id())
            }
            (Self::Source(left), Self::Source(right)) => left
                .selector()
                .path()
                .as_bytes()
                .cmp(right.selector().path().as_bytes())
                .then_with(|| {
                    left.selector()
                        .fact_ordinal()
                        .cmp(&right.selector().fact_ordinal())
                }),
            _ => Ordering::Equal,
        }
    }
}

struct PreparedPending<P> {
    items: Vec<PendingItem>,
    projection: Option<ContextMemoryProjection<P>>,
    total_matches: u64,
    returned_matches: u64,
    recall_omitted: u64,
    non_current_omitted: u64,
}

struct Admission {
    items: Vec<ContextItem>,
    used_units: u64,
    source_budget_omitted: u64,
    memory_budget_omitted: u64,
    source_included: u64,
    memory_included: u64,
}

/// Compiles exact source declarations and current memory under one budget.
pub fn compile_context<G, P>(
    source: ContextSourceInput<G>,
    memory: Option<&MemoryRecallResult<G, P>>,
    budget: ContextBuildBudget,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<ContextBuildResult<G, P>, ContextBuildError>
where
    G: Copy + Eq,
    P: Copy,
{
    check_control(cancelled, deadline)?;
    let expanded_count = u64::try_from(source.candidates.len())
        .map_err(|_| ContextBuildError::CountNotRepresentable)?;
    let source_expansion_omitted = source
        .returned_matches
        .checked_sub(expanded_count)
        .ok_or(ContextBuildError::InvalidSourceInput)?;
    let source_search_omitted = source
        .total_matches
        .checked_sub(source.returned_matches)
        .ok_or(ContextBuildError::InvalidSourceInput)?;
    let prepared = prepare_pending(
        source.candidates,
        source.snapshot,
        source.generation,
        memory,
        cancelled,
        deadline,
    )?;
    let admission = admit_pending(prepared.items, budget, cancelled, deadline)?;

    let coverage = ContextBuildCoverage {
        source_index: source.coverage,
        source_total_matches: source.total_matches,
        source_returned_matches: source.returned_matches,
        source_expansion_omitted,
        source_budget_omitted: admission.source_budget_omitted,
        source_included: admission.source_included,
        memory_total_matches: prepared.total_matches,
        memory_returned_matches: prepared.returned_matches,
        memory_non_current_omitted: prepared.non_current_omitted,
        memory_budget_omitted: admission.memory_budget_omitted,
        memory_included: admission.memory_included,
    };
    let omissions = omissions(
        source_search_omitted,
        source_expansion_omitted,
        memory.is_some(),
        prepared.recall_omitted,
        prepared.non_current_omitted,
        admission.memory_budget_omitted,
        admission.source_budget_omitted,
    );
    check_control(cancelled, deadline)?;
    Ok(ContextBuildResult {
        repository: source.repository,
        query: source.query,
        snapshot: source.snapshot,
        generation: source.generation,
        memory: prepared.projection,
        budget,
        used_units: admission.used_units,
        items: admission.items.into_boxed_slice(),
        coverage,
        omissions: omissions.into_boxed_slice(),
    })
}

fn prepare_pending<G, P>(
    source: Vec<ContextSourceCandidate>,
    snapshot: repowitness_domain::SourceSnapshotDigest,
    generation: G,
    memory: Option<&MemoryRecallResult<G, P>>,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<PreparedPending<P>, ContextBuildError>
where
    G: Copy + Eq,
    P: Copy,
{
    let mut prepared = PreparedPending {
        items: source.into_iter().map(PendingItem::Source).collect(),
        projection: None,
        total_matches: 0,
        returned_matches: 0,
        recall_omitted: 0,
        non_current_omitted: 0,
    };
    if let Some(memory) = memory {
        if memory.snapshot() != snapshot || memory.generation() != &generation {
            return Err(ContextBuildError::ContextMismatch);
        }
        prepared.total_matches = memory.total_matches();
        prepared.returned_matches = u64::try_from(memory.records().len())
            .map_err(|_| ContextBuildError::CountNotRepresentable)?;
        prepared.recall_omitted = memory.omitted_matches();
        prepared.projection = Some(ContextMemoryProjection {
            projection: *memory.projection(),
            source_epoch: memory.source_epoch(),
            producer: memory.producer().clone(),
            coverage: memory.projection_coverage(),
        });
        add_current_memory(&mut prepared, memory, cancelled, deadline)?;
    }
    prepared.items.sort_by(|left, right| {
        left.provider_rank()
            .cmp(&right.provider_rank())
            .then_with(|| left.provider().cmp(&right.provider()))
            .then_with(|| left.stable_tie_break(right))
    });
    Ok(prepared)
}

fn add_current_memory<G, P>(
    prepared: &mut PreparedPending<P>,
    memory: &MemoryRecallResult<G, P>,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), ContextBuildError> {
    for (index, record) in memory.records().iter().enumerate() {
        check_control(cancelled, deadline)?;
        if record.effective_state() != MemoryEffectiveState::Current {
            prepared.non_current_omitted = prepared
                .non_current_omitted
                .checked_add(1)
                .ok_or(ContextBuildError::CountNotRepresentable)?;
            continue;
        }
        if record.record().is_none() {
            return Err(ContextBuildError::InvalidMemoryCandidate);
        }
        let provider_rank =
            u16::try_from(index + 1).map_err(|_| ContextBuildError::CountNotRepresentable)?;
        prepared.items.push(PendingItem::Memory {
            provider_rank,
            record: record.clone(),
        });
    }
    Ok(())
}

fn admit_pending(
    pending: Vec<PendingItem>,
    budget: ContextBuildBudget,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<Admission, ContextBuildError> {
    let mut admission = Admission {
        items: Vec::with_capacity(pending.len()),
        used_units: 0,
        source_budget_omitted: 0,
        memory_budget_omitted: 0,
        source_included: 0,
        memory_included: 0,
    };
    for (index, candidate) in pending.into_iter().enumerate() {
        check_control(cancelled, deadline)?;
        let rank = context_rank(&candidate, index)?;
        let estimated_units = pending_item_units(&candidate)?;
        let next_used = admission
            .used_units
            .checked_add(estimated_units)
            .ok_or(ContextBuildError::CountNotRepresentable)?;
        if next_used > budget.units() {
            add_budget_omission(&mut admission, candidate.provider())?;
            continue;
        }
        admission.used_units = next_used;
        admit_item(&mut admission, candidate, rank, estimated_units)?;
    }
    Ok(admission)
}

fn context_rank(candidate: &PendingItem, index: usize) -> Result<ContextRank, ContextBuildError> {
    let provider_rank = candidate.provider_rank();
    Ok(ContextRank {
        provider: candidate.provider(),
        provider_rank,
        fused_rank: u16::try_from(index + 1)
            .map_err(|_| ContextBuildError::CountNotRepresentable)?,
        reciprocal_rank_denominator: CONTEXT_BUILD_RRF_K
            .checked_add(provider_rank)
            .ok_or(ContextBuildError::CountNotRepresentable)?,
    })
}

fn add_budget_omission(
    admission: &mut Admission,
    provider: ContextProvider,
) -> Result<(), ContextBuildError> {
    let count = match provider {
        ContextProvider::Memory => &mut admission.memory_budget_omitted,
        ContextProvider::Source => &mut admission.source_budget_omitted,
        ContextProvider::Structural | ContextProvider::References | ContextProvider::History => {
            return Err(ContextBuildError::InvalidSourceInput);
        }
    };
    *count = count
        .checked_add(1)
        .ok_or(ContextBuildError::CountNotRepresentable)?;
    Ok(())
}

fn admit_item(
    admission: &mut Admission,
    candidate: PendingItem,
    rank: ContextRank,
    estimated_units: u64,
) -> Result<(), ContextBuildError> {
    match candidate {
        PendingItem::Memory { record, .. } => {
            admission.memory_included = admission
                .memory_included
                .checked_add(1)
                .ok_or(ContextBuildError::CountNotRepresentable)?;
            admission
                .items
                .push(ContextItem::Memory(ContextMemoryItem {
                    rank,
                    estimated_units,
                    record,
                }));
        }
        PendingItem::Source(candidate) => {
            admission.source_included = admission
                .source_included
                .checked_add(1)
                .ok_or(ContextBuildError::CountNotRepresentable)?;
            admission
                .items
                .push(ContextItem::Source(ContextSourceItem {
                    rank,
                    estimated_units,
                    candidate,
                }));
        }
    }
    Ok(())
}

fn pending_item_units(candidate: &PendingItem) -> Result<u64, ContextBuildError> {
    match candidate {
        PendingItem::Source(candidate) => u64::try_from(candidate.declaration().len())
            .map_err(|_| ContextBuildError::CountNotRepresentable),
        PendingItem::Memory { record, .. } => {
            let record = record
                .record()
                .ok_or(ContextBuildError::InvalidMemoryCandidate)?;
            let title = u64::try_from(record.claim().title().as_str().len())
                .map_err(|_| ContextBuildError::CountNotRepresentable)?;
            let body = u64::try_from(record.claim().body().as_str().len())
                .map_err(|_| ContextBuildError::CountNotRepresentable)?;
            title
                .checked_add(body)
                .ok_or(ContextBuildError::CountNotRepresentable)
        }
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "each independently observable omission count remains explicit"
)]
fn omissions(
    source_search: u64,
    source_expansion: u64,
    memory_available: bool,
    memory_recall: u64,
    memory_non_current: u64,
    memory_budget: u64,
    source_budget: u64,
) -> Vec<ContextOmission> {
    let mut omissions = Vec::with_capacity(10);
    if source_search != 0 {
        omissions.push(ContextOmission::SourceSearchLimit(source_search));
    }
    if source_expansion != 0 {
        omissions.push(ContextOmission::SourceExpansionLimit(source_expansion));
    }
    if memory_available {
        if memory_recall != 0 {
            omissions.push(ContextOmission::MemoryRecallLimit(memory_recall));
        }
        if memory_non_current != 0 {
            omissions.push(ContextOmission::MemoryNotCurrent(memory_non_current));
        }
    } else {
        omissions.push(ContextOmission::MemoryProjectionUnavailable);
    }
    if memory_budget != 0 {
        omissions.push(ContextOmission::Budget {
            provider: ContextProvider::Memory,
            count: memory_budget,
        });
    }
    if source_budget != 0 {
        omissions.push(ContextOmission::Budget {
            provider: ContextProvider::Source,
            count: source_budget,
        });
    }
    omissions.extend([
        ContextOmission::ProviderUnavailable(ContextProvider::Structural),
        ContextOmission::ProviderUnavailable(ContextProvider::References),
        ContextOmission::ProviderUnavailable(ContextProvider::History),
    ]);
    omissions
}

fn check_control(
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), ContextBuildError> {
    if cancelled.load(AtomicOrdering::Acquire) {
        Err(ContextBuildError::Cancelled)
    } else if Instant::now() >= deadline {
        Err(ContextBuildError::DeadlineExceeded)
    } else {
        Ok(())
    }
}
