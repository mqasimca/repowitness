#[allow(
    clippy::too_many_arguments,
    reason = "one exact evidence evaluation keeps its source, review, history, and control identities"
)]
fn evaluate_rust_evidence(
    writer: &OwnedSqliteIndex,
    source: MemoryProjectionSource,
    record_id: MemoryRecordId,
    revision: CanonicalMemoryDigest,
    evidence_ordinal: u8,
    approval_git_source: Option<MemoryCommitId>,
    evidence: &RustSymbolMemoryEvidence,
    queries: Option<&GitMemoryQueries>,
    head: Option<MemoryCommitId>,
    query_budget: &mut GitQueryBudget,
    cancelled: &Arc<AtomicBool>,
    deadline: Instant,
) -> Result<(MemoryEvidenceOutcome, PreparedProjectionEvidence), LocalMemoryRevalidationError> {
    let loaded = writer
        .load_rust_memory_candidates(source, evidence.clone(), Arc::clone(cancelled), deadline)
        .map_err(|source| LocalMemoryRevalidationError::CandidateLoad { source })?;
    let name_elided = loaded.subject_name_elided();
    let candidate_count = loaded.candidate_count_before_limit();
    let mut candidates = loaded.into_candidates();
    let complete_candidates = u64::try_from(candidates.len()).ok() == Some(candidate_count);
    let reviews = writer
        .load_memory_correspondence_reviews(
            source,
            record_id,
            revision,
            evidence_ordinal,
            Arc::clone(cancelled),
            deadline,
        )
        .map_err(|source| LocalMemoryRevalidationError::CandidateLoad { source })?;
    match reviews.decision() {
        CorrespondenceReviewDecision::Reviewed(target) => {
            return Ok((
                MemoryEvidenceOutcome::Corresponded,
                PreparedProjectionEvidence::reviewed_link(
                    target.clone(),
                    candidate_count,
                    complete_candidates,
                ),
            ));
        }
        CorrespondenceReviewDecision::Indeterminate => {
            return Ok((
                MemoryEvidenceOutcome::Indeterminate,
                PreparedProjectionEvidence::indeterminate(candidate_count),
            ));
        }
        CorrespondenceReviewDecision::None => {}
    }
    let subject = RustCorrespondenceSubject::try_new(
        evidence.path().clone(),
        analysis_symbol_kind(evidence.symbol_kind()),
        evidence.name().as_str().to_owned(),
        evidence.qualified_name().as_str().to_owned(),
        evidence.declaration_digest(),
        name_elided,
    )
    .map_err(|source| LocalMemoryRevalidationError::Correspondence { source })?;

    let exact_present = complete_candidates
        && candidates
            .iter()
            .any(|candidate| exact_evidence_candidate(evidence, candidate));
    let mut history_indeterminate = false;
    if complete_candidates
        && !exact_present
        && let (Some(source_commit), Some(target_commit), Some(queries)) =
            (approval_git_source, head, queries)
    {
        for candidate in &mut candidates {
            if !move_evidence_candidate(evidence, candidate) {
                continue;
            }
            query_budget.reserve()?;
            match queries
                .exact_path_continuity(
                    source_commit,
                    target_commit,
                    evidence.path(),
                    candidate.path(),
                    cancelled.as_ref(),
                    deadline,
                )
                .map_err(|source| LocalMemoryRevalidationError::GitQuery { source })?
            {
                GitPathContinuityOutcome::ExactMove => {
                    *candidate = candidate
                        .clone()
                        .with_path_continuity(RustPathContinuity::GitExactMove);
                }
                GitPathContinuityOutcome::NoMatch => {}
                GitPathContinuityOutcome::Indeterminate => {
                    history_indeterminate = true;
                }
            }
        }
    }
    if history_indeterminate {
        return Ok((
            MemoryEvidenceOutcome::Indeterminate,
            PreparedProjectionEvidence::indeterminate(candidate_count),
        ));
    }
    let resolution = resolve_rust_correspondence(&subject, &candidates, candidate_count)
        .map_err(|source| LocalMemoryRevalidationError::Correspondence { source })?;
    prepared_evidence_resolution_with_rejections(evidence, resolution, candidate_count, &reviews)
}

fn prepared_evidence_resolution_with_rejections(
    evidence: &RustSymbolMemoryEvidence,
    resolution: RustCorrespondenceResolution,
    candidate_count: u64,
    reviews: &LoadedCorrespondenceReviews,
) -> Result<(MemoryEvidenceOutcome, PreparedProjectionEvidence), LocalMemoryRevalidationError> {
    let resolution = match resolution {
        RustCorrespondenceResolution::Exact { target } if reviews.rejects_candidate(&target) => {
            return Ok((
                MemoryEvidenceOutcome::Indeterminate,
                PreparedProjectionEvidence::indeterminate(candidate_count),
            ));
        }
        RustCorrespondenceResolution::Automatic {
            relationship: _,
            target,
        }
        | RustCorrespondenceResolution::Changed { target }
            if reviews.rejects_candidate(&target) =>
        {
            return Ok((
                MemoryEvidenceOutcome::Indeterminate,
                PreparedProjectionEvidence::indeterminate(candidate_count),
            ));
        }
        RustCorrespondenceResolution::NeedsReview { mut candidates } => {
            let prior_count = candidates.len();
            candidates.retain(|candidate| !reviews.rejects_candidate(candidate));
            if candidates.is_empty() && prior_count != 0 {
                return Ok((
                    MemoryEvidenceOutcome::Indeterminate,
                    PreparedProjectionEvidence::indeterminate(candidate_count),
                ));
            }
            RustCorrespondenceResolution::NeedsReview { candidates }
        }
        other => other,
    };
    prepared_evidence_resolution(evidence, resolution, candidate_count)
}

