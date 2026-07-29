use std::{
    error::Error,
    fmt,
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};

use repowitness_application::ResolvedConfiguration;
use repowitness_domain::ConfigurationDigest;

use crate::sqlite::{MAX_RETENTION_GENERATION_PINS, MAX_RETENTION_VIEW_PINS, SqliteStoreError};

/// Version of the aggregate-only local retention maintenance report.
pub const LOCAL_RETENTION_PROFILE_VERSION: u16 = 1;
/// Conservative default end-to-end retention maintenance timeout.
pub const DEFAULT_LOCAL_RETENTION_TIMEOUT: Duration = Duration::from_secs(30);
/// Absolute end-to-end retention maintenance timeout.
pub const MAX_LOCAL_RETENTION_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// Stable path-free validation failure for a local retention request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalRetentionRequestError {
    /// The timeout was zero or exceeded the compiled ceiling.
    InvalidTimeout,
    /// A generation pin was not a positive database-local integer.
    InvalidGenerationPin,
    /// A workspace-view pin was not a positive database-local integer.
    InvalidWorkspaceViewPin,
    /// The request supplied more pins than the compiled storage bound.
    TooManyPins,
    /// A pin was repeated within the same provenance category.
    DuplicatePin,
}

impl fmt::Display for LocalRetentionRequestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("local retention request is invalid")
    }
}

impl Error for LocalRetentionRequestError {}

/// Complete bounded snapshot of explicit, supervised, and immutable-view pins.
#[derive(Clone, Default, Eq, PartialEq)]
pub struct LocalRetentionPins {
    pub(super) explicit_generations: Box<[i64]>,
    pub(super) supervised_generations: Box<[i64]>,
    pub(super) workspace_views: Box<[i64]>,
}

impl LocalRetentionPins {
    /// Validates, sorts, and uniquely records one complete live-pin snapshot.
    pub fn try_new(
        mut explicit_generations: Vec<i64>,
        mut supervised_generations: Vec<i64>,
        mut workspace_views: Vec<i64>,
    ) -> Result<Self, LocalRetentionRequestError> {
        let generation_count = explicit_generations
            .len()
            .checked_add(supervised_generations.len())
            .ok_or(LocalRetentionRequestError::TooManyPins)?;
        if generation_count > MAX_RETENTION_GENERATION_PINS
            || workspace_views.len() > MAX_RETENTION_VIEW_PINS
        {
            return Err(LocalRetentionRequestError::TooManyPins);
        }
        canonicalize_positive(
            &mut explicit_generations,
            LocalRetentionRequestError::InvalidGenerationPin,
        )?;
        canonicalize_positive(
            &mut supervised_generations,
            LocalRetentionRequestError::InvalidGenerationPin,
        )?;
        canonicalize_positive(
            &mut workspace_views,
            LocalRetentionRequestError::InvalidWorkspaceViewPin,
        )?;
        Ok(Self {
            explicit_generations: explicit_generations.into_boxed_slice(),
            supervised_generations: supervised_generations.into_boxed_slice(),
            workspace_views: workspace_views.into_boxed_slice(),
        })
    }

    /// Returns the exact explicit and supervised generation-pin count.
    #[must_use]
    pub fn generation_pin_count(&self) -> usize {
        self.explicit_generations.len() + self.supervised_generations.len()
    }

    /// Returns the exact immutable workspace-view pin count.
    #[must_use]
    pub const fn workspace_view_pin_count(&self) -> usize {
        self.workspace_views.len()
    }
}

impl fmt::Debug for LocalRetentionPins {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalRetentionPins")
            .field("generation_pin_count", &self.generation_pin_count())
            .field("workspace_view_pin_count", &self.workspace_view_pin_count())
            .finish()
    }
}

fn canonicalize_positive(
    values: &mut [i64],
    invalid: LocalRetentionRequestError,
) -> Result<(), LocalRetentionRequestError> {
    if values.iter().any(|value| *value <= 0) {
        return Err(invalid);
    }
    values.sort_unstable();
    if values.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(LocalRetentionRequestError::DuplicatePin);
    }
    Ok(())
}

