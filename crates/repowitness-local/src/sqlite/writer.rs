use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use repowitness_application::{
    PreparedRustFile, PreparedRustIndex, RustIndexCoverage, RustSourceSnapshotIdentity,
    hash_analysis_artifact_key, hash_analysis_artifact_payload, hash_rust_source_snapshot,
};
use repowitness_domain::{
    AnalysisArtifactKey, RepositoryIdentityDigest, SourceFileKind, SourceSnapshotDigest,
};
use rusqlite::{
    Connection, ErrorCode, OptionalExtension, Transaction, TransactionBehavior, params,
};

use super::{
    SqliteStoreError,
    schema::{RECREATE_GENERATION_SEARCH, RECREATE_GENERATION_SEARCH_REBUILD},
};

const WRITE_BATCH_ROWS: usize = 256;
pub(super) const MAX_STARTUP_RECOVERY_GENERATIONS: usize = 4_096;
const RECOVERY_PROGRESS_INSTRUCTIONS: i32 = 1_000;
const MAX_PROJECTION_REBUILD_ROWS: u64 = 100_000_000;
const DEFAULT_PROJECTION_REBUILD_ROWS: u64 = 5_000_000;

type PersistedArtifactMetadata = (
    Vec<u8>,
    Vec<u8>,
    Vec<u8>,
    Vec<u8>,
    i64,
    i64,
    i64,
    i64,
    Option<Vec<u8>>,
);

/// Inclusive authoritative-row limit for one search-projection rebuild.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProjectionRebuildLimits {
    max_rows: u64,
}

impl ProjectionRebuildLimits {
    /// Constructs a nonzero Phase 0 projection-rebuild limit.
    pub const fn try_new(max_rows: u64) -> Result<Self, SqliteStoreError> {
        if max_rows == 0 || max_rows > MAX_PROJECTION_REBUILD_ROWS {
            return Err(SqliteStoreError::InvalidProjectionRebuildLimits);
        }
        Ok(Self { max_rows })
    }

    /// Returns the inclusive authoritative-row limit.
    #[must_use]
    pub const fn max_rows(self) -> u64 {
        self.max_rows
    }
}

impl Default for ProjectionRebuildLimits {
    fn default() -> Self {
        Self {
            max_rows: DEFAULT_PROJECTION_REBUILD_ROWS,
        }
    }
}

/// Bounded facts about one atomically published search-projection rebuild.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProjectionRebuildOutcome {
    previous_slot: u8,
    active_slot: u8,
    rebuilt_rows: u64,
    write_batches: u64,
}

impl ProjectionRebuildOutcome {
    /// Returns the projection slot that remained readable during the rebuild.
    #[must_use]
    pub const fn previous_slot(self) -> u8 {
        self.previous_slot
    }

    /// Returns the newly published projection slot.
    #[must_use]
    pub const fn active_slot(self) -> u8 {
        self.active_slot
    }

    /// Returns the exact number of rebuilt authoritative rows.
    #[must_use]
    pub const fn rebuilt_rows(self) -> u64 {
        self.rebuilt_rows
    }

    /// Returns the number of transactions that inserted at most 256 rows.
    #[must_use]
    pub const fn write_batches(self) -> u64 {
        self.write_batches
    }
}

/// Fixed-width database identity for one immutable index generation.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct GenerationId(i64);

impl GenerationId {
    pub(crate) const fn from_database(value: i64) -> Self {
        Self(value)
    }

    /// Returns the positive database-local identity.
    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }
}

/// One explicit WAL checkpoint observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CheckpointOutcome {
    busy: u64,
    log_frames: u64,
    checkpointed_frames: u64,
}

impl CheckpointOutcome {
    /// Returns SQLite's busy result count.
    #[must_use]
    pub const fn busy(self) -> u64 {
        self.busy
    }

    /// Returns the number of frames observed in the WAL.
    #[must_use]
    pub const fn log_frames(self) -> u64 {
        self.log_frames
    }

    /// Returns the number of frames checkpointed.
    #[must_use]
    pub const fn checkpointed_frames(self) -> u64 {
        self.checkpointed_frames
    }
}

#[derive(Clone, Copy)]
pub(super) struct WriteControl<'a> {
    pub(super) cancelled: &'a Arc<AtomicBool>,
    pub(super) deadline: Instant,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SearchProjection {
    Primary,
    Rebuild,
}

impl SearchProjection {
    const fn from_slot(slot: i64) -> Result<Self, SqliteStoreError> {
        match slot {
            0 => Ok(Self::Primary),
            1 => Ok(Self::Rebuild),
            _ => Err(SqliteStoreError::IntegrityCheckFailed),
        }
    }

    const fn slot(self) -> i64 {
        match self {
            Self::Primary => 0,
            Self::Rebuild => 1,
        }
    }

    const fn inactive(self) -> Self {
        match self {
            Self::Primary => Self::Rebuild,
            Self::Rebuild => Self::Primary,
        }
    }

    const fn stage_insert_sql(self) -> &'static str {
        match self {
            Self::Primary => INSERT_PRIMARY_SEARCH_ROW,
            Self::Rebuild => INSERT_REBUILD_SEARCH_ROW,
        }
    }

    const fn rebuild_insert_sql(self) -> &'static str {
        match self {
            Self::Primary => REBUILD_PRIMARY_BATCH,
            Self::Rebuild => REBUILD_SHADOW_BATCH,
        }
    }

    const fn count_sql(self) -> &'static str {
        match self {
            Self::Primary => "SELECT count(*) FROM generation_search",
            Self::Rebuild => "SELECT count(*) FROM generation_search_rebuild",
        }
    }

    const fn integrity_sql(self) -> &'static str {
        match self {
            Self::Primary => {
                "INSERT INTO generation_search(generation_search) VALUES('integrity-check')"
            }
            Self::Rebuild => {
                "INSERT INTO generation_search_rebuild(generation_search_rebuild)
                 VALUES('integrity-check')"
            }
        }
    }

    const fn recreate_sql(self) -> &'static str {
        match self {
            Self::Primary => RECREATE_GENERATION_SEARCH,
            Self::Rebuild => RECREATE_GENERATION_SEARCH_REBUILD,
        }
    }
}

#[derive(Clone, Copy)]
struct ProjectionCursor {
    generation: i64,
    file_ordinal: i64,
    fact_ordinal: i64,
}

const INITIAL_PROJECTION_CURSOR: ProjectionCursor = ProjectionCursor {
    generation: 0,
    file_ordinal: -1,
    fact_ordinal: -1,
};

