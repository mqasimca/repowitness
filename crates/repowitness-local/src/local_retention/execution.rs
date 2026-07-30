use std::{
    fs,
    path::Path,
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};

use crate::contained_source::{FileIdentity, file_has_single_link};
use crate::sqlite::{
    GenerationId, GenerationRetentionPolicy, OwnedSqliteIndex, RetentionApplyRequest,
    RetentionLimits, RetentionPins, RetentionPlanDigest, SqliteMutationLease, SqliteStoreError,
    WorkspaceViewId, database_file_identity, load_retention_apply_outcome_read_only,
    plan_generation_retention_read_only,
};

use super::model::{
    LocalRetentionApplyReport, LocalRetentionApplyRequest, LocalRetentionCommon,
    LocalRetentionError, LocalRetentionErrorKind, LocalRetentionPins, LocalRetentionPlanReport,
    LocalRetentionPlanRequest, LocalRetentionPolicySummary,
};

const APPLY_OUTCOME_RECOVERY_TIMEOUT: Duration = Duration::from_secs(2);

/// Computes one deterministic aggregate-only retention plan.
pub fn plan_local_retention(
    request: LocalRetentionPlanRequest<'_>,
) -> Result<LocalRetentionPlanReport, LocalRetentionError> {
    let LocalRetentionCommon {
        database,
        migration_applied_at_unix_ms: _,
        configuration,
        pins,
        cancelled,
        timeout,
    } = request.common;
    let deadline = Instant::now()
        .checked_add(timeout)
        .ok_or_else(deadline_error)?;
    check_control(cancelled.as_ref(), deadline)?;
    let (policy, summary) = policy_from_configuration(configuration, pins)?;
    let configuration_digest = configuration.digest();
    let policy_digest = *policy.digest().as_bytes();
    let opened_identity = existing_database_identity(&database)?;
    let plan =
        plan_generation_retention_read_only(&database, &policy, Arc::clone(&cancelled), deadline)
            .map_err(map_store_error)?;
    if !opened_identity_is_current(&database, &opened_identity) {
        return Err(identity_changed_error());
    }
    let candidate_count = u64::try_from(plan.candidate_generations().len())
        .map_err(|_| map_store_error(SqliteStoreError::CountNotRepresentable))?;
    Ok(LocalRetentionPlanReport {
        configuration_digest,
        policy_digest,
        plan_digest: *plan.plan_digest().as_bytes(),
        policy: summary,
        candidate_count,
        estimated_rows: plan.estimated_rows(),
        estimated_bytes: plan.estimated_bytes(),
        root_count: plan.root_count(),
        unresolved_count: plan.unresolved_count(),
        unresolved_truncated: plan.unresolved_truncated(),
        logical_work_rows: plan.logical_work_rows(),
        more_work: plan.more_work(),
    })
}

/// Revalidates and atomically applies one exact prior retention plan.
///
/// On [`LocalRetentionErrorKind::OutcomeUnknown`], obtain the
/// [`LocalRetentionError::reconciliation_guidance`] and complete its read-only
/// exact-receipt lookup before any fresh-plan comparison or apply retry.
pub fn apply_local_retention(
    request: LocalRetentionApplyRequest<'_>,
) -> Result<LocalRetentionApplyReport, LocalRetentionError> {
    apply_local_retention_with_hooks(request, || {}, |deadline| deadline)
}

