use repowitness_application::{
    MemoryEffectiveState, MemoryProjectionValidityState, MemoryRecallCandidateRelation,
    MemoryRecallEvidenceAssurance, MemoryRecallEvidenceOutcome, MemoryRecallEvidenceState,
    MemoryRecallReason,
};
use repowitness_domain::{
    MemoryCommitId, MemoryRecordId, MemoryRevalidationTarget, SourceSnapshotDigest,
};

use super::{RecallFailure, SqliteStoreError};

pub(super) fn parse_target(
    kind: &str,
    format: &str,
    revision: &[u8],
    head_format: Option<&str>,
    head_revision: Option<&[u8]>,
    snapshot: SourceSnapshotDigest,
) -> Result<MemoryRevalidationTarget, RecallFailure> {
    match kind {
        "git" if head_format.is_none() && head_revision.is_none() => Ok(
            MemoryRevalidationTarget::git(parse_commit(format, revision)?),
        ),
        "worktree" if format == "source_snapshot" && revision == snapshot.as_bytes() => {
            let head = match (head_format, head_revision) {
                (None, None) => None,
                (Some(format), Some(revision)) => Some(parse_commit(format, revision)?),
                _ => return Err(integrity_failure()),
            };
            Ok(MemoryRevalidationTarget::worktree(snapshot, head))
        }
        _ => Err(integrity_failure()),
    }
}

fn parse_commit(format: &str, revision: &[u8]) -> Result<MemoryCommitId, RecallFailure> {
    match format {
        "sha1" => <[u8; 20]>::try_from(revision)
            .map(MemoryCommitId::Sha1)
            .map_err(|_| integrity_failure()),
        "sha256" => <[u8; 32]>::try_from(revision)
            .map(MemoryCommitId::Sha256)
            .map_err(|_| integrity_failure()),
        _ => Err(integrity_failure()),
    }
}

pub(super) fn parse_effective_state(value: &str) -> Result<MemoryEffectiveState, RecallFailure> {
    match value {
        "current" => Ok(MemoryEffectiveState::Current),
        "not_applicable" => Ok(MemoryEffectiveState::NotApplicable),
        "stale" => Ok(MemoryEffectiveState::Stale),
        "needs_review" => Ok(MemoryEffectiveState::NeedsReview),
        "indeterminate" => Ok(MemoryEffectiveState::Indeterminate),
        "conflicted" => Ok(MemoryEffectiveState::Conflicted),
        "contradicted" => Ok(MemoryEffectiveState::Contradicted),
        "superseded" => Ok(MemoryEffectiveState::Superseded),
        "quarantined" => Ok(MemoryEffectiveState::Quarantined),
        "tombstoned" => Ok(MemoryEffectiveState::Tombstoned),
        _ => Err(integrity_failure()),
    }
}

pub(super) fn parse_validity_state(
    value: &str,
) -> Result<MemoryProjectionValidityState, RecallFailure> {
    match value {
        "valid" => Ok(MemoryProjectionValidityState::Valid),
        "invalid" => Ok(MemoryProjectionValidityState::Invalid),
        "indeterminate" => Ok(MemoryProjectionValidityState::Indeterminate),
        "not_evaluated" => Ok(MemoryProjectionValidityState::NotEvaluated),
        _ => Err(integrity_failure()),
    }
}

pub(super) fn parse_evidence_state(
    value: &str,
) -> Result<MemoryRecallEvidenceState, RecallFailure> {
    match value {
        "exact" => Ok(MemoryRecallEvidenceState::Exact),
        "corresponded" => Ok(MemoryRecallEvidenceState::Corresponded),
        "changed" => Ok(MemoryRecallEvidenceState::Changed),
        "ambiguous" => Ok(MemoryRecallEvidenceState::Ambiguous),
        "missing" => Ok(MemoryRecallEvidenceState::Missing),
        "indeterminate" => Ok(MemoryRecallEvidenceState::Indeterminate),
        "conflicted" => Ok(MemoryRecallEvidenceState::Conflicted),
        "not_evaluated" => Ok(MemoryRecallEvidenceState::NotEvaluated),
        _ => Err(integrity_failure()),
    }
}