fn prepared_evidence_resolution(
    evidence: &RustSymbolMemoryEvidence,
    resolution: RustCorrespondenceResolution,
    candidate_count: u64,
) -> Result<(MemoryEvidenceOutcome, PreparedProjectionEvidence), LocalMemoryRevalidationError> {
    match resolution {
        RustCorrespondenceResolution::Exact { target } => Ok((
            MemoryEvidenceOutcome::Exact,
            PreparedProjectionEvidence::resolved(
                ProjectionEvidenceOutcome::Exact,
                ProjectionEvidenceAssurance::Automatic,
                ProjectionOccurrence::from_candidate(&target),
                candidate_count,
            )
            .map_err(|source| LocalMemoryRevalidationError::ProjectionPreparation { source })?,
        )),
        RustCorrespondenceResolution::Automatic {
            relationship,
            target,
        } => Ok((
            MemoryEvidenceOutcome::Corresponded,
            PreparedProjectionEvidence::resolved(
                match relationship {
                    RustAutomaticCorrespondence::Renamed => {
                        ProjectionEvidenceOutcome::SamePathRename
                    }
                    RustAutomaticCorrespondence::Moved => ProjectionEvidenceOutcome::GitExactMove,
                },
                ProjectionEvidenceAssurance::Automatic,
                ProjectionOccurrence::from_candidate(&target),
                candidate_count,
            )
            .map_err(|source| LocalMemoryRevalidationError::ProjectionPreparation { source })?,
        )),
        RustCorrespondenceResolution::Changed { target } => Ok((
            MemoryEvidenceOutcome::Changed,
            PreparedProjectionEvidence::resolved(
                ProjectionEvidenceOutcome::Changed,
                ProjectionEvidenceAssurance::None,
                ProjectionOccurrence::from_candidate(&target),
                candidate_count,
            )
            .map_err(|source| LocalMemoryRevalidationError::ProjectionPreparation { source })?,
        )),
        RustCorrespondenceResolution::NeedsReview { candidates } => {
            let candidates = candidates
                .iter()
                .map(|candidate| PreparedProjectionCandidate {
                    occurrence: ProjectionOccurrence::from_candidate(candidate),
                    relation: candidate_relation(evidence, candidate),
                })
                .collect();
            Ok((
                MemoryEvidenceOutcome::NeedsReview,
                PreparedProjectionEvidence::ambiguous(candidates).map_err(|source| {
                    LocalMemoryRevalidationError::ProjectionPreparation { source }
                })?,
            ))
        }
        RustCorrespondenceResolution::Missing => Ok((
            MemoryEvidenceOutcome::Missing,
            PreparedProjectionEvidence::missing(candidate_count),
        )),
        RustCorrespondenceResolution::Indeterminate { .. } => Ok((
            MemoryEvidenceOutcome::Indeterminate,
            PreparedProjectionEvidence::indeterminate(candidate_count),
        )),
    }
}

fn exact_evidence_candidate(
    evidence: &RustSymbolMemoryEvidence,
    candidate: &RustCorrespondenceCandidate,
) -> bool {
    candidate.path() == evidence.path()
        && candidate.kind() == analysis_symbol_kind(evidence.symbol_kind())
        && candidate.name() == evidence.name().as_str()
        && candidate.qualified_name() == evidence.qualified_name().as_str()
        && candidate.fingerprint().declaration() == evidence.declaration_digest()
}

fn move_evidence_candidate(
    evidence: &RustSymbolMemoryEvidence,
    candidate: &RustCorrespondenceCandidate,
) -> bool {
    candidate.path() != evidence.path()
        && candidate.kind() == analysis_symbol_kind(evidence.symbol_kind())
        && candidate.name() == evidence.name().as_str()
        && candidate.qualified_name() == evidence.qualified_name().as_str()
        && candidate.fingerprint().declaration() == evidence.declaration_digest()
}

fn candidate_relation(
    evidence: &RustSymbolMemoryEvidence,
    candidate: &RustCorrespondenceCandidate,
) -> ProjectionCandidateRelation {
    match (
        candidate.path() == evidence.path(),
        candidate.name() == evidence.name().as_str(),
    ) {
        (true, true) => ProjectionCandidateRelation::Same,
        (true, false) => ProjectionCandidateRelation::Renamed,
        (false, true) => ProjectionCandidateRelation::Moved,
        (false, false) => ProjectionCandidateRelation::MovedRenamed,
    }
}

const fn analysis_symbol_kind(kind: RustMemorySymbolKind) -> RustSymbolKind {
    match kind {
        RustMemorySymbolKind::Function => RustSymbolKind::Function,
        RustMemorySymbolKind::Method => RustSymbolKind::Method,
        RustMemorySymbolKind::Struct => RustSymbolKind::Struct,
        RustMemorySymbolKind::Enum => RustSymbolKind::Enum,
        RustMemorySymbolKind::Union => RustSymbolKind::Union,
        RustMemorySymbolKind::Trait => RustSymbolKind::Trait,
        RustMemorySymbolKind::Module => RustSymbolKind::Module,
        RustMemorySymbolKind::TypeAlias => RustSymbolKind::TypeAlias,
        RustMemorySymbolKind::Constant => RustSymbolKind::Constant,
        RustMemorySymbolKind::Static => RustSymbolKind::Static,
        RustMemorySymbolKind::Macro => RustSymbolKind::Macro,
    }
}