pub(super) fn apply_local_retention_with_hooks(
    request: LocalRetentionApplyRequest<'_>,
    after_commit: impl FnOnce(),
    shutdown_deadline: impl FnOnce(Instant) -> Instant,
) -> Result<LocalRetentionApplyReport, LocalRetentionError> {
    let LocalRetentionApplyRequest {
        common:
            LocalRetentionCommon {
                database,
                migration_applied_at_unix_ms,
                configuration,
                pins,
                cancelled,
                timeout,
            },
        expected_plan_digest,
    } = request;
    let deadline = Instant::now()
        .checked_add(timeout)
        .ok_or_else(deadline_error)?;
    check_control(cancelled.as_ref(), deadline)?;
    let (policy, summary) = policy_from_configuration(configuration, pins)?;
    let configuration_digest = configuration.digest();
    let policy_digest = *policy.digest().as_bytes();
    let storage_policy_digest = policy.digest();
    let storage_plan_digest = RetentionPlanDigest::new(expected_plan_digest);
    let (store, opened_identity) = open_existing_store(
        &database,
        migration_applied_at_unix_ms,
        &cancelled,
        deadline,
    )?;
    if !opened_identity_is_current(&database, &opened_identity) {
        return Err(identity_changed_error());
    }
    let outcome = store.apply_generation_retention(RetentionApplyRequest::new(
        policy,
        storage_plan_digest,
        Arc::clone(&cancelled),
        deadline,
    ));
    let outcome = match outcome {
        Ok(outcome) => outcome,
        Err(error) => {
            let recovery_deadline = Instant::now()
                .checked_add(APPLY_OUTCOME_RECOVERY_TIMEOUT)
                .ok_or_else(deadline_error)?;
            let shutdown_complete = store.shutdown(recovery_deadline).is_ok();
            let database_identity_confirmed =
                opened_identity_is_current(&database, &opened_identity);
            if database_identity_confirmed
                && let Ok(Some(outcome)) = load_retention_apply_outcome_read_only(
                    &database,
                    storage_policy_digest,
                    storage_plan_digest,
                    Arc::new(std::sync::atomic::AtomicBool::new(false)),
                    recovery_deadline,
                )
            {
                return Ok(apply_report(
                    configuration_digest,
                    policy_digest,
                    expected_plan_digest,
                    summary,
                    outcome,
                    shutdown_complete,
                    true,
                ));
            }
            return Err(map_apply_store_error(error));
        }
    };
    after_commit();
    let shutdown_complete = store.shutdown(shutdown_deadline(deadline)).is_ok();
    let database_identity_confirmed = opened_identity_is_current(&database, &opened_identity);
    Ok(apply_report(
        configuration_digest,
        policy_digest,
        expected_plan_digest,
        summary,
        outcome,
        shutdown_complete,
        database_identity_confirmed,
    ))
}

#[allow(
    clippy::too_many_arguments,
    reason = "the aggregate committed receipt keeps every stable identity and warning explicit"
)]
fn apply_report(
    configuration_digest: repowitness_domain::ConfigurationDigest,
    policy_digest: [u8; 32],
    plan_digest: [u8; 32],
    policy: LocalRetentionPolicySummary,
    outcome: crate::sqlite::RetentionApplyOutcome,
    shutdown_complete: bool,
    database_identity_confirmed: bool,
) -> LocalRetentionApplyReport {
    LocalRetentionApplyReport {
        configuration_digest,
        policy_digest,
        plan_digest,
        policy,
        collection_id: outcome.collection_id(),
        generation_count: outcome.generation_count(),
        workspace_view_count: outcome.workspace_view_count(),
        source_slot_receipt_count: outcome.source_slot_receipt_count(),
        snapshot_count: outcome.snapshot_count(),
        artifact_count: outcome.artifact_count(),
        deleted_rows: outcome.deleted_rows(),
        estimated_deleted_bytes: outcome.estimated_deleted_bytes(),
        more_work: outcome.more_work(),
        shutdown_complete,
        database_identity_confirmed,
    }
}

fn open_existing_store(
    database: &Path,
    migration_applied_at_unix_ms: u64,
    cancelled: &Arc<std::sync::atomic::AtomicBool>,
    deadline: Instant,
) -> Result<(OwnedSqliteIndex, FileIdentity), LocalRetentionError> {
    check_control(cancelled.as_ref(), deadline)?;
    validate_existing_database(database)?;
    let opened_identity = database_file_identity(database)
        .map_err(map_store_error)?
        .ok_or_else(database_unavailable)?;
    let mutation_lease =
        SqliteMutationLease::acquire_with_cancel(database, Some(cancelled.as_ref()), deadline)
            .map_err(map_store_error)?;
    let expected_identity = database_file_identity(database)
        .map_err(map_store_error)?
        .ok_or_else(database_unavailable)?;
    if opened_identity != expected_identity {
        return Err(identity_changed_error());
    }
    let (store, _) = OwnedSqliteIndex::start_with_lease(
        mutation_lease,
        Some(expected_identity),
        migration_applied_at_unix_ms,
        Arc::clone(cancelled),
        deadline,
    )
    .map_err(map_store_error)?;
    Ok((store, opened_identity))
}

fn existing_database_identity(path: &Path) -> Result<FileIdentity, LocalRetentionError> {
    validate_existing_database(path)?;
    database_file_identity(path)
        .map_err(map_store_error)?
        .ok_or_else(database_unavailable)
}

fn opened_identity_is_current(path: &Path, opened: &FileIdentity) -> bool {
    validate_existing_database(path).is_ok()
        && database_file_identity(path)
            .ok()
            .flatten()
            .is_some_and(|current| &current == opened)
}

