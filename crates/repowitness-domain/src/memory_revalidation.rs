use std::{error::Error, fmt};

use crate::{MemoryCommitId, MemoryValidity, SourceSnapshotDigest};

/// Maximum ancestry outcomes consumed by one version-1 validity evaluation.
pub const MAX_MEMORY_ANCESTRY_CHECKS: usize = 2 * crate::MAX_MEMORY_COMMITS;

/// Categorical result of one exact Git ancestry query.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryAncestryOutcome {
    /// The supplied ancestor is proven reachable from the descendant.
    Ancestor,
    /// The supplied ancestor is proven not reachable from the descendant.
    NotAncestor,
    /// Required objects or complete history were unavailable.
    Indeterminate,
}

/// One attributed ancestry outcome over exact commit identities.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct MemoryAncestryCheck {
    ancestor: MemoryCommitId,
    descendant: MemoryCommitId,
    outcome: MemoryAncestryOutcome,
}

impl MemoryAncestryCheck {
    /// Creates one exact ancestry result returned by a trusted adapter.
    #[must_use]
    pub const fn new(
        ancestor: MemoryCommitId,
        descendant: MemoryCommitId,
        outcome: MemoryAncestryOutcome,
    ) -> Self {
        Self {
            ancestor,
            descendant,
            outcome,
        }
    }

    /// Returns the queried ancestor.
    #[must_use]
    pub const fn ancestor(self) -> MemoryCommitId {
        self.ancestor
    }

    /// Returns the concrete query target.
    #[must_use]
    pub const fn descendant(self) -> MemoryCommitId {
        self.descendant
    }

    /// Returns the categorical adapter result.
    #[must_use]
    pub const fn outcome(self) -> MemoryAncestryOutcome {
        self.outcome
    }
}

impl fmt::Debug for MemoryAncestryCheck {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MemoryAncestryCheck")
            .field("ancestor", &self.ancestor)
            .field("descendant", &self.descendant)
            .field("outcome", &self.outcome)
            .finish()
    }
}

/// Concrete source target against which project validity is evaluated.
#[derive(Clone, Copy, Eq, PartialEq)]
pub enum MemoryRevalidationTarget {
    /// One exact Git commit.
    Git {
        /// Concrete target commit.
        commit: MemoryCommitId,
    },
    /// One exact worktree snapshot and its optional concrete HEAD.
    Worktree {
        /// Complete source-snapshot identity.
        source_snapshot: SourceSnapshotDigest,
        /// Concrete HEAD used only for commit-scoped project validity.
        head: Option<MemoryCommitId>,
    },
}

impl MemoryRevalidationTarget {
    /// Creates a concrete Git target.
    #[must_use]
    pub const fn git(commit: MemoryCommitId) -> Self {
        Self::Git { commit }
    }

    /// Creates an exact worktree target.
    #[must_use]
    pub const fn worktree(
        source_snapshot: SourceSnapshotDigest,
        head: Option<MemoryCommitId>,
    ) -> Self {
        Self::Worktree {
            source_snapshot,
            head,
        }
    }

    const fn ancestry_target(self) -> Option<MemoryCommitId> {
        match self {
            Self::Git { commit } => Some(commit),
            Self::Worktree { head, .. } => head,
        }
    }
}

impl fmt::Debug for MemoryRevalidationTarget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Git { commit } => formatter
                .debug_struct("MemoryRevalidationTarget")
                .field("kind", &"git")
                .field("commit", commit)
                .finish_non_exhaustive(),
            Self::Worktree { head, .. } => formatter
                .debug_struct("MemoryRevalidationTarget")
                .field("kind", &"worktree")
                .field("snapshot_digest", &"SHA-256")
                .field("has_head", &head.is_some())
                .finish_non_exhaustive(),
        }
    }
}

/// Project-valid result for one exact memory version and source target.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryProjectValidity {
    /// Introduction is proven and no invalidation is reachable.
    Valid,
    /// The record was not introduced or has been invalidated at the target.
    NotApplicable,
    /// Missing objects, history, or a concrete target prevents a conclusion.
    Indeterminate,
}

