struct PersistedProjectionRecord {
    revision: Option<Vec<u8>>,
    effective: &'static str,
    validity: &'static str,
    evidence: &'static str,
    reason: &'static str,
    evidence_count: u32,
    resolved_count: u32,
    review_count: u32,
    indeterminate_count: u32,
    head_count: u32,
    missing_parent_count: u32,
}

fn persisted_record(record: &PreparedProjectionRecord) -> PersistedProjectionRecord {
    match &record.kind {
        PreparedProjectionRecordKind::Evaluated {
            revision, decision, ..
        } => PersistedProjectionRecord {
            revision: Some(revision.as_bytes().to_vec()),
            effective: effective_state(decision.effective_state()),
            validity: validity_state(decision.validity_state()),
            evidence: evidence_state(decision.evidence_state()),
            reason: projection_reason(decision.reason()),
            evidence_count: decision.evidence_count(),
            resolved_count: decision.resolved_count(),
            review_count: decision.review_count(),
            indeterminate_count: decision.indeterminate_count(),
            head_count: 1,
            missing_parent_count: 0,
        },
        PreparedProjectionRecordKind::Conflicted { head_count } => PersistedProjectionRecord {
            revision: None,
            effective: "conflicted",
            validity: "not_evaluated",
            evidence: "conflicted",
            reason: "approved_head_conflict",
            evidence_count: 0,
            resolved_count: 0,
            review_count: 0,
            indeterminate_count: 0,
            head_count: *head_count,
            missing_parent_count: 0,
        },
        PreparedProjectionRecordKind::IndeterminateHead {
            revision,
            evidence_count,
            head_count,
            missing_parent_count,
            reason,
        } => PersistedProjectionRecord {
            revision: revision.map(|revision| revision.as_bytes().to_vec()),
            effective: "indeterminate",
            validity: "not_evaluated",
            evidence: "not_evaluated",
            reason: projection_head_reason(*reason),
            evidence_count: *evidence_count,
            resolved_count: 0,
            review_count: 0,
            indeterminate_count: 0,
            head_count: *head_count,
            missing_parent_count: *missing_parent_count,
        },
    }
}

struct PersistedProjectionTarget {
    kind: &'static str,
    format: &'static str,
    revision: Vec<u8>,
    head_format: Option<&'static str>,
    head_revision: Option<Vec<u8>>,
}

fn projection_target(target: MemoryRevalidationTarget) -> PersistedProjectionTarget {
    match target {
        MemoryRevalidationTarget::Git { commit } => PersistedProjectionTarget {
            kind: "git",
            format: commit_format(commit),
            revision: commit.as_bytes().to_vec(),
            head_format: None,
            head_revision: None,
        },
        MemoryRevalidationTarget::Worktree {
            source_snapshot,
            head,
        } => PersistedProjectionTarget {
            kind: "worktree",
            format: "source_snapshot",
            revision: source_snapshot.as_bytes().to_vec(),
            head_format: head.map(commit_format),
            head_revision: head.map(|commit| commit.as_bytes().to_vec()),
        },
    }
}

const fn commit_format(commit: MemoryCommitId) -> &'static str {
    match commit {
        MemoryCommitId::Sha1(_) => "sha1",
        MemoryCommitId::Sha256(_) => "sha256",
    }
}

const fn effective_state(state: MemoryEffectiveState) -> &'static str {
    match state {
        MemoryEffectiveState::Current => "current",
        MemoryEffectiveState::NotApplicable => "not_applicable",
        MemoryEffectiveState::Stale => "stale",
        MemoryEffectiveState::NeedsReview => "needs_review",
        MemoryEffectiveState::Indeterminate => "indeterminate",
        MemoryEffectiveState::Conflicted => "conflicted",
        MemoryEffectiveState::Contradicted => "contradicted",
        MemoryEffectiveState::Superseded => "superseded",
        MemoryEffectiveState::Quarantined => "quarantined",
        MemoryEffectiveState::Tombstoned => "tombstoned",
    }
}

const fn validity_state(state: MemoryProjectionValidityState) -> &'static str {
    match state {
        MemoryProjectionValidityState::Valid => "valid",
        MemoryProjectionValidityState::Invalid => "invalid",
        MemoryProjectionValidityState::Indeterminate => "indeterminate",
        MemoryProjectionValidityState::NotEvaluated => "not_evaluated",
    }
}

