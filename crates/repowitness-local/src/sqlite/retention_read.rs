use std::{
    path::Path,
    sync::{Arc, atomic::AtomicBool},
    time::Instant,
};

use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};

use super::{
    GenerationRetentionPolicy, RetentionApplyOutcome, RetentionPlan, RetentionPlanDigest,
    RetentionPolicyDigest, SqliteStoreError, open_index_reader,
    writer::{build_retention_plan, check_retention_control, retention_database_error},
};

const RETENTION_READ_PROGRESS_INSTRUCTIONS: i32 = 1_000;

/// Computes one current-schema retention plan through a query-only connection.
///
/// This boundary never acquires the mutation lease, migrates, recovers, or
/// starts the writer owner.
pub fn plan_generation_retention_read_only(
    database: &Path,
    policy: &GenerationRetentionPolicy,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
) -> Result<RetentionPlan, SqliteStoreError> {
    with_retention_read_transaction(database, cancelled, deadline, |transaction, control| {
        build_retention_plan(transaction, policy, control, deadline)
    })
}

pub(crate) fn load_retention_apply_outcome_read_only(
    database: &Path,
    policy_digest: RetentionPolicyDigest,
    plan_digest: RetentionPlanDigest,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
) -> Result<Option<RetentionApplyOutcome>, SqliteStoreError> {
    with_retention_read_transaction(database, cancelled, deadline, |transaction, control| {
        load_apply_outcome(transaction, policy_digest, plan_digest, control, deadline)
    })
}

fn with_retention_read_transaction<T>(
    database: &Path,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    operation: impl FnOnce(&Transaction<'_>, &AtomicBool) -> Result<T, SqliteStoreError>,
) -> Result<T, SqliteStoreError> {
    check_retention_control(cancelled.as_ref(), deadline)?;
    let mut connection = open_index_reader(database)?;
    check_retention_control(cancelled.as_ref(), deadline)?;
    let progress_cancelled = Arc::clone(&cancelled);
    connection
        .progress_handler(
            RETENTION_READ_PROGRESS_INSTRUCTIONS,
            Some(move || {
                progress_cancelled.load(std::sync::atomic::Ordering::Acquire)
                    || Instant::now() >= deadline
            }),
        )
        .map_err(|_| SqliteStoreError::ConfigurationFailed)?;
    let result = (|| {
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Deferred)
            .map_err(|error| retention_database_error(error, cancelled.as_ref(), deadline))?;
        let result = operation(&transaction, cancelled.as_ref())?;
        check_retention_control(cancelled.as_ref(), deadline)?;
        transaction
            .commit()
            .map_err(|error| retention_database_error(error, cancelled.as_ref(), deadline))?;
        Ok(result)
    })();
    let clear = connection
        .progress_handler(0, None::<fn() -> bool>)
        .map_err(|_| SqliteStoreError::ConfigurationFailed);
    match (result, clear) {
        (Err(error), _) | (Ok(_), Err(error)) => Err(error),
        (Ok(value), Ok(())) => Ok(value),
    }
}

fn load_apply_outcome(
    transaction: &Transaction<'_>,
    policy_digest: RetentionPolicyDigest,
    plan_digest: RetentionPlanDigest,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<Option<RetentionApplyOutcome>, SqliteStoreError> {
    check_retention_control(cancelled, deadline)?;
    let stored = transaction
        .query_row(
            "SELECT collection_id, generation_count, workspace_view_count,
                    source_slot_receipt_count, snapshot_count, artifact_count,
                    deleted_row_count, estimated_deleted_bytes, more_work
             FROM retention_collection_audit
             WHERE policy_digest = ?1 AND plan_digest = ?2",
            params![
                policy_digest.as_bytes().as_slice(),
                plan_digest.as_bytes().as_slice()
            ],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, bool>(8)?,
                ))
            },
        )
        .optional()
        .map_err(|error| retention_database_error(error, cancelled, deadline))?;
    stored.map(decode_apply_outcome).transpose()
}

fn decode_apply_outcome(
    stored: (i64, i64, i64, i64, i64, i64, i64, i64, bool),
) -> Result<RetentionApplyOutcome, SqliteStoreError> {
    let (collection, generations, views, receipts, snapshots, artifacts, rows, bytes, more_work) =
        stored;
    Ok(RetentionApplyOutcome::new(
        nonnegative_count(collection)?,
        nonnegative_count(generations)?,
        nonnegative_count(views)?,
        nonnegative_count(receipts)?,
        nonnegative_count(snapshots)?,
        nonnegative_count(artifacts)?,
        nonnegative_count(rows)?,
        nonnegative_count(bytes)?,
        more_work,
    ))
}

fn nonnegative_count(value: i64) -> Result<u64, SqliteStoreError> {
    u64::try_from(value).map_err(|_| SqliteStoreError::IntegrityCheckFailed)
}