pub(super) struct LocalRetentionCommon<'a> {
    pub(super) database: PathBuf,
    pub(super) migration_applied_at_unix_ms: u64,
    pub(super) configuration: &'a ResolvedConfiguration,
    pub(super) pins: LocalRetentionPins,
    pub(super) cancelled: Arc<AtomicBool>,
    pub(super) timeout: Duration,
}

impl fmt::Debug for LocalRetentionCommon<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalRetentionCommon")
            .field("database", &"<redacted-path>")
            .field(
                "migration_applied_at_unix_ms",
                &self.migration_applied_at_unix_ms,
            )
            .field("configuration_digest", &self.configuration.digest())
            .field("pins", &self.pins)
            .field(
                "cancelled",
                &self.cancelled.load(std::sync::atomic::Ordering::Acquire),
            )
            .field("timeout", &self.timeout)
            .finish()
    }
}

/// Complete input for one deterministic read-only retention plan.
pub struct LocalRetentionPlanRequest<'a> {
    pub(super) common: LocalRetentionCommon<'a>,
}

impl<'a> LocalRetentionPlanRequest<'a> {
    /// Constructs a plan request after validating its end-to-end bound.
    pub fn try_new(
        database: &Path,
        migration_applied_at_unix_ms: u64,
        configuration: &'a ResolvedConfiguration,
        pins: LocalRetentionPins,
        cancelled: Arc<AtomicBool>,
        timeout: Duration,
    ) -> Result<Self, LocalRetentionRequestError> {
        Ok(Self {
            common: validated_common(
                database,
                migration_applied_at_unix_ms,
                configuration,
                pins,
                cancelled,
                timeout,
            )?,
        })
    }
}

impl fmt::Debug for LocalRetentionPlanRequest<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("LocalRetentionPlanRequest")
            .field(&self.common)
            .finish()
    }
}

/// Complete input for one stale-safe explicit retention apply.
pub struct LocalRetentionApplyRequest<'a> {
    pub(super) common: LocalRetentionCommon<'a>,
    pub(super) expected_plan_digest: [u8; 32],
}

impl<'a> LocalRetentionApplyRequest<'a> {
    /// Constructs an apply request bound to an exact prior plan digest.
    #[allow(
        clippy::too_many_arguments,
        reason = "the explicit stale-safe maintenance boundary keeps every security input visible"
    )]
    pub fn try_new(
        database: &Path,
        migration_applied_at_unix_ms: u64,
        configuration: &'a ResolvedConfiguration,
        pins: LocalRetentionPins,
        expected_plan_digest: [u8; 32],
        cancelled: Arc<AtomicBool>,
        timeout: Duration,
    ) -> Result<Self, LocalRetentionRequestError> {
        Ok(Self {
            common: validated_common(
                database,
                migration_applied_at_unix_ms,
                configuration,
                pins,
                cancelled,
                timeout,
            )?,
            expected_plan_digest,
        })
    }
}

impl fmt::Debug for LocalRetentionApplyRequest<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalRetentionApplyRequest")
            .field("common", &self.common)
            .field(
                "expected_plan_digest_bytes",
                &self.expected_plan_digest.len(),
            )
            .finish()
    }
}

fn validated_common<'a>(
    database: &Path,
    migration_applied_at_unix_ms: u64,
    configuration: &'a ResolvedConfiguration,
    pins: LocalRetentionPins,
    cancelled: Arc<AtomicBool>,
    timeout: Duration,
) -> Result<LocalRetentionCommon<'a>, LocalRetentionRequestError> {
    if timeout.is_zero() || timeout > MAX_LOCAL_RETENTION_TIMEOUT {
        return Err(LocalRetentionRequestError::InvalidTimeout);
    }
    Ok(LocalRetentionCommon {
        database: database.to_path_buf(),
        migration_applied_at_unix_ms,
        configuration,
        pins,
        cancelled,
        timeout,
    })
}

/// Complete path-free summary of the exact retention policy used by one pass.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalRetentionPolicySummary {
    pub(super) retained_generations_per_source_slot: u16,
    pub(super) max_generation_candidates: u64,
    pub(super) max_rows: u64,
    pub(super) max_bytes: u64,
    pub(super) generation_pin_count: u64,
    pub(super) workspace_view_pin_count: u64,
}

impl LocalRetentionPolicySummary {
    /// Returns the minimum newest generations retained for every source slot.
    #[must_use]
    pub const fn retained_generations_per_source_slot(self) -> u16 {
        self.retained_generations_per_source_slot
    }