fn policy_from_configuration(
    configuration: &repowitness_application::ResolvedConfiguration,
    pins: LocalRetentionPins,
) -> Result<(GenerationRetentionPolicy, LocalRetentionPolicySummary), LocalRetentionError> {
    let retention = configuration.policy().retention();
    let retained_generations_per_source_slot =
        u16::try_from(*retention.retained_generations_per_source_slot().effective())
            .map_err(|_| invalid_policy())?;
    let max_generation_candidates = *retention.max_generation_candidates().effective();
    let max_rows = *retention.max_rows().effective();
    let max_bytes = *retention.max_bytes().effective();
    let generation_pin_count =
        u64::try_from(pins.generation_pin_count()).map_err(|_| invalid_policy())?;
    let workspace_view_pin_count =
        u64::try_from(pins.workspace_view_pin_count()).map_err(|_| invalid_policy())?;
    let storage_pins = RetentionPins::try_new(
        pins.explicit_generations
            .iter()
            .copied()
            .map(GenerationId::from_database)
            .collect(),
        pins.supervised_generations
            .iter()
            .copied()
            .map(GenerationId::from_database)
            .collect(),
        pins.workspace_views
            .iter()
            .copied()
            .map(WorkspaceViewId::from_database)
            .collect(),
    )
    .map_err(map_store_error)?;
    let limits = RetentionLimits::try_new(max_generation_candidates, max_rows, max_bytes)
        .map_err(map_store_error)?;
    let policy = GenerationRetentionPolicy::try_new(
        retained_generations_per_source_slot,
        limits,
        storage_pins,
    )
    .map_err(map_store_error)?;
    Ok((
        policy,
        LocalRetentionPolicySummary {
            retained_generations_per_source_slot,
            max_generation_candidates,
            max_rows,
            max_bytes,
            generation_pin_count,
            workspace_view_pin_count,
        },
    ))
}

fn validate_existing_database(path: &Path) -> Result<(), LocalRetentionError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| database_unavailable())?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(database_unavailable());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;

        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(database_unavailable());
        }
    }
    let file = fs::File::open(path).map_err(|_| database_unavailable())?;
    if !file_has_single_link(&file).map_err(|_| database_unavailable())? {
        return Err(database_unavailable());
    }
    Ok(())
}

fn check_control(
    cancelled: &std::sync::atomic::AtomicBool,
    deadline: Instant,
) -> Result<(), LocalRetentionError> {
    if cancelled.load(Ordering::Acquire) {
        Err(LocalRetentionError::new(
            LocalRetentionErrorKind::Cancelled,
            Some(SqliteStoreError::Cancelled),
        ))
    } else if Instant::now() >= deadline {
        Err(deadline_error())
    } else {
        Ok(())
    }
}

fn database_unavailable() -> LocalRetentionError {
    LocalRetentionError::new(LocalRetentionErrorKind::DatabaseUnavailable, None)
}

fn invalid_policy() -> LocalRetentionError {
    LocalRetentionError::new(LocalRetentionErrorKind::InvalidPolicy, None)
}

fn deadline_error() -> LocalRetentionError {
    LocalRetentionError::new(
        LocalRetentionErrorKind::DeadlineExceeded,
        Some(SqliteStoreError::DeadlineExceeded),
    )
}

fn identity_changed_error() -> LocalRetentionError {
    LocalRetentionError::new(
        LocalRetentionErrorKind::MaintenanceUnavailable,
        Some(SqliteStoreError::DatabaseIdentityChanged),
    )
}

pub(super) fn map_store_error(source: SqliteStoreError) -> LocalRetentionError {
    let kind = match source {
        SqliteStoreError::InvalidRetentionPolicy | SqliteStoreError::RetentionPinUnavailable => {
            LocalRetentionErrorKind::InvalidPolicy
        }
        SqliteStoreError::RetentionLimitExceeded => LocalRetentionErrorKind::BlockedByLimit,
        SqliteStoreError::Cancelled => LocalRetentionErrorKind::Cancelled,
        SqliteStoreError::DeadlineExceeded | SqliteStoreError::ReplyTimeout => {
            LocalRetentionErrorKind::DeadlineExceeded
        }
        SqliteStoreError::RetentionPlanStale => LocalRetentionErrorKind::PlanStale,
        _ => LocalRetentionErrorKind::MaintenanceUnavailable,
    };
    LocalRetentionError::new(kind, Some(source))
}

pub(super) fn map_apply_store_error(source: SqliteStoreError) -> LocalRetentionError {
    if matches!(
        source,
        SqliteStoreError::MutationOutcomeUnknown
            | SqliteStoreError::WorkerUnavailable
            | SqliteStoreError::WorkerPanicked
            | SqliteStoreError::ReplyTimeout
            | SqliteStoreError::DatabaseOperationFailed
    ) {
        LocalRetentionError::new(LocalRetentionErrorKind::OutcomeUnknown, Some(source))
    } else {
        map_store_error(source)
    }
}