const INSERT_PRIMARY_SEARCH_ROW: &str = "INSERT INTO generation_search(
    generation_id, repository_path, fact_ordinal,
    content_digest, artifact_digest, name_start, name_end,
    declaration_start, declaration_end, kind, name, qualified_name
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)";

const INSERT_REBUILD_SEARCH_ROW: &str = "INSERT INTO generation_search_rebuild(
    generation_id, repository_path, fact_ordinal,
    content_digest, artifact_digest, name_start, name_end,
    declaration_start, declaration_end, kind, name, qualified_name
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)";

const REBUILD_PRIMARY_BATCH: &str = "INSERT INTO generation_search(
    generation_id, repository_path, fact_ordinal,
    content_digest, artifact_digest, name_start, name_end,
    declaration_start, declaration_end, kind, name, qualified_name
)
SELECT generation.generation_id, file.repository_path, fact.ordinal,
       file.content_digest, file.artifact_digest, fact.name_start, fact.name_end,
       fact.declaration_start, fact.declaration_end, fact.kind, fact.name,
       fact.qualified_name
FROM index_generations AS generation
JOIN generation_files AS file USING (generation_id)
JOIN artifact_facts AS fact USING (artifact_digest)
WHERE generation.lifecycle_state IN ('ready', 'active', 'retained')
  AND (
    generation.generation_id > ?1 OR
    (generation.generation_id = ?1 AND file.ordinal > ?2) OR
    (generation.generation_id = ?1 AND file.ordinal = ?2 AND fact.ordinal > ?3)
  )
ORDER BY generation.generation_id, file.ordinal, fact.ordinal
LIMIT ?4";

const REBUILD_SHADOW_BATCH: &str = "INSERT INTO generation_search_rebuild(
    generation_id, repository_path, fact_ordinal,
    content_digest, artifact_digest, name_start, name_end,
    declaration_start, declaration_end, kind, name, qualified_name
)
SELECT generation.generation_id, file.repository_path, fact.ordinal,
       file.content_digest, file.artifact_digest, fact.name_start, fact.name_end,
       fact.declaration_start, fact.declaration_end, fact.kind, fact.name,
       fact.qualified_name
FROM index_generations AS generation
JOIN generation_files AS file USING (generation_id)
JOIN artifact_facts AS fact USING (artifact_digest)
WHERE generation.lifecycle_state IN ('ready', 'active', 'retained')
  AND (
    generation.generation_id > ?1 OR
    (generation.generation_id = ?1 AND file.ordinal > ?2) OR
    (generation.generation_id = ?1 AND file.ordinal = ?2 AND fact.ordinal > ?3)
  )
ORDER BY generation.generation_id, file.ordinal, fact.ordinal
LIMIT ?4";

const NEXT_PROJECTION_CURSOR: &str = "SELECT generation.generation_id, file.ordinal, fact.ordinal
FROM index_generations AS generation
JOIN generation_files AS file USING (generation_id)
JOIN artifact_facts AS fact USING (artifact_digest)
WHERE generation.lifecycle_state IN ('ready', 'active', 'retained')
  AND (
    generation.generation_id > ?1 OR
    (generation.generation_id = ?1 AND file.ordinal > ?2) OR
    (generation.generation_id = ?1 AND file.ordinal = ?2 AND fact.ordinal > ?3)
  )
ORDER BY generation.generation_id, file.ordinal, fact.ordinal
LIMIT 1 OFFSET ?4";

pub(super) struct WriterState {
    connection: Connection,
}

impl WriterState {
    pub(super) const fn new(connection: Connection) -> Self {
        Self { connection }
    }