    /// Returns the maximum candidates admitted by the transaction.
    #[must_use]
    pub const fn max_generation_candidates(self) -> u64 {
        self.max_generation_candidates
    }

    /// Returns the maximum shared logical row work.
    #[must_use]
    pub const fn max_rows(self) -> u64 {
        self.max_rows
    }

    /// Returns the maximum conservative byte estimate.
    #[must_use]
    pub const fn max_bytes(self) -> u64 {
        self.max_bytes
    }

    /// Returns the complete generation-pin count.
    #[must_use]
    pub const fn generation_pin_count(self) -> u64 {
        self.generation_pin_count
    }

    /// Returns the complete immutable-view pin count.
    #[must_use]
    pub const fn workspace_view_pin_count(self) -> u64 {
        self.workspace_view_pin_count
    }
}

/// Aggregate-only result of one deterministic retention plan.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalRetentionPlanReport {
    pub(super) configuration_digest: ConfigurationDigest,
    pub(super) policy_digest: [u8; 32],
    pub(super) plan_digest: [u8; 32],
    pub(super) policy: LocalRetentionPolicySummary,
    pub(super) candidate_count: u64,
    pub(super) estimated_rows: u64,
    pub(super) estimated_bytes: u64,
    pub(super) root_count: u64,
    pub(super) unresolved_count: u64,
    pub(super) unresolved_truncated: bool,
    pub(super) logical_work_rows: u64,
    pub(super) more_work: bool,
}

impl LocalRetentionPlanReport {
    /// Returns the semantic configuration identity.
    #[must_use]
    pub const fn configuration_digest(self) -> ConfigurationDigest {
        self.configuration_digest
    }

    /// Returns the exact retention-policy identity.
    #[must_use]
    pub const fn policy_digest(self) -> [u8; 32] {
        self.policy_digest
    }

    /// Returns the stale-safe digest required by apply.
    #[must_use]
    pub const fn plan_digest(self) -> [u8; 32] {
        self.plan_digest
    }

    /// Returns the complete effective policy summary.
    #[must_use]
    pub const fn policy(self) -> LocalRetentionPolicySummary {
        self.policy
    }

    /// Returns the exact candidate count without exposing candidate identities.
    #[must_use]
    pub const fn candidate_count(self) -> u64 {
        self.candidate_count
    }

    /// Returns the conservative row estimate.
    #[must_use]
    pub const fn estimated_rows(self) -> u64 {
        self.estimated_rows
    }

    /// Returns the conservative byte estimate.
    #[must_use]
    pub const fn estimated_bytes(self) -> u64 {
        self.estimated_bytes
    }

    /// Returns authoritative and explicitly pinned root rows examined.
    #[must_use]
    pub const fn root_count(self) -> u64 {
        self.root_count
    }

    /// Returns known eligible candidates not admitted by this batch.
    #[must_use]
    pub const fn unresolved_count(self) -> u64 {
        self.unresolved_count
    }

    /// Reports whether additional unresolved candidates may remain uncounted.
    #[must_use]
    pub const fn unresolved_truncated(self) -> bool {
        self.unresolved_truncated
    }

    /// Returns shared root/candidate/reserved-apply logical row work.
    #[must_use]
    pub const fn logical_work_rows(self) -> u64 {
        self.logical_work_rows
    }

    /// Reports whether another bounded pass may remain.
    #[must_use]
    pub const fn more_work(self) -> bool {
        self.more_work
    }
}

/// Aggregate result of one committed explicit retention apply.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalRetentionApplyReport {
    pub(super) configuration_digest: ConfigurationDigest,
    pub(super) policy_digest: [u8; 32],
    pub(super) plan_digest: [u8; 32],
    pub(super) policy: LocalRetentionPolicySummary,
    pub(super) collection_id: u64,
    pub(super) generation_count: u64,
    pub(super) workspace_view_count: u64,
    pub(super) source_slot_receipt_count: u64,
    pub(super) snapshot_count: u64,
    pub(super) artifact_count: u64,
    pub(super) deleted_rows: u64,
    pub(super) estimated_deleted_bytes: u64,
    pub(super) more_work: bool,
    pub(super) shutdown_complete: bool,
    pub(super) database_identity_confirmed: bool,
}