/// Stable, content-redacted validity-evaluation failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryValidityEvaluationError {
    /// More ancestry checks were supplied than the version-1 bound.
    TooManyChecks,
    /// Checks were missing, duplicated, unexpected, or used another descendant.
    InvalidChecks,
}

impl fmt::Display for MemoryValidityEvaluationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::TooManyChecks => "memory ancestry check limit exceeded",
            Self::InvalidChecks => "memory ancestry checks are invalid",
        })
    }
}

impl Error for MemoryValidityEvaluationError {}

/// Evaluates ADR-0005 project validity from complete attributed ancestry
/// outcomes. This function performs no Git or filesystem I/O.
pub fn evaluate_memory_project_validity(
    validity: &MemoryValidity,
    target: MemoryRevalidationTarget,
    checks: &[MemoryAncestryCheck],
) -> Result<MemoryProjectValidity, MemoryValidityEvaluationError> {
    if checks.len() > MAX_MEMORY_ANCESTRY_CHECKS {
        return Err(MemoryValidityEvaluationError::TooManyChecks);
    }

    match validity {
        MemoryValidity::Worktree { source_snapshot } => {
            if !checks.is_empty() {
                return Err(MemoryValidityEvaluationError::InvalidChecks);
            }
            Ok(match target {
                MemoryRevalidationTarget::Worktree {
                    source_snapshot: target_snapshot,
                    ..
                } if source_snapshot == &target_snapshot => MemoryProjectValidity::Valid,
                _ => MemoryProjectValidity::NotApplicable,
            })
        }
        MemoryValidity::Commits {
            introduced_by,
            invalidated_by,
        } => {
            let Some(descendant) = target.ancestry_target() else {
                if checks.is_empty() {
                    return Ok(MemoryProjectValidity::Indeterminate);
                }
                return Err(MemoryValidityEvaluationError::InvalidChecks);
            };
            validate_checks(introduced_by, invalidated_by, descendant, checks)?;

            let introduction = aggregate_side(introduced_by, checks);
            let invalidation = aggregate_side(invalidated_by, checks);
            if invalidation == SideOutcome::Reachable {
                return Ok(MemoryProjectValidity::NotApplicable);
            }
            if introduction == SideOutcome::NoneReachable {
                return Ok(MemoryProjectValidity::NotApplicable);
            }
            if introduction == SideOutcome::Indeterminate {
                return Ok(MemoryProjectValidity::Indeterminate);
            }

            Ok(match invalidation {
                SideOutcome::Indeterminate => MemoryProjectValidity::Indeterminate,
                SideOutcome::NoneReachable => MemoryProjectValidity::Valid,
                SideOutcome::Reachable => unreachable!("reachable invalidation returned above"),
            })
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum SideOutcome {
    Reachable,
    NoneReachable,
    Indeterminate,
}

fn aggregate_side(commits: &[MemoryCommitId], checks: &[MemoryAncestryCheck]) -> SideOutcome {
    let mut indeterminate = false;
    for commit in commits {
        let outcome = checks
            .iter()
            .find(|check| check.ancestor == *commit)
            .map(|check| check.outcome)
            .expect("validated ancestry check set");
        match outcome {
            MemoryAncestryOutcome::Ancestor => return SideOutcome::Reachable,
            MemoryAncestryOutcome::NotAncestor => {}
            MemoryAncestryOutcome::Indeterminate => indeterminate = true,
        }
    }
    if indeterminate {
        SideOutcome::Indeterminate
    } else {
        SideOutcome::NoneReachable
    }
}

fn validate_checks(
    introduced_by: &[MemoryCommitId],
    invalidated_by: &[MemoryCommitId],
    descendant: MemoryCommitId,
    checks: &[MemoryAncestryCheck],
) -> Result<(), MemoryValidityEvaluationError> {
    let expected = introduced_by.len() + invalidated_by.len();
    if checks.len() != expected
        || checks.iter().any(|check| check.descendant != descendant)
        || checks.iter().enumerate().any(|(index, check)| {
            checks[..index]
                .iter()
                .any(|prior| prior.ancestor == check.ancestor)
        })
        || introduced_by
            .iter()
            .chain(invalidated_by)
            .any(|commit| !checks.iter().any(|check| check.ancestor == *commit))
    {
        return Err(MemoryValidityEvaluationError::InvalidChecks);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        MemoryAncestryCheck, MemoryAncestryOutcome, MemoryProjectValidity,
        MemoryRevalidationTarget, MemoryValidityEvaluationError, evaluate_memory_project_validity,
    };
    use crate::{MemoryCommitId, MemoryValidity, SourceSnapshotDigest};

    fn commit(byte: u8) -> MemoryCommitId {
        MemoryCommitId::Sha1([byte; 20])
    }

    fn check(
        ancestor: MemoryCommitId,
        descendant: MemoryCommitId,
        outcome: MemoryAncestryOutcome,
    ) -> MemoryAncestryCheck {
        MemoryAncestryCheck::new(ancestor, descendant, outcome)
    }

    #[test]
    fn introduction_and_invalidation_follow_git_dag_outcomes() {
        let introduced = commit(0x11);
        let invalidated = commit(0x22);
        let target = commit(0x33);
        let validity =
            MemoryValidity::try_commits(vec![introduced], vec![invalidated]).expect("validity");

        assert_eq!(
            evaluate_memory_project_validity(
                &validity,
                MemoryRevalidationTarget::git(target),
                &[
                    check(introduced, target, MemoryAncestryOutcome::Ancestor),
                    check(invalidated, target, MemoryAncestryOutcome::NotAncestor),
                ],
            ),
            Ok(MemoryProjectValidity::Valid)
        );
        assert_eq!(
            evaluate_memory_project_validity(
                &validity,
                MemoryRevalidationTarget::git(target),
                &[
                    check(introduced, target, MemoryAncestryOutcome::Ancestor),
                    check(invalidated, target, MemoryAncestryOutcome::Ancestor),
                ],
            ),
            Ok(MemoryProjectValidity::NotApplicable)
        );
    }

    #[test]
    fn missing_history_never_becomes_valid_or_not_ancestor() {
        let introduced = commit(0x11);
        let invalidated = commit(0x22);
        let target = commit(0x33);
        let validity =
            MemoryValidity::try_commits(vec![introduced], vec![invalidated]).expect("validity");

        assert_eq!(
            evaluate_memory_project_validity(
                &validity,
                MemoryRevalidationTarget::git(target),
                &[
                    check(introduced, target, MemoryAncestryOutcome::Ancestor),
                    check(invalidated, target, MemoryAncestryOutcome::Indeterminate),
                ],
            ),
            Ok(MemoryProjectValidity::Indeterminate)
        );
        assert_eq!(
            evaluate_memory_project_validity(
                &validity,
                MemoryRevalidationTarget::git(target),
                &[
                    check(introduced, target, MemoryAncestryOutcome::Indeterminate),
                    check(invalidated, target, MemoryAncestryOutcome::NotAncestor),
                ],
            ),
            Ok(MemoryProjectValidity::Indeterminate)
        );
    }

    #[test]
    fn a_proven_non_introduction_is_not_applicable() {
        let introduced = commit(0x11);
        let invalidated = commit(0x22);
        let target = commit(0x33);
        let validity =
            MemoryValidity::try_commits(vec![introduced], vec![invalidated]).expect("validity");

        assert_eq!(
            evaluate_memory_project_validity(
                &validity,
                MemoryRevalidationTarget::git(target),
                &[
                    check(introduced, target, MemoryAncestryOutcome::NotAncestor),
                    check(invalidated, target, MemoryAncestryOutcome::Indeterminate),
                ],
            ),
            Ok(MemoryProjectValidity::NotApplicable)
        );
    }

    #[test]
    fn a_proven_invalidation_overrides_indeterminate_introduction_history() {
        let introduced = commit(0x11);
        let invalidated = commit(0x22);
        let target = commit(0x33);
        let validity =
            MemoryValidity::try_commits(vec![introduced], vec![invalidated]).expect("validity");

        assert_eq!(
            evaluate_memory_project_validity(
                &validity,
                MemoryRevalidationTarget::git(target),
                &[
                    check(introduced, target, MemoryAncestryOutcome::Indeterminate,),
                    check(invalidated, target, MemoryAncestryOutcome::Ancestor),
                ],
            ),
            Ok(MemoryProjectValidity::NotApplicable)
        );
    }

    #[test]
    fn worktree_validity_requires_the_exact_snapshot() {
        let expected = SourceSnapshotDigest::new([0x44; 32]);
        let validity = MemoryValidity::worktree(expected);

        assert_eq!(
            evaluate_memory_project_validity(
                &validity,
                MemoryRevalidationTarget::worktree(expected, Some(commit(0x33))),
                &[],
            ),
            Ok(MemoryProjectValidity::Valid)
        );
        assert_eq!(
            evaluate_memory_project_validity(
                &validity,
                MemoryRevalidationTarget::worktree(
                    SourceSnapshotDigest::new([0x55; 32]),
                    Some(commit(0x33)),
                ),
                &[],
            ),
            Ok(MemoryProjectValidity::NotApplicable)
        );
        assert_eq!(
            evaluate_memory_project_validity(
                &validity,
                MemoryRevalidationTarget::git(commit(0x33)),
                &[],
            ),
            Ok(MemoryProjectValidity::NotApplicable)
        );
    }

    #[test]
    fn an_unborn_worktree_makes_commit_validity_indeterminate() {
        let validity =
            MemoryValidity::try_commits(vec![commit(0x11)], Vec::new()).expect("validity");

        assert_eq!(
            evaluate_memory_project_validity(
                &validity,
                MemoryRevalidationTarget::worktree(SourceSnapshotDigest::new([0x44; 32]), None,),
                &[],
            ),
            Ok(MemoryProjectValidity::Indeterminate)
        );
    }

    #[test]
    fn malformed_or_mixed_check_sets_fail_closed() {
        let introduced = commit(0x11);
        let target = commit(0x33);
        let validity = MemoryValidity::try_commits(vec![introduced], Vec::new()).expect("validity");
        let valid = check(introduced, target, MemoryAncestryOutcome::Ancestor);

        assert_eq!(
            evaluate_memory_project_validity(&validity, MemoryRevalidationTarget::git(target), &[],),
            Err(MemoryValidityEvaluationError::InvalidChecks)
        );
        assert_eq!(
            evaluate_memory_project_validity(
                &validity,
                MemoryRevalidationTarget::git(target),
                &[valid, valid],
            ),
            Err(MemoryValidityEvaluationError::InvalidChecks)
        );
        assert_eq!(
            evaluate_memory_project_validity(
                &validity,
                MemoryRevalidationTarget::git(target),
                &[check(
                    introduced,
                    commit(0x44),
                    MemoryAncestryOutcome::Ancestor,
                )],
            ),
            Err(MemoryValidityEvaluationError::InvalidChecks)
        );
    }

    #[test]
    fn debug_output_redacts_commit_and_snapshot_bytes() {
        let target = MemoryRevalidationTarget::worktree(
            SourceSnapshotDigest::new([0xA5; 32]),
            Some(MemoryCommitId::Sha1([0xA5; 20])),
        );
        let check = MemoryAncestryCheck::new(
            MemoryCommitId::Sha1([0xA5; 20]),
            MemoryCommitId::Sha1([0xA5; 20]),
            MemoryAncestryOutcome::Ancestor,
        );

        for debug in [format!("{target:?}"), format!("{check:?}")] {
            assert!(!debug.contains("165"));
            assert!(!debug.contains("A5"));
        }
    }
}
