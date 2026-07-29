use std::{
    fmt,
    sync::{Arc, atomic::AtomicBool},
    time::Instant,
};

use sha2::{Digest, Sha256};

use super::{GenerationId, SqliteStoreError, WorkspaceViewId};

const RETENTION_POLICY_DIGEST_DOMAIN: &[u8] = b"RepoWitness\0phase1-generation-retention-policy\0";
/// Canonical policy encoding version used by retention plan/apply.
pub const RETENTION_POLICY_VERSION: u16 = 1;
/// Initial minimum number of newest retained generations kept per source slot.
pub const DEFAULT_RETAINED_GENERATIONS_PER_SOURCE_SLOT: u16 = 2;
/// Largest caller-selectable retained-generation floor.
pub const MAX_RETAINED_GENERATIONS_PER_SOURCE_SLOT: u16 = 4_096;
/// Largest combined explicit and supervised generation-pin set.
pub const MAX_RETENTION_GENERATION_PINS: usize = 4_096;
/// Largest pinned immutable-view set admitted by one collection pass.
pub const MAX_RETENTION_VIEW_PINS: usize = 4_096;
/// Largest number of generation candidates admitted by one transaction.
pub const MAX_RETENTION_GENERATION_CANDIDATES: u64 = 4_096;
/// Largest estimated row budget admitted by one transaction.
pub const MAX_RETENTION_ROWS: u64 = 100_000_000;
/// Largest estimated byte budget admitted by one transaction.
pub const MAX_RETENTION_BYTES: u64 = 16 * 1024 * 1024 * 1024;

const DEFAULT_RETENTION_GENERATION_CANDIDATES: u64 = 64;
const DEFAULT_RETENTION_ROWS: u64 = 1_000_000;
const DEFAULT_RETENTION_BYTES: u64 = 512 * 1024 * 1024;

macro_rules! define_retention_digest {
    ($name:ident, $documentation:literal) => {
        #[doc = $documentation]
        #[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name([u8; 32]);

        impl $name {
            pub(crate) const fn new(bytes: [u8; 32]) -> Self {
                Self(bytes)
            }

            /// Returns the canonical digest bytes.
            #[must_use]
            pub const fn as_bytes(&self) -> &[u8; 32] {
                &self.0
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter
                    .debug_struct(stringify!($name))
                    .field("encoded_bytes", &self.0.len())
                    .finish_non_exhaustive()
            }
        }
    };
}

define_retention_digest!(
    RetentionPolicyDigest,
    "Canonical identity of one complete bounded generation-retention policy."
);
define_retention_digest!(
    RetentionPlanDigest,
    "Canonical identity of one policy, root snapshot, and candidate batch."
);

/// Hard work budgets for one deterministic collection transaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RetentionLimits {
    max_generation_candidates: u64,
    max_rows: u64,
    max_estimated_bytes: u64,
}

impl RetentionLimits {
    /// Validates nonzero limits against compiled safety ceilings.
    pub const fn try_new(
        max_generation_candidates: u64,
        max_rows: u64,
        max_estimated_bytes: u64,
    ) -> Result<Self, SqliteStoreError> {
        if max_generation_candidates == 0
            || max_generation_candidates > MAX_RETENTION_GENERATION_CANDIDATES
            || max_rows == 0
            || max_rows > MAX_RETENTION_ROWS
            || max_estimated_bytes == 0
            || max_estimated_bytes > MAX_RETENTION_BYTES
        {
            return Err(SqliteStoreError::InvalidRetentionPolicy);
        }
        Ok(Self {
            max_generation_candidates,
            max_rows,
            max_estimated_bytes,
        })
    }

    /// Returns the inclusive candidate limit.
    #[must_use]
    pub const fn max_generation_candidates(self) -> u64 {
        self.max_generation_candidates
    }

    /// Returns the inclusive shared logical row-work limit.
    #[must_use]
    pub const fn max_rows(self) -> u64 {
        self.max_rows
    }

    /// Returns the inclusive estimated-byte limit.
    #[must_use]
    pub const fn max_estimated_bytes(self) -> u64 {
        self.max_estimated_bytes
    }
}