    pub(super) fn recover(
        &mut self,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<u64, SqliteStoreError> {
        check_recovery_control(cancelled.as_ref(), deadline)?;
        let progress_cancelled = Arc::clone(&cancelled);
        self.connection
            .progress_handler(
                RECOVERY_PROGRESS_INSTRUCTIONS,
                Some(move || {
                    progress_cancelled.load(Ordering::Acquire) || Instant::now() >= deadline
                }),
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        let result = self.recover_with_control(cancelled.as_ref(), deadline);
        let clear_result = self
            .connection
            .progress_handler(0, None::<fn() -> bool>)
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed);
        match (result, clear_result) {
            (Err(error), _) | (Ok(_), Err(error)) => Err(error),
            (Ok(recovered), Ok(())) => Ok(recovered),
        }
    }

    fn recover_with_control(
        &mut self,
        cancelled: &AtomicBool,
        deadline: Instant,
    ) -> Result<u64, SqliteStoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| recovery_database_error(error, cancelled, deadline))?;
        let query_limit = i64::try_from(MAX_STARTUP_RECOVERY_GENERATIONS + 1)
            .map_err(|_| SqliteStoreError::CountNotRepresentable)?;
        let incomplete: Vec<i64> = {
            let mut statement = transaction
                .prepare(
                    "SELECT generation_id FROM index_generations
                     WHERE lifecycle_state IN (
                        'discovered', 'extracting', 'resolving', 'validating', 'ready'
                     )
                     ORDER BY generation_id
                     LIMIT ?1",
                )
                .map_err(|error| recovery_database_error(error, cancelled, deadline))?;
            let rows = statement
                .query_map([query_limit], |row| row.get(0))
                .map_err(|error| recovery_database_error(error, cancelled, deadline))?;
            rows.collect::<Result<_, _>>()
                .map_err(|error| recovery_database_error(error, cancelled, deadline))?
        };
        if incomplete.len() > MAX_STARTUP_RECOVERY_GENERATIONS {
            return Err(SqliteStoreError::RecoveryGenerationLimitExceeded);
        }
        for generation_id in &incomplete {
            check_recovery_control(cancelled, deadline)?;
            transaction
                .execute(
                    "DELETE FROM generation_search WHERE generation_id = ?1",
                    [generation_id],
                )
                .map_err(|error| recovery_database_error(error, cancelled, deadline))?;
            transaction
                .execute(
                    "DELETE FROM generation_search_rebuild WHERE generation_id = ?1",
                    [generation_id],
                )
                .map_err(|error| recovery_database_error(error, cancelled, deadline))?;
            transaction
                .execute(
                    "DELETE FROM generation_facts WHERE generation_id = ?1",
                    [generation_id],
                )
                .map_err(|error| recovery_database_error(error, cancelled, deadline))?;
            transaction
                .execute(
                    "DELETE FROM generation_files WHERE generation_id = ?1",
                    [generation_id],
                )
                .map_err(|error| recovery_database_error(error, cancelled, deadline))?;
            transaction
                .execute(
                    "UPDATE index_generations SET lifecycle_state = 'failed'
                     WHERE generation_id = ?1",
                    [generation_id],
                )
                .map_err(|error| recovery_database_error(error, cancelled, deadline))?;
        }
        check_recovery_control(cancelled, deadline)?;
        delete_staging_content(&transaction).map_err(|error| {
            if cancelled.load(Ordering::Acquire) {
                SqliteStoreError::Cancelled
            } else if Instant::now() >= deadline {
                SqliteStoreError::DeadlineExceeded
            } else {
                error
            }
        })?;
        check_recovery_control(cancelled, deadline)?;
        transaction
            .commit()
            .map_err(|error| recovery_database_error(error, cancelled, deadline))?;
        u64::try_from(incomplete.len()).map_err(|_| SqliteStoreError::CountNotRepresentable)
    }

    pub(super) fn register_workspace(
        &mut self,
        repository: RepositoryIdentityDigest,
        initial_source_epoch: u64,
    ) -> Result<i64, SqliteStoreError> {
        let epoch = fixed_integer(initial_source_epoch)?;
        let transaction = self.transaction()?;
        transaction
            .execute(
                "INSERT INTO workspaces(repository_identity, source_epoch)
                 VALUES (?1, ?2)
                 ON CONFLICT(repository_identity) DO NOTHING",
                params![repository.as_bytes().as_slice(), epoch],
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        let (workspace_id, stored_epoch): (i64, i64) = transaction
            .query_row(
                "SELECT workspace_id, source_epoch FROM workspaces
                 WHERE repository_identity = ?1",
                [repository.as_bytes().as_slice()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        if stored_epoch != epoch {
            return Err(SqliteStoreError::StaleSourceEpoch);
        }
        transaction
            .commit()
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        Ok(workspace_id)
    }

    pub(super) fn advance_source_epoch(
        &mut self,
        repository: RepositoryIdentityDigest,
        expected: u64,
        next: u64,
    ) -> Result<(), SqliteStoreError> {
        if next <= expected {
            return Err(SqliteStoreError::InvalidSourceEpoch);
        }
        let expected = fixed_integer(expected)?;
        let next = fixed_integer(next)?;
        let changed = self
            .connection
            .execute(
                "UPDATE workspaces SET source_epoch = ?1
                 WHERE repository_identity = ?2 AND source_epoch = ?3",
                params![next, repository.as_bytes().as_slice(), expected],
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        if changed != 1 {
            return Err(SqliteStoreError::StaleSourceEpoch);
        }
        Ok(())
    }

    pub(super) fn stage(
        &mut self,
        source_epoch: u64,
        identity: RustSourceSnapshotIdentity,
        prepared: &PreparedRustIndex,
        coverage: RustIndexCoverage,
        control: WriteControl<'_>,
    ) -> Result<GenerationId, SqliteStoreError> {
        check_control(control)?;
        let workspace_id = self.workspace(identity.repository(), source_epoch)?;
        validate_prepared_identity(identity, prepared)?;
        let snapshot_digest = hash_rust_source_snapshot(identity, prepared.manifest_digest());
        self.ensure_snapshot(snapshot_digest, identity, prepared, control)?;
        for file in prepared.files() {
            check_control(control)?;
            self.ensure_artifact(identity, file, control)?;
        }
        let generation = self.create_generation(workspace_id, source_epoch, snapshot_digest)?;
        let result = self.stage_generation_rows(generation, prepared, coverage, control);
        if let Err(error) = result {
            let target = if error == SqliteStoreError::Cancelled {
                "cancelled"
            } else {
                "failed"
            };
            let _ = self.fail_generation(generation, target);
            return Err(error);
        }
        Ok(generation)
    }

    pub(super) fn activate(
        &mut self,
        generation: GenerationId,
        expected_source_epoch: u64,
    ) -> Result<(), SqliteStoreError> {
        let expected_epoch = fixed_integer(expected_source_epoch)?;
        let transaction = self.transaction()?;
        let (workspace_id, generation_epoch, state): (i64, i64, String) = transaction
            .query_row(
                "SELECT workspace_id, source_epoch, lifecycle_state
                 FROM index_generations WHERE generation_id = ?1",
                [generation.get()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .map_err(|_| SqliteStoreError::GenerationUnavailable)?;
        let workspace_epoch: i64 = transaction
            .query_row(
                "SELECT source_epoch FROM workspaces WHERE workspace_id = ?1",
                [workspace_id],
                |row| row.get(0),
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        if state != "ready"
            || generation_epoch != expected_epoch
            || workspace_epoch != expected_epoch
        {
            return Err(SqliteStoreError::StaleSourceEpoch);
        }
        transaction
            .execute(
                "UPDATE index_generations SET lifecycle_state = 'retained'
                 WHERE workspace_id = ?1 AND lifecycle_state = 'active'",
                [workspace_id],
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        let changed = transaction
            .execute(
                "UPDATE index_generations SET lifecycle_state = 'active'
                 WHERE generation_id = ?1 AND lifecycle_state = 'ready'",
                [generation.get()],
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        if changed != 1 {
            return Err(SqliteStoreError::GenerationUnavailable);
        }
        transaction
            .execute(
                "UPDATE workspaces SET active_generation_id = ?1 WHERE workspace_id = ?2",
                params![generation.get(), workspace_id],
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        transaction
            .commit()
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)
    }

    pub(super) fn active_generation(
        &self,
        repository: RepositoryIdentityDigest,
    ) -> Result<Option<GenerationId>, SqliteStoreError> {
        self.connection
            .query_row(
                "SELECT active_generation_id FROM workspaces
                 WHERE repository_identity = ?1",
                [repository.as_bytes().as_slice()],
                |row| row.get::<_, Option<i64>>(0),
            )
            .optional()
            .map(|value| value.flatten().map(GenerationId))
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)
    }

    pub(super) fn checkpoint(&self) -> Result<CheckpointOutcome, SqliteStoreError> {
        let (busy, log_frames, checkpointed_frames): (i64, i64, i64) = self
            .connection
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        Ok(CheckpointOutcome {
            busy: positive_database_count(busy)?,
            log_frames: positive_database_count(log_frames)?,
            checkpointed_frames: positive_database_count(checkpointed_frames)?,
        })
    }

    pub(super) fn rebuild_search_projection(
        &mut self,
        limits: ProjectionRebuildLimits,
        control: WriteControl<'_>,
    ) -> Result<ProjectionRebuildOutcome, SqliteStoreError> {
        check_control(control)?;
        let progress_cancelled = Arc::clone(control.cancelled);
        let deadline = control.deadline;
        self.connection
            .progress_handler(
                1_000,
                Some(move || {
                    progress_cancelled.load(Ordering::Acquire) || Instant::now() >= deadline
                }),
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        let result = self.rebuild_search_projection_inner(limits, control);
        let clear_result = self
            .connection
            .progress_handler(0, None::<fn() -> bool>)
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed);
        clear_result?;
        match result {
            Err(SqliteStoreError::DatabaseOperationFailed) => {
                check_control(control)?;
                Err(SqliteStoreError::DatabaseOperationFailed)
            }
            other => other,
        }
    }

    fn rebuild_search_projection_inner(
        &mut self,
        limits: ProjectionRebuildLimits,
        control: WriteControl<'_>,
    ) -> Result<ProjectionRebuildOutcome, SqliteStoreError> {
        let current = self.active_search_projection()?;
        let target = current.inactive();
        let expected_rows = self.projection_source_row_count()?;
        if expected_rows > limits.max_rows() {
            return Err(SqliteStoreError::ProjectionRebuildRowLimitExceeded);
        }
        check_control(control)?;
        self.reset_projection(target)?;
        let (rebuilt_rows, write_batches) =
            self.populate_projection(target, expected_rows, control)?;
        self.verify_projection(target, expected_rows)?;
        check_control(control)?;
        self.publish_projection(current, target)?;
        Ok(ProjectionRebuildOutcome {
            previous_slot: u8::try_from(current.slot())
                .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
            active_slot: u8::try_from(target.slot())
                .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
            rebuilt_rows,
            write_batches,
        })
    }

    fn projection_source_row_count(&self) -> Result<u64, SqliteStoreError> {
        let rows: i64 = self
            .connection
            .query_row(
                "SELECT count(*)
                 FROM index_generations AS generation
                 JOIN generation_files AS file USING (generation_id)
                 JOIN artifact_facts AS fact USING (artifact_digest)
                 WHERE generation.lifecycle_state IN ('ready', 'active', 'retained')",
                [],
                |row| row.get(0),
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        positive_database_count(rows)
    }

    fn reset_projection(&mut self, target: SearchProjection) -> Result<(), SqliteStoreError> {
        let reset = self.transaction()?;
        reset
            .execute_batch(target.recreate_sql())
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        reset
            .commit()
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)
    }

    fn populate_projection(
        &mut self,
        target: SearchProjection,
        expected_rows: u64,
        control: WriteControl<'_>,
    ) -> Result<(u64, u64), SqliteStoreError> {
        let mut rebuilt_rows = 0_u64;
        let mut write_batches = 0_u64;
        let mut cursor = INITIAL_PROJECTION_CURSOR;
        while rebuilt_rows < expected_rows {
            check_control(control)?;
            let transaction = self.transaction()?;
            let inserted = transaction
                .execute(
                    target.rebuild_insert_sql(),
                    params![
                        cursor.generation,
                        cursor.file_ordinal,
                        cursor.fact_ordinal,
                        fixed_usize(WRITE_BATCH_ROWS)?
                    ],
                )
                .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
            if inserted == 0 || inserted > WRITE_BATCH_ROWS {
                return Err(SqliteStoreError::IntegrityCheckFailed);
            }
            let offset = inserted
                .checked_sub(1)
                .ok_or(SqliteStoreError::IntegrityCheckFailed)?;
            cursor = transaction
                .query_row(
                    NEXT_PROJECTION_CURSOR,
                    params![
                        cursor.generation,
                        cursor.file_ordinal,
                        cursor.fact_ordinal,
                        fixed_usize(offset)?
                    ],
                    |row| {
                        Ok(ProjectionCursor {
                            generation: row.get(0)?,
                            file_ordinal: row.get(1)?,
                            fact_ordinal: row.get(2)?,
                        })
                    },
                )
                .map_err(projection_validation_error)?;
            transaction
                .commit()
                .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
            rebuilt_rows = rebuilt_rows
                .checked_add(
                    u64::try_from(inserted).map_err(|_| SqliteStoreError::CountNotRepresentable)?,
                )
                .ok_or(SqliteStoreError::CountNotRepresentable)?;
            write_batches = write_batches
                .checked_add(1)
                .ok_or(SqliteStoreError::CountNotRepresentable)?;
        }
        Ok((rebuilt_rows, write_batches))
    }

    fn verify_projection(
        &self,
        target: SearchProjection,
        expected_rows: u64,
    ) -> Result<(), SqliteStoreError> {
        let actual_rows: i64 = self
            .connection
            .query_row(target.count_sql(), [], |row| row.get(0))
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        if positive_database_count(actual_rows)? != expected_rows {
            return Err(SqliteStoreError::IntegrityCheckFailed);
        }
        self.connection
            .execute(target.integrity_sql(), [])
            .map_err(projection_validation_error)?;
        Ok(())
    }

    fn publish_projection(
        &mut self,
        current: SearchProjection,
        target: SearchProjection,
    ) -> Result<(), SqliteStoreError> {
        let publication = self.transaction()?;
        let changed = publication
            .execute(
                "UPDATE search_projection_state SET active_slot = ?1
                 WHERE singleton = 1 AND active_slot = ?2",
                params![target.slot(), current.slot()],
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        if changed != 1 {
            return Err(SqliteStoreError::IntegrityCheckFailed);
        }
        publication
            .commit()
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)
    }

    fn active_search_projection(&self) -> Result<SearchProjection, SqliteStoreError> {
        let slot = self
            .connection
            .query_row(
                "SELECT active_slot FROM search_projection_state WHERE singleton = 1",
                [],
                |row| row.get(0),
            )
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        SearchProjection::from_slot(slot)
    }

    fn workspace(
        &self,
        repository: RepositoryIdentityDigest,
        source_epoch: u64,
    ) -> Result<i64, SqliteStoreError> {
        let epoch = fixed_integer(source_epoch)?;
        self.connection
            .query_row(
                "SELECT workspace_id FROM workspaces
                 WHERE repository_identity = ?1 AND source_epoch = ?2",
                params![repository.as_bytes().as_slice(), epoch],
                |row| row.get(0),
            )
            .map_err(|_| SqliteStoreError::StaleSourceEpoch)
    }

    fn ensure_snapshot(
        &mut self,
        digest: SourceSnapshotDigest,
        identity: RustSourceSnapshotIdentity,
        prepared: &PreparedRustIndex,
        control: WriteControl<'_>,
    ) -> Result<(), SqliteStoreError> {
        let existing = self
            .connection
            .query_row(
                "SELECT lifecycle_state FROM source_snapshots WHERE snapshot_digest = ?1",
                [digest.as_bytes().as_slice()],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        if existing.as_deref() == Some("complete") {
            return self.verify_snapshot(digest, identity, prepared);
        }
        if existing.is_some() {
            self.delete_staging_snapshot(digest)?;
        }
        let file_count = fixed_integer(prepared.manifest().count().get())?;
        let source_bytes = fixed_integer(prepared.total_source_bytes())?;
        let syntax_errors = fixed_integer(prepared.total_syntax_error_nodes())?;
        self.connection
            .execute(
                "INSERT INTO source_snapshots(
                    snapshot_digest, lifecycle_state, repository_identity, git_state_digest,
                    worktree_state_digest, configuration_digest, producer_manifest_digest,
                    analysis_schema_digest, canonicalization_version, manifest_digest,
                    file_count, total_source_bytes, total_syntax_error_nodes
                 ) VALUES (
                    ?1, 'staging', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12
                 )",
                params![
                    digest.as_bytes().as_slice(),
                    identity.repository().as_bytes().as_slice(),
                    identity.git_state().as_bytes().as_slice(),
                    identity.worktree_state().as_bytes().as_slice(),
                    identity.configuration().as_bytes().as_slice(),
                    identity.producer_manifest().as_bytes().as_slice(),
                    identity.analysis_schema().as_bytes().as_slice(),
                    i64::from(identity.canonicalization_version()),
                    prepared.manifest_digest().as_bytes().as_slice(),
                    file_count,
                    source_bytes,
                    syntax_errors
                ],
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        for (batch_index, batch) in prepared
            .manifest()
            .as_slice()
            .chunks(WRITE_BATCH_ROWS)
            .enumerate()
        {
            check_control(control)?;
            let transaction = self.transaction()?;
            for (offset, entry) in batch.iter().enumerate() {
                let ordinal = batch_index
                    .checked_mul(WRITE_BATCH_ROWS)
                    .and_then(|value| value.checked_add(offset))
                    .ok_or(SqliteStoreError::CountNotRepresentable)?;
                transaction
                    .execute(
                        "INSERT INTO source_manifest_entries(
                            snapshot_digest, ordinal, repository_path, file_kind, content_digest
                         ) VALUES (?1, ?2, ?3, ?4, ?5)",
                        params![
                            digest.as_bytes().as_slice(),
                            fixed_usize(ordinal)?,
                            entry.path().as_bytes(),
                            file_kind(*entry.file_type()),
                            entry.content_digest().as_bytes().as_slice()
                        ],
                    )
                    .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
            }
            transaction
                .commit()
                .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        }
        check_control(control)?;
        let changed = self
            .connection
            .execute(
                "UPDATE source_snapshots SET lifecycle_state = 'complete'
                 WHERE snapshot_digest = ?1 AND lifecycle_state = 'staging'
                 AND file_count = (
                    SELECT count(*) FROM source_manifest_entries
                    WHERE snapshot_digest = ?1
                 )",
                [digest.as_bytes().as_slice()],
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        if changed != 1 {
            return Err(SqliteStoreError::IntegrityCheckFailed);
        }
        Ok(())
    }

    fn ensure_artifact(
        &mut self,
        identity: RustSourceSnapshotIdentity,
        file: &PreparedRustFile,
        control: WriteControl<'_>,
    ) -> Result<(), SqliteStoreError> {
        let expected_digest = hash_analysis_artifact_key(&AnalysisArtifactKey::new(
            file.content_digest(),
            identity.producer_manifest(),
            identity.configuration(),
            identity.analysis_schema(),
            identity.canonicalization_version(),
        ));
        if expected_digest != file.artifact_digest() {
            return Err(SqliteStoreError::PreparedIdentityMismatch);
        }
        let existing = self
            .connection
            .query_row(
                "SELECT lifecycle_state FROM analysis_artifacts WHERE artifact_digest = ?1",
                [expected_digest.as_bytes().as_slice()],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        if existing.as_deref() == Some("complete") {
            return self.verify_artifact(identity, file, control);
        }
        if existing.is_some() {
            self.delete_staging_artifact(expected_digest.as_bytes())?;
        }
        let analysis = file.analysis();
        let payload_digest = hash_analysis_artifact_payload(analysis);
        self.connection
            .execute(
                "INSERT INTO analysis_artifacts(
                    artifact_digest, lifecycle_state, source_content_digest,
                    producer_manifest_digest, configuration_digest, analysis_schema_digest,
                    canonicalization_version, fact_count, visited_nodes, syntax_error_nodes,
                    payload_digest
                 ) VALUES (?1, 'staging', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    expected_digest.as_bytes().as_slice(),
                    file.content_digest().as_bytes().as_slice(),
                    identity.producer_manifest().as_bytes().as_slice(),
                    identity.configuration().as_bytes().as_slice(),
                    identity.analysis_schema().as_bytes().as_slice(),
                    i64::from(identity.canonicalization_version()),
                    fixed_usize(analysis.facts().len())?,
                    i64::from(analysis.visited_nodes()),
                    i64::from(analysis.syntax_error_nodes()),
                    payload_digest.as_bytes().as_slice()
                ],
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        for (batch_index, batch) in analysis.facts().chunks(WRITE_BATCH_ROWS).enumerate() {
            check_control(control)?;
            let transaction = self.transaction()?;
            for (offset, fact) in batch.iter().enumerate() {
                let ordinal = batch_index
                    .checked_mul(WRITE_BATCH_ROWS)
                    .and_then(|value| value.checked_add(offset))
                    .ok_or(SqliteStoreError::CountNotRepresentable)?;
                transaction
                    .execute(
                        "INSERT INTO artifact_facts(
                            artifact_digest, ordinal, kind, name, qualified_name,
                            name_start, name_end, declaration_start, declaration_end
                         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                        params![
                            expected_digest.as_bytes().as_slice(),
                            fixed_usize(ordinal)?,
                            fact.kind().as_str(),
                            fact.name(),
                            fact.qualified_name(),
                            fixed_integer(fact.name_span().start().get())?,
                            fixed_integer(fact.name_span().end().get())?,
                            fixed_integer(fact.declaration_span().start().get())?,
                            fixed_integer(fact.declaration_span().end().get())?
                        ],
                    )
                    .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
            }
            transaction
                .commit()
                .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        }
        check_control(control)?;
        let changed = self
            .connection
            .execute(
                "UPDATE analysis_artifacts SET lifecycle_state = 'complete'
                 WHERE artifact_digest = ?1 AND lifecycle_state = 'staging'
                 AND fact_count = (
                    SELECT count(*) FROM artifact_facts WHERE artifact_digest = ?1
                 )",
                [expected_digest.as_bytes().as_slice()],
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        if changed != 1 {
            return Err(SqliteStoreError::IntegrityCheckFailed);
        }
        Ok(())
    }

    fn create_generation(
        &mut self,
        workspace_id: i64,
        source_epoch: u64,
        snapshot: SourceSnapshotDigest,
    ) -> Result<GenerationId, SqliteStoreError> {
        self.connection
            .execute(
                "INSERT INTO index_generations(
                    workspace_id, source_epoch, snapshot_digest, lifecycle_state
                 ) VALUES (?1, ?2, ?3, 'discovered')",
                params![
                    workspace_id,
                    fixed_integer(source_epoch)?,
                    snapshot.as_bytes().as_slice()
                ],
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        Ok(GenerationId(self.connection.last_insert_rowid()))
    }

    fn stage_generation_rows(
        &mut self,
        generation: GenerationId,
        prepared: &PreparedRustIndex,
        coverage: RustIndexCoverage,
        control: WriteControl<'_>,
    ) -> Result<(), SqliteStoreError> {
        let projection = self.active_search_projection()?;
        self.transition(generation, "discovered", "extracting")?;
        for (batch_index, batch) in prepared.files().chunks(WRITE_BATCH_ROWS).enumerate() {
            check_control(control)?;
            let transaction = self.transaction()?;
            for (offset, file) in batch.iter().enumerate() {
                let ordinal = batch_index
                    .checked_mul(WRITE_BATCH_ROWS)
                    .and_then(|value| value.checked_add(offset))
                    .ok_or(SqliteStoreError::CountNotRepresentable)?;
                transaction
                    .execute(
                        "INSERT INTO generation_files(
                            generation_id, ordinal, repository_path,
                            content_digest, artifact_digest
                         ) VALUES (?1, ?2, ?3, ?4, ?5)",
                        params![
                            generation.get(),
                            fixed_usize(ordinal)?,
                            file.path().as_bytes(),
                            file.content_digest().as_bytes().as_slice(),
                            file.artifact_digest().as_bytes().as_slice()
                        ],
                    )
                    .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
            }
            transaction
                .commit()
                .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        }
        self.transition(generation, "extracting", "resolving")?;
        for file in prepared.files() {
            for (batch_index, batch) in file.analysis().facts().chunks(WRITE_BATCH_ROWS).enumerate()
            {
                check_control(control)?;
                let transaction = self.transaction()?;
                for (offset, fact) in batch.iter().enumerate() {
                    let ordinal = batch_index
                        .checked_mul(WRITE_BATCH_ROWS)
                        .and_then(|value| value.checked_add(offset))
                        .ok_or(SqliteStoreError::CountNotRepresentable)?;
                    transaction
                        .execute(
                            projection.stage_insert_sql(),
                            params![
                                generation.get(),
                                file.path().as_bytes(),
                                fixed_usize(ordinal)?,
                                file.content_digest().as_bytes().as_slice(),
                                file.artifact_digest().as_bytes().as_slice(),
                                fixed_integer(fact.name_span().start().get())?,
                                fixed_integer(fact.name_span().end().get())?,
                                fixed_integer(fact.declaration_span().start().get())?,
                                fixed_integer(fact.declaration_span().end().get())?,
                                fact.kind().as_str(),
                                fact.name(),
                                fact.qualified_name()
                            ],
                        )
                        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
                }
                transaction
                    .commit()
                    .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
            }
        }
        self.transition(generation, "resolving", "validating")?;
        check_control(control)?;
        self.validate_generation(generation, prepared, projection)?;
        let changed = self
            .connection
            .execute(
                "UPDATE index_generations
                 SET searched_count = ?1, skipped_count = ?2,
                     unresolved_count = ?3, truncated_count = ?4,
                     lifecycle_state = 'ready'
                 WHERE generation_id = ?5 AND lifecycle_state = 'validating'",
                params![
                    fixed_integer(coverage.searched())?,
                    fixed_integer(coverage.skipped())?,
                    fixed_integer(coverage.unresolved())?,
                    fixed_integer(coverage.truncated())?,
                    generation.get()
                ],
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        if changed != 1 {
            return Err(SqliteStoreError::IntegrityCheckFailed);
        }
        Ok(())
    }

    fn verify_snapshot(
        &self,
        digest: SourceSnapshotDigest,
        identity: RustSourceSnapshotIdentity,
        prepared: &PreparedRustIndex,
    ) -> Result<(), SqliteStoreError> {
        let expected = (
            identity.repository().as_bytes().to_vec(),
            identity.git_state().as_bytes().to_vec(),
            identity.worktree_state().as_bytes().to_vec(),
            identity.configuration().as_bytes().to_vec(),
            identity.producer_manifest().as_bytes().to_vec(),
            identity.analysis_schema().as_bytes().to_vec(),
            i64::from(identity.canonicalization_version()),
            prepared.manifest_digest().as_bytes().to_vec(),
            fixed_integer(prepared.manifest().count().get())?,
            fixed_integer(prepared.total_source_bytes())?,
            fixed_integer(prepared.total_syntax_error_nodes())?,
        );
        let actual = self
            .connection
            .query_row(
                "SELECT repository_identity, git_state_digest, worktree_state_digest,
                        configuration_digest, producer_manifest_digest, analysis_schema_digest,
                        canonicalization_version, manifest_digest, file_count,
                        total_source_bytes, total_syntax_error_nodes
                 FROM source_snapshots WHERE snapshot_digest = ?1
                 AND lifecycle_state = 'complete'",
                [digest.as_bytes().as_slice()],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                        row.get(8)?,
                        row.get(9)?,
                        row.get(10)?,
                    ))
                },
            )
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        if actual != expected {
            return Err(SqliteStoreError::IntegrityCheckFailed);
        }
        Ok(())
    }

    fn verify_artifact(
        &self,
        identity: RustSourceSnapshotIdentity,
        file: &PreparedRustFile,
        control: WriteControl<'_>,
    ) -> Result<(), SqliteStoreError> {
        check_control(control)?;
        let analysis = file.analysis();
        let expected_payload = hash_analysis_artifact_payload(analysis);
        let expected = (
            file.content_digest().as_bytes().to_vec(),
            identity.producer_manifest().as_bytes().to_vec(),
            identity.configuration().as_bytes().to_vec(),
            identity.analysis_schema().as_bytes().to_vec(),
            i64::from(identity.canonicalization_version()),
            fixed_usize(analysis.facts().len())?,
            i64::from(analysis.visited_nodes()),
            i64::from(analysis.syntax_error_nodes()),
        );
        let actual: PersistedArtifactMetadata = self
            .connection
            .query_row(
                "SELECT source_content_digest, producer_manifest_digest,
                        configuration_digest, analysis_schema_digest,
                        canonicalization_version, fact_count, visited_nodes, syntax_error_nodes,
                        payload_digest
                 FROM analysis_artifacts
                 WHERE artifact_digest = ?1 AND lifecycle_state = 'complete'",
                [file.artifact_digest().as_bytes().as_slice()],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                        row.get::<_, Option<Vec<u8>>>(8)?,
                    ))
                },
            )
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        if actual.0 != expected.0
            || actual.1 != expected.1
            || actual.2 != expected.2
            || actual.3 != expected.3
            || actual.4 != expected.4
            || actual.5 != expected.5
            || actual.6 != expected.6
            || actual.7 != expected.7
        {
            return Err(SqliteStoreError::IntegrityCheckFailed);
        }
        self.verify_artifact_facts(file, control)?;
        match actual.8 {
            Some(payload) if payload.as_slice() == expected_payload.as_bytes() => {}
            Some(_) => return Err(SqliteStoreError::IntegrityCheckFailed),
            None => {
                let changed = self
                    .connection
                    .execute(
                        "UPDATE analysis_artifacts SET payload_digest = ?2
                         WHERE artifact_digest = ?1 AND lifecycle_state = 'complete'
                         AND payload_digest IS NULL",
                        params![
                            file.artifact_digest().as_bytes().as_slice(),
                            expected_payload.as_bytes().as_slice()
                        ],
                    )
                    .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
                if changed != 1 {
                    return Err(SqliteStoreError::IntegrityCheckFailed);
                }
            }
        }
        check_control(control)?;
        Ok(())
    }

    fn verify_artifact_facts(
        &self,
        file: &PreparedRustFile,
        control: WriteControl<'_>,
    ) -> Result<(), SqliteStoreError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT ordinal, kind, name, qualified_name,
                        name_start, name_end, declaration_start, declaration_end
                 FROM artifact_facts
                 WHERE artifact_digest = ?1
                 ORDER BY ordinal",
            )
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        let mut rows = statement
            .query([file.artifact_digest().as_bytes().as_slice()])
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        for (ordinal, fact) in file.analysis().facts().iter().enumerate() {
            check_control(control)?;
            let row = rows
                .next()
                .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?
                .ok_or(SqliteStoreError::IntegrityCheckFailed)?;
            let stored_ordinal: i64 = row
                .get(0)
                .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
            let kind: String = row
                .get(1)
                .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
            let name: String = row
                .get(2)
                .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
            let qualified_name: String = row
                .get(3)
                .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
            let name_start: i64 = row
                .get(4)
                .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
            let name_end: i64 = row
                .get(5)
                .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
            let declaration_start: i64 = row
                .get(6)
                .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
            let declaration_end: i64 = row
                .get(7)
                .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
            if stored_ordinal != fixed_usize(ordinal)?
                || kind != fact.kind().as_str()
                || name != fact.name()
                || qualified_name != fact.qualified_name()
                || name_start != fixed_integer(fact.name_span().start().get())?
                || name_end != fixed_integer(fact.name_span().end().get())?
                || declaration_start != fixed_integer(fact.declaration_span().start().get())?
                || declaration_end != fixed_integer(fact.declaration_span().end().get())?
            {
                return Err(SqliteStoreError::IntegrityCheckFailed);
            }
        }
        if rows
            .next()
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?
            .is_some()
        {
            return Err(SqliteStoreError::IntegrityCheckFailed);
        }
        Ok(())
    }

    fn validate_generation(
        &self,
        generation: GenerationId,
        prepared: &PreparedRustIndex,
        projection: SearchProjection,
    ) -> Result<(), SqliteStoreError> {
        let search_count_sql = match projection {
            SearchProjection::Primary => {
                "SELECT count(*) FROM generation_search WHERE generation_id = ?1"
            }
            SearchProjection::Rebuild => {
                "SELECT count(*) FROM generation_search_rebuild WHERE generation_id = ?1"
            }
        };
        let files: i64 = self
            .connection
            .query_row(
                "SELECT count(*) FROM generation_files WHERE generation_id = ?1",
                [generation.get()],
                |row| row.get(0),
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        let facts: i64 = self
            .connection
            .query_row(search_count_sql, [generation.get()], |row| row.get(0))
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        if files != fixed_usize(prepared.files().len())?
            || facts != fixed_integer(prepared.total_facts())?
        {
            return Err(SqliteStoreError::IntegrityCheckFailed);
        }
        Ok(())
    }

    fn transition(
        &self,
        generation: GenerationId,
        expected: &str,
        next: &str,
    ) -> Result<(), SqliteStoreError> {
        let changed = self
            .connection
            .execute(
                "UPDATE index_generations SET lifecycle_state = ?1
                 WHERE generation_id = ?2 AND lifecycle_state = ?3",
                params![next, generation.get(), expected],
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        if changed != 1 {
            return Err(SqliteStoreError::GenerationUnavailable);
        }
        Ok(())
    }

    fn fail_generation(
        &self,
        generation: GenerationId,
        target: &str,
    ) -> Result<(), SqliteStoreError> {
        self.connection
            .execute(
                "UPDATE index_generations SET lifecycle_state = ?1
                 WHERE generation_id = ?2
                 AND lifecycle_state IN (
                    'discovered', 'extracting', 'resolving', 'validating', 'ready'
                 )",
                params![target, generation.get()],
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        self.connection
            .execute(
                "DELETE FROM generation_search WHERE generation_id = ?1",
                [generation.get()],
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        self.connection
            .execute(
                "DELETE FROM generation_search_rebuild WHERE generation_id = ?1",
                [generation.get()],
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        self.connection
            .execute(
                "DELETE FROM generation_facts WHERE generation_id = ?1",
                [generation.get()],
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        self.connection
            .execute(
                "DELETE FROM generation_files WHERE generation_id = ?1",
                [generation.get()],
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        Ok(())
    }

    fn delete_staging_snapshot(
        &mut self,
        digest: SourceSnapshotDigest,
    ) -> Result<(), SqliteStoreError> {
        let transaction = self.transaction()?;
        transaction
            .execute(
                "DELETE FROM source_manifest_entries WHERE snapshot_digest = ?1",
                [digest.as_bytes().as_slice()],
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        transaction
            .execute(
                "DELETE FROM source_snapshots
                 WHERE snapshot_digest = ?1 AND lifecycle_state = 'staging'",
                [digest.as_bytes().as_slice()],
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        transaction
            .commit()
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)
    }

    fn delete_staging_artifact(&mut self, digest: &[u8; 32]) -> Result<(), SqliteStoreError> {
        let transaction = self.transaction()?;
        transaction
            .execute(
                "DELETE FROM artifact_facts WHERE artifact_digest = ?1",
                [digest.as_slice()],
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        transaction
            .execute(
                "DELETE FROM analysis_artifacts
                 WHERE artifact_digest = ?1 AND lifecycle_state = 'staging'",
                [digest.as_slice()],
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        transaction
            .commit()
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)
    }

    fn transaction(&mut self) -> Result<Transaction<'_>, SqliteStoreError> {
        self.connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)
    }
}

fn validate_prepared_identity(
    identity: RustSourceSnapshotIdentity,
    prepared: &PreparedRustIndex,
) -> Result<(), SqliteStoreError> {
    if prepared.files().len() != prepared.manifest().as_slice().len() {
        return Err(SqliteStoreError::PreparedIdentityMismatch);
    }
    for (file, entry) in prepared.files().iter().zip(prepared.manifest().as_slice()) {
        if file.path() != entry.path()
            || file.content_digest() != *entry.content_digest()
            || *entry.file_type() != SourceFileKind::Regular
        {
            return Err(SqliteStoreError::PreparedIdentityMismatch);
        }
        let expected = hash_analysis_artifact_key(&AnalysisArtifactKey::new(
            file.content_digest(),
            identity.producer_manifest(),
            identity.configuration(),
            identity.analysis_schema(),
            identity.canonicalization_version(),
        ));
        if expected != file.artifact_digest() {
            return Err(SqliteStoreError::PreparedIdentityMismatch);
        }
    }
    Ok(())
}

fn delete_staging_content(transaction: &Transaction<'_>) -> Result<(), SqliteStoreError> {
    transaction
        .execute(
            "DELETE FROM artifact_facts
             WHERE artifact_digest IN (
                SELECT artifact_digest FROM analysis_artifacts
                WHERE lifecycle_state = 'staging'
             )",
            [],
        )
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    transaction
        .execute(
            "DELETE FROM analysis_artifacts WHERE lifecycle_state = 'staging'",
            [],
        )
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    transaction
        .execute(
            "DELETE FROM source_manifest_entries
             WHERE snapshot_digest IN (
                SELECT snapshot_digest FROM source_snapshots
                WHERE lifecycle_state = 'staging'
             )",
            [],
        )
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    transaction
        .execute(
            "DELETE FROM source_snapshots WHERE lifecycle_state = 'staging'",
            [],
        )
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    Ok(())
}

fn check_control(control: WriteControl<'_>) -> Result<(), SqliteStoreError> {
    if control.cancelled.load(Ordering::Acquire) {
        Err(SqliteStoreError::Cancelled)
    } else if Instant::now() >= control.deadline {
        Err(SqliteStoreError::DeadlineExceeded)
    } else {
        Ok(())
    }
}

fn check_recovery_control(
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), SqliteStoreError> {
    if cancelled.load(Ordering::Acquire) {
        Err(SqliteStoreError::Cancelled)
    } else if Instant::now() >= deadline {
        Err(SqliteStoreError::DeadlineExceeded)
    } else {
        Ok(())
    }
}

fn recovery_database_error(
    _error: rusqlite::Error,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> SqliteStoreError {
    if cancelled.load(Ordering::Acquire) {
        SqliteStoreError::Cancelled
    } else if Instant::now() >= deadline {
        SqliteStoreError::DeadlineExceeded
    } else {
        SqliteStoreError::DatabaseOperationFailed
    }
}

fn projection_validation_error(error: rusqlite::Error) -> SqliteStoreError {
    if matches!(
        error,
        rusqlite::Error::SqliteFailure(code, _) if code.code == ErrorCode::OperationInterrupted
    ) {
        SqliteStoreError::DatabaseOperationFailed
    } else {
        SqliteStoreError::IntegrityCheckFailed
    }
}

fn fixed_integer(value: u64) -> Result<i64, SqliteStoreError> {
    i64::try_from(value).map_err(|_| SqliteStoreError::CountNotRepresentable)
}

fn fixed_usize(value: usize) -> Result<i64, SqliteStoreError> {
    i64::try_from(value).map_err(|_| SqliteStoreError::CountNotRepresentable)
}

fn positive_database_count(value: i64) -> Result<u64, SqliteStoreError> {
    u64::try_from(value).map_err(|_| SqliteStoreError::IntegrityCheckFailed)
}

const fn file_kind(kind: SourceFileKind) -> &'static str {
    match kind {
        SourceFileKind::Regular => "regular",
        SourceFileKind::SymbolicLink => "symbolic_link",
        SourceFileKind::Gitlink => "gitlink",
        SourceFileKind::Other => "other",
    }
}

#[cfg(test)]
mod tests {
    use rusqlite::ffi::{Error, SQLITE_CORRUPT, SQLITE_INTERRUPT};

    use super::{SqliteStoreError, projection_validation_error};

    #[test]
    fn projection_validation_preserves_interruption_for_control_diagnostics() {
        let interrupted = rusqlite::Error::SqliteFailure(Error::new(SQLITE_INTERRUPT), None);
        let corrupt = rusqlite::Error::SqliteFailure(Error::new(SQLITE_CORRUPT), None);

        assert_eq!(
            projection_validation_error(interrupted),
            SqliteStoreError::DatabaseOperationFailed
        );
        assert_eq!(
            projection_validation_error(corrupt),
            SqliteStoreError::IntegrityCheckFailed
        );
    }
}