const fn evidence_state(state: MemoryProjectionEvidenceState) -> &'static str {
    match state {
        MemoryProjectionEvidenceState::Exact => "exact",
        MemoryProjectionEvidenceState::Corresponded => "corresponded",
        MemoryProjectionEvidenceState::Changed => "changed",
        MemoryProjectionEvidenceState::Ambiguous => "ambiguous",
        MemoryProjectionEvidenceState::Missing => "missing",
        MemoryProjectionEvidenceState::Indeterminate => "indeterminate",
        MemoryProjectionEvidenceState::NotEvaluated => "not_evaluated",
    }
}

const fn projection_reason(reason: MemoryProjectionReason) -> &'static str {
    match reason {
        MemoryProjectionReason::EvidenceExact => "evidence_exact",
        MemoryProjectionReason::EvidenceCorresponded => "evidence_corresponded",
        MemoryProjectionReason::EvidenceChanged => "evidence_changed",
        MemoryProjectionReason::EvidenceAmbiguous => "evidence_ambiguous",
        MemoryProjectionReason::EvidenceMissing => "evidence_missing",
        MemoryProjectionReason::EvidenceIndeterminate => "evidence_indeterminate",
        MemoryProjectionReason::ProjectNotApplicable => "project_not_applicable",
        MemoryProjectionReason::ProjectIndeterminate => "project_indeterminate",
        MemoryProjectionReason::AuthoredNeedsReview => "authored_needs_review",
        MemoryProjectionReason::AuthoredStale => "authored_stale",
        MemoryProjectionReason::AuthoredContradicted => "authored_contradicted",
        MemoryProjectionReason::AuthoredSuperseded => "authored_superseded",
        MemoryProjectionReason::AuthoredQuarantined => "authored_quarantined",
        MemoryProjectionReason::AuthoredTombstoned => "authored_tombstoned",
    }
}

const fn projection_head_reason(reason: ProjectionHeadReason) -> &'static str {
    match reason {
        ProjectionHeadReason::MissingParent => "missing_parent",
        ProjectionHeadReason::InvalidHeadGraph => "invalid_head_graph",
    }
}

const fn projection_evidence_outcome(outcome: ProjectionEvidenceOutcome) -> &'static str {
    match outcome {
        ProjectionEvidenceOutcome::Exact => "exact",
        ProjectionEvidenceOutcome::SamePathRename => "same_path_rename",
        ProjectionEvidenceOutcome::GitExactMove => "git_exact_move",
        ProjectionEvidenceOutcome::ReviewedLink => "reviewed_link",
        ProjectionEvidenceOutcome::Changed => "changed",
        ProjectionEvidenceOutcome::Ambiguous => "ambiguous",
        ProjectionEvidenceOutcome::Missing => "missing",
        ProjectionEvidenceOutcome::Indeterminate => "indeterminate",
    }
}

const fn projection_evidence_method(
    outcome: ProjectionEvidenceOutcome,
) -> (&'static str, u32) {
    if matches!(outcome, ProjectionEvidenceOutcome::ReviewedLink) {
        (MANUAL_REVIEW_METHOD_ID, MANUAL_REVIEW_METHOD_VERSION)
    } else {
        (
            RUST_CORRESPONDENCE_PROFILE_ID,
            RUST_CORRESPONDENCE_PROFILE_VERSION,
        )
    }
}

const fn projection_assurance(assurance: ProjectionEvidenceAssurance) -> &'static str {
    match assurance {
        ProjectionEvidenceAssurance::Automatic => "automatic",
        ProjectionEvidenceAssurance::Reviewed => "reviewed",
        ProjectionEvidenceAssurance::None => "none",
    }
}

const fn projection_candidate_relation(relation: ProjectionCandidateRelation) -> &'static str {
    match relation {
        ProjectionCandidateRelation::Same => "same",
        ProjectionCandidateRelation::Moved => "moved",
        ProjectionCandidateRelation::Renamed => "renamed",
        ProjectionCandidateRelation::MovedRenamed => "moved_renamed",
        ProjectionCandidateRelation::Split => "split",
        ProjectionCandidateRelation::Merged => "merged",
    }
}

fn occurrence_order(
    left: &ProjectionOccurrence,
    right: &ProjectionOccurrence,
) -> std::cmp::Ordering {
    left.path
        .cmp(&right.path)
        .then_with(|| left.artifact.cmp(&right.artifact))
        .then_with(|| left.fact_ordinal.cmp(&right.fact_ordinal))
}

fn fixed_integer(value: u64) -> Result<i64, SqliteStoreError> {
    i64::try_from(value).map_err(|_| SqliteStoreError::CountNotRepresentable)
}