impl Default for RetentionLimits {
    fn default() -> Self {
        Self {
            max_generation_candidates: DEFAULT_RETENTION_GENERATION_CANDIDATES,
            max_rows: DEFAULT_RETENTION_ROWS,
            max_estimated_bytes: DEFAULT_RETENTION_BYTES,
        }
    }
}

/// Bounded application-owned reader, task, and immutable-view pins.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RetentionPins {
    explicit_generations: Box<[GenerationId]>,
    supervised_generations: Box<[GenerationId]>,
    workspace_views: Box<[WorkspaceViewId]>,
}

impl RetentionPins {
    /// Validates, sorts, and deduplicates one complete live-pin snapshot.
    pub fn try_new(
        mut explicit_generations: Vec<GenerationId>,
        mut supervised_generations: Vec<GenerationId>,
        mut workspace_views: Vec<WorkspaceViewId>,
    ) -> Result<Self, SqliteStoreError> {
        canonical_generation_ids(&mut explicit_generations)?;
        canonical_generation_ids(&mut supervised_generations)?;
        canonical_workspace_view_ids(&mut workspace_views)?;
        let generation_pins = explicit_generations
            .len()
            .checked_add(supervised_generations.len())
            .ok_or(SqliteStoreError::InvalidRetentionPolicy)?;
        if generation_pins > MAX_RETENTION_GENERATION_PINS
            || workspace_views.len() > MAX_RETENTION_VIEW_PINS
        {
            return Err(SqliteStoreError::InvalidRetentionPolicy);
        }
        Ok(Self {
            explicit_generations: explicit_generations.into_boxed_slice(),
            supervised_generations: supervised_generations.into_boxed_slice(),
            workspace_views: workspace_views.into_boxed_slice(),
        })
    }

    pub(super) fn explicit_generations(&self) -> &[GenerationId] {
        &self.explicit_generations
    }

    pub(super) fn supervised_generations(&self) -> &[GenerationId] {
        &self.supervised_generations
    }

    pub(super) fn workspace_views(&self) -> &[WorkspaceViewId] {
        &self.workspace_views
    }

    /// Returns the exact number of generation pins.
    #[must_use]
    pub fn generation_pin_count(&self) -> usize {
        self.explicit_generations.len() + self.supervised_generations.len()
    }

    /// Returns the exact number of immutable workspace-view pins.
    #[must_use]
    pub const fn workspace_view_pin_count(&self) -> usize {
        self.workspace_views.len()
    }
}

/// Complete path-free policy used by both plan and apply.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GenerationRetentionPolicy {
    retained_generations_per_source_slot: u16,
    limits: RetentionLimits,
    pins: RetentionPins,
    digest: RetentionPolicyDigest,
}

impl GenerationRetentionPolicy {
    /// Constructs one bounded policy and its canonical identity.
    pub fn try_new(
        retained_generations_per_source_slot: u16,
        limits: RetentionLimits,
        pins: RetentionPins,
    ) -> Result<Self, SqliteStoreError> {
        if retained_generations_per_source_slot == 0
            || retained_generations_per_source_slot > MAX_RETAINED_GENERATIONS_PER_SOURCE_SLOT
        {
            return Err(SqliteStoreError::InvalidRetentionPolicy);
        }
        let digest = retention_policy_digest(retained_generations_per_source_slot, limits, &pins);
        Ok(Self {
            retained_generations_per_source_slot,
            limits,
            pins,
            digest,
        })
    }

    /// Returns the minimum newest retained generations kept for every slot.
    #[must_use]
    pub const fn retained_generations_per_source_slot(&self) -> u16 {
        self.retained_generations_per_source_slot
    }

    /// Returns the complete hard work limits.
    #[must_use]
    pub const fn limits(&self) -> RetentionLimits {
        self.limits
    }

    /// Returns the complete bounded live-pin snapshot.
    #[must_use]
    pub const fn pins(&self) -> &RetentionPins {
        &self.pins
    }

    /// Returns the canonical policy identity.
    #[must_use]
    pub const fn digest(&self) -> RetentionPolicyDigest {
        self.digest
    }
}