impl LocalRetentionApplyReport {
    /// Returns the semantic configuration identity.
    #[must_use]
    pub const fn configuration_digest(self) -> ConfigurationDigest {
        self.configuration_digest
    }

    /// Returns the exact retention-policy identity.
    #[must_use]
    pub const fn policy_digest(self) -> [u8; 32] {
        self.policy_digest
    }

    /// Returns the prior plan identity revalidated by this apply.
    #[must_use]
    pub const fn plan_digest(self) -> [u8; 32] {
        self.plan_digest
    }

    /// Returns the complete effective policy summary.
    #[must_use]
    pub const fn policy(self) -> LocalRetentionPolicySummary {
        self.policy
    }

    /// Returns the append-only local audit receipt identity.
    #[must_use]
    pub const fn collection_id(self) -> u64 {
        self.collection_id
    }

    /// Returns the exact deleted-generation count.
    #[must_use]
    pub const fn generation_count(self) -> u64 {
        self.generation_count
    }

    /// Returns the exact deleted historical-view count.
    #[must_use]
    pub const fn workspace_view_count(self) -> u64 {
        self.workspace_view_count
    }

    /// Returns the exact deleted superseded-receipt count.
    #[must_use]
    pub const fn source_slot_receipt_count(self) -> u64 {
        self.source_slot_receipt_count
    }

    /// Returns the exact deleted source-snapshot count.
    #[must_use]
    pub const fn snapshot_count(self) -> u64 {
        self.snapshot_count
    }

    /// Returns the exact deleted analysis-artifact count.
    #[must_use]
    pub const fn artifact_count(self) -> u64 {
        self.artifact_count
    }

    /// Returns the exact ordinary and derived row count deleted.
    #[must_use]
    pub const fn deleted_rows(self) -> u64 {
        self.deleted_rows
    }

    /// Returns the plan's conservative deleted-byte estimate.
    #[must_use]
    pub const fn estimated_deleted_bytes(self) -> u64 {
        self.estimated_deleted_bytes
    }

    /// Reports whether another bounded pass may remain.
    #[must_use]
    pub const fn more_work(self) -> bool {
        self.more_work
    }

    /// Reports whether the writer owner acknowledged bounded shutdown.
    #[must_use]
    pub const fn shutdown_complete(self) -> bool {
        self.shutdown_complete
    }

    /// Reports whether the final pathname still named the exact writer-opened file.
    #[must_use]
    pub const fn database_identity_confirmed(self) -> bool {
        self.database_identity_confirmed
    }

    /// Returns the exact number of bounded post-commit maintenance warnings.
    #[must_use]
    pub const fn warning_count(self) -> u8 {
        (!self.shutdown_complete) as u8 + (!self.database_identity_confirmed) as u8
    }
}

/// Stable path-free local retention failure category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalRetentionErrorKind {
    /// The database did not exist as one admissible regular file.
    DatabaseUnavailable,
    /// The resolved policy could not be represented by the storage contract.
    InvalidPolicy,
    /// At least one eligible object exceeded the caller's bounded work budget.
    BlockedByLimit,
    /// The operation was explicitly cancelled.
    Cancelled,
    /// The end-to-end deadline elapsed.
    DeadlineExceeded,
    /// The supplied prior plan no longer matched current roots and candidates.
    PlanStale,
    /// Apply may have committed, but its exact audit receipt could not be recovered.
    OutcomeUnknown,
    /// The maintenance owner or database rejected the operation.
    MaintenanceUnavailable,
}

/// Content- and path-redacted failure from local retention maintenance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalRetentionError {
    kind: LocalRetentionErrorKind,
    source: Option<SqliteStoreError>,
}

impl LocalRetentionError {
    pub(super) const fn new(
        kind: LocalRetentionErrorKind,
        source: Option<SqliteStoreError>,
    ) -> Self {
        Self { kind, source }
    }

    /// Returns the stable path-free failure category.
    #[must_use]
    pub const fn kind(self) -> LocalRetentionErrorKind {
        self.kind
    }
}

impl fmt::Display for LocalRetentionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("local retention maintenance failed")
    }
}

impl Error for LocalRetentionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source
            .as_ref()
            .map(|source| source as &(dyn Error + 'static))
    }
}