pub(super) fn parse_reason(value: &str) -> Result<MemoryRecallReason, RecallFailure> {
    match value {
        "evidence_exact" => Ok(MemoryRecallReason::EvidenceExact),
        "evidence_corresponded" => Ok(MemoryRecallReason::EvidenceCorresponded),
        "evidence_changed" => Ok(MemoryRecallReason::EvidenceChanged),
        "evidence_ambiguous" => Ok(MemoryRecallReason::EvidenceAmbiguous),
        "evidence_missing" => Ok(MemoryRecallReason::EvidenceMissing),
        "evidence_indeterminate" => Ok(MemoryRecallReason::EvidenceIndeterminate),
        "project_not_applicable" => Ok(MemoryRecallReason::ProjectNotApplicable),
        "project_indeterminate" => Ok(MemoryRecallReason::ProjectIndeterminate),
        "authored_needs_review" => Ok(MemoryRecallReason::AuthoredNeedsReview),
        "authored_stale" => Ok(MemoryRecallReason::AuthoredStale),
        "authored_contradicted" => Ok(MemoryRecallReason::AuthoredContradicted),
        "authored_superseded" => Ok(MemoryRecallReason::AuthoredSuperseded),
        "authored_quarantined" => Ok(MemoryRecallReason::AuthoredQuarantined),
        "authored_tombstoned" => Ok(MemoryRecallReason::AuthoredTombstoned),
        "approved_head_conflict" => Ok(MemoryRecallReason::ApprovedHeadConflict),
        "missing_parent" => Ok(MemoryRecallReason::MissingParent),
        "invalid_head_graph" => Ok(MemoryRecallReason::InvalidHeadGraph),
        _ => Err(integrity_failure()),
    }
}

pub(super) fn parse_evidence_outcome(
    value: &str,
) -> Result<MemoryRecallEvidenceOutcome, RecallFailure> {
    match value {
        "exact" => Ok(MemoryRecallEvidenceOutcome::Exact),
        "same_path_rename" => Ok(MemoryRecallEvidenceOutcome::SamePathRename),
        "git_exact_move" => Ok(MemoryRecallEvidenceOutcome::GitExactMove),
        "reviewed_link" => Ok(MemoryRecallEvidenceOutcome::ReviewedLink),
        "changed" => Ok(MemoryRecallEvidenceOutcome::Changed),
        "ambiguous" => Ok(MemoryRecallEvidenceOutcome::Ambiguous),
        "missing" => Ok(MemoryRecallEvidenceOutcome::Missing),
        "indeterminate" => Ok(MemoryRecallEvidenceOutcome::Indeterminate),
        _ => Err(integrity_failure()),
    }
}

pub(super) fn parse_evidence_assurance(
    value: &str,
) -> Result<MemoryRecallEvidenceAssurance, RecallFailure> {
    match value {
        "automatic" => Ok(MemoryRecallEvidenceAssurance::Automatic),
        "reviewed" => Ok(MemoryRecallEvidenceAssurance::Reviewed),
        "none" => Ok(MemoryRecallEvidenceAssurance::None),
        _ => Err(integrity_failure()),
    }
}

pub(super) fn parse_candidate_relation(
    value: &str,
) -> Result<MemoryRecallCandidateRelation, RecallFailure> {
    match value {
        "same" => Ok(MemoryRecallCandidateRelation::Same),
        "moved" => Ok(MemoryRecallCandidateRelation::Moved),
        "renamed" => Ok(MemoryRecallCandidateRelation::Renamed),
        "moved_renamed" => Ok(MemoryRecallCandidateRelation::MovedRenamed),
        "split" => Ok(MemoryRecallCandidateRelation::Split),
        "merged" => Ok(MemoryRecallCandidateRelation::Merged),
        _ => Err(integrity_failure()),
    }
}

pub(super) fn memory_record_id(bytes: &[u8]) -> Result<MemoryRecordId, RecallFailure> {
    <[u8; 16]>::try_from(bytes)
        .map(MemoryRecordId::new)
        .map_err(|_| integrity_failure())
}

pub(super) fn persisted_count(value: i64) -> Result<u64, RecallFailure> {
    u64::try_from(value).map_err(|_| integrity_failure())
}

pub(super) fn persisted_u32(value: i64) -> Result<u32, RecallFailure> {
    u32::try_from(value).map_err(|_| integrity_failure())
}

pub(super) fn integrity_failure() -> RecallFailure {
    RecallFailure::Store(SqliteStoreError::IntegrityCheckFailed)
}