impl Default for GenerationRetentionPolicy {
    fn default() -> Self {
        Self::try_new(
            DEFAULT_RETAINED_GENERATIONS_PER_SOURCE_SLOT,
            RetentionLimits::default(),
            RetentionPins::default(),
        )
        .expect("compiled retention defaults must remain valid")
    }
}

/// Complete owned input for one read-only retention plan.
pub struct RetentionPlanRequest {
    policy: GenerationRetentionPolicy,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
}

impl RetentionPlanRequest {
    /// Constructs one read-only plan request.
    #[must_use]
    pub const fn new(
        policy: GenerationRetentionPolicy,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Self {
        Self {
            policy,
            cancelled,
            deadline,
        }
    }

    pub(super) fn into_parts(self) -> (GenerationRetentionPolicy, Arc<AtomicBool>, Instant) {
        (self.policy, self.cancelled, self.deadline)
    }
}

impl fmt::Debug for RetentionPlanRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RetentionPlanRequest")
            .field("policy_digest", &self.policy.digest())
            .field(
                "generation_pin_count",
                &self.policy.pins().generation_pin_count(),
            )
            .field(
                "workspace_view_pin_count",
                &self.policy.pins().workspace_view_pin_count(),
            )
            .field(
                "cancelled",
                &self.cancelled.load(std::sync::atomic::Ordering::Acquire),
            )
            .field("deadline", &"<monotonic>")
            .finish()
    }
}

/// Complete owned input for one stale-safe retention apply.
pub struct RetentionApplyRequest {
    policy: GenerationRetentionPolicy,
    expected_plan: RetentionPlanDigest,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
}

impl RetentionApplyRequest {
    /// Constructs one explicit apply request bound to a prior plan digest.
    #[must_use]
    pub const fn new(
        policy: GenerationRetentionPolicy,
        expected_plan: RetentionPlanDigest,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Self {
        Self {
            policy,
            expected_plan,
            cancelled,
            deadline,
        }
    }

    pub(super) fn into_parts(
        self,
    ) -> (
        GenerationRetentionPolicy,
        RetentionPlanDigest,
        Arc<AtomicBool>,
        Instant,
    ) {
        (
            self.policy,
            self.expected_plan,
            self.cancelled,
            self.deadline,
        )
    }
}

impl fmt::Debug for RetentionApplyRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RetentionApplyRequest")
            .field("policy_digest", &self.policy.digest())
            .field("expected_plan", &self.expected_plan)
            .field(
                "generation_pin_count",
                &self.policy.pins().generation_pin_count(),
            )
            .field(
                "workspace_view_pin_count",
                &self.policy.pins().workspace_view_pin_count(),
            )
            .field(
                "cancelled",
                &self.cancelled.load(std::sync::atomic::Ordering::Acquire),
            )
            .field("deadline", &"<monotonic>")
            .finish()
    }
}

/// One bounded, deterministic, read-only garbage-collection plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RetentionPlan {
    policy_digest: RetentionPolicyDigest,
    plan_digest: RetentionPlanDigest,
    candidate_generations: Box<[GenerationId]>,
    estimated_rows: u64,
    estimated_bytes: u64,
    root_count: u64,
    unresolved_count: u64,
    unresolved_truncated: bool,
    logical_work_rows: u64,
    more_work: bool,
}

impl RetentionPlan {
    #[allow(
        clippy::too_many_arguments,
        reason = "the immutable bounded plan mirrors its fixed aggregate receipt and digest fields"
    )]
    pub(super) fn new(
        policy_digest: RetentionPolicyDigest,
        plan_digest: RetentionPlanDigest,
        candidate_generations: Vec<GenerationId>,
        estimated_rows: u64,
        estimated_bytes: u64,
        root_count: u64,
        unresolved_count: u64,
        unresolved_truncated: bool,
        logical_work_rows: u64,
        more_work: bool,
    ) -> Self {
        Self {
            policy_digest,
            plan_digest,
            candidate_generations: candidate_generations.into_boxed_slice(),
            estimated_rows,
            estimated_bytes,
            root_count,
            unresolved_count,
            unresolved_truncated,
            logical_work_rows,
            more_work,
        }
    }

    /// Returns the policy identity used to derive the plan.
    #[must_use]
    pub const fn policy_digest(&self) -> RetentionPolicyDigest {
        self.policy_digest
    }

    /// Returns the stale-safe identity required by apply.
    #[must_use]
    pub const fn plan_digest(&self) -> RetentionPlanDigest {
        self.plan_digest
    }

    /// Returns the canonically ordered database-local generation candidates.
    #[must_use]
    pub fn candidate_generations(&self) -> &[GenerationId] {
        &self.candidate_generations
    }

    /// Returns the conservative estimated rows affected by this batch.
    #[must_use]
    pub const fn estimated_rows(&self) -> u64 {
        self.estimated_rows
    }

    /// Returns the conservative estimated bytes affected by this batch.
    #[must_use]
    pub const fn estimated_bytes(&self) -> u64 {
        self.estimated_bytes
    }

    /// Returns authoritative and explicitly pinned root rows examined.
    #[must_use]
    pub const fn root_count(&self) -> u64 {
        self.root_count
    }

    /// Returns known eligible candidates not admitted by this batch.
    #[must_use]
    pub const fn unresolved_count(&self) -> u64 {
        self.unresolved_count
    }

    /// Reports whether additional unresolved candidates may remain uncounted.
    #[must_use]
    pub const fn unresolved_truncated(&self) -> bool {
        self.unresolved_truncated
    }

    /// Returns shared root/candidate/reserved-apply logical row work.
    #[must_use]
    pub const fn logical_work_rows(&self) -> u64 {
        self.logical_work_rows
    }

    /// Reports whether another bounded pass may remain.
    #[must_use]
    pub const fn more_work(&self) -> bool {
        self.more_work
    }
}

/// Aggregate result of one committed retention transaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RetentionApplyOutcome {
    collection_id: u64,
    generation_count: u64,
    workspace_view_count: u64,
    source_slot_receipt_count: u64,
    snapshot_count: u64,
    artifact_count: u64,
    deleted_rows: u64,
    estimated_deleted_bytes: u64,
    more_work: bool,
}

impl RetentionApplyOutcome {
    #[allow(
        clippy::too_many_arguments,
        reason = "the immutable aggregate audit receipt mirrors fixed schema columns"
    )]
    pub(super) const fn new(
        collection_id: u64,
        generation_count: u64,
        workspace_view_count: u64,
        source_slot_receipt_count: u64,
        snapshot_count: u64,
        artifact_count: u64,
        deleted_rows: u64,
        estimated_deleted_bytes: u64,
        more_work: bool,
    ) -> Self {
        Self {
            collection_id,
            generation_count,
            workspace_view_count,
            source_slot_receipt_count,
            snapshot_count,
            artifact_count,
            deleted_rows,
            estimated_deleted_bytes,
            more_work,
        }
    }

    /// Returns the append-only local audit identity.
    #[must_use]
    pub const fn collection_id(self) -> u64 {
        self.collection_id
    }

    /// Returns deleted generation count.
    #[must_use]
    pub const fn generation_count(self) -> u64 {
        self.generation_count
    }

    /// Returns deleted historical workspace-view count.
    #[must_use]
    pub const fn workspace_view_count(self) -> u64 {
        self.workspace_view_count
    }

    /// Returns deleted superseded completion-receipt count.
    #[must_use]
    pub const fn source_slot_receipt_count(self) -> u64 {
        self.source_slot_receipt_count
    }

    /// Returns deleted now-unreferenced source-snapshot count.
    #[must_use]
    pub const fn snapshot_count(self) -> u64 {
        self.snapshot_count
    }

    /// Returns deleted now-unreferenced analysis-artifact count.
    #[must_use]
    pub const fn artifact_count(self) -> u64 {
        self.artifact_count
    }

    /// Returns exact rows deleted from ordinary and derived relations.
    #[must_use]
    pub const fn deleted_rows(self) -> u64 {
        self.deleted_rows
    }

    /// Returns the plan's conservative estimated deleted bytes.
    #[must_use]
    pub const fn estimated_deleted_bytes(self) -> u64 {
        self.estimated_deleted_bytes
    }

    /// Reports whether another bounded pass may remain.
    #[must_use]
    pub const fn more_work(self) -> bool {
        self.more_work
    }
}

fn canonical_generation_ids(values: &mut Vec<GenerationId>) -> Result<(), SqliteStoreError> {
    if values.iter().any(|value| value.get() <= 0) {
        return Err(SqliteStoreError::InvalidRetentionPolicy);
    }
    values.sort_unstable();
    values.dedup();
    Ok(())
}

fn canonical_workspace_view_ids(values: &mut Vec<WorkspaceViewId>) -> Result<(), SqliteStoreError> {
    if values.iter().any(|value| value.get() <= 0) {
        return Err(SqliteStoreError::InvalidRetentionPolicy);
    }
    values.sort_unstable();
    values.dedup();
    Ok(())
}

fn retention_policy_digest(
    retained_generations_per_source_slot: u16,
    limits: RetentionLimits,
    pins: &RetentionPins,
) -> RetentionPolicyDigest {
    let mut hasher = Sha256::new();
    hasher.update(RETENTION_POLICY_DIGEST_DOMAIN);
    hasher.update(RETENTION_POLICY_VERSION.to_be_bytes());
    hasher.update(retained_generations_per_source_slot.to_be_bytes());
    hasher.update(limits.max_generation_candidates().to_be_bytes());
    hasher.update(limits.max_rows().to_be_bytes());
    hasher.update(limits.max_estimated_bytes().to_be_bytes());
    update_generation_ids(&mut hasher, pins.explicit_generations());
    update_generation_ids(&mut hasher, pins.supervised_generations());
    hasher.update(
        u64::try_from(pins.workspace_views().len())
            .unwrap_or(u64::MAX)
            .to_be_bytes(),
    );
    for view in pins.workspace_views() {
        hasher.update(view.get().to_be_bytes());
    }
    RetentionPolicyDigest::new(hasher.finalize().into())
}

fn update_generation_ids(hasher: &mut Sha256, values: &[GenerationId]) {
    hasher.update(
        u64::try_from(values.len())
            .unwrap_or(u64::MAX)
            .to_be_bytes(),
    );
    for generation in values {
        hasher.update(generation.get().to_be_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retention_limits_and_floors_reject_zero_and_one_over_the_ceiling() {
        assert_eq!(
            RetentionLimits::try_new(0, 1, 1),
            Err(SqliteStoreError::InvalidRetentionPolicy)
        );
        assert_eq!(
            RetentionLimits::try_new(
                MAX_RETENTION_GENERATION_CANDIDATES + 1,
                MAX_RETENTION_ROWS,
                MAX_RETENTION_BYTES,
            ),
            Err(SqliteStoreError::InvalidRetentionPolicy)
        );
        assert_eq!(
            GenerationRetentionPolicy::try_new(
                0,
                RetentionLimits::default(),
                RetentionPins::default(),
            ),
            Err(SqliteStoreError::InvalidRetentionPolicy)
        );
    }

    #[test]
    fn retention_pins_are_canonical_and_policy_digests_are_order_independent() {
        let first = GenerationId::from_database(1);
        let second = GenerationId::from_database(2);
        let view = WorkspaceViewId::from_database(3);
        let left = RetentionPins::try_new(vec![second, first, first], vec![], vec![view, view])
            .expect("bounded pins should validate");
        let right = RetentionPins::try_new(vec![first, second], vec![], vec![view])
            .expect("canonical pins should validate");
        let left = GenerationRetentionPolicy::try_new(2, RetentionLimits::default(), left)
            .expect("left policy should validate");
        let right = GenerationRetentionPolicy::try_new(2, RetentionLimits::default(), right)
            .expect("right policy should validate");

        assert_eq!(left, right);
        assert_eq!(left.digest(), right.digest());
        assert!(!format!("{:?}", left.digest()).contains("000000"));
    }

    #[test]
    fn invalid_database_local_pin_id_fails_before_queueing() {
        assert_eq!(
            RetentionPins::try_new(vec![GenerationId::from_database(0)], Vec::new(), Vec::new(),),
            Err(SqliteStoreError::InvalidRetentionPolicy)
        );
    }
}
