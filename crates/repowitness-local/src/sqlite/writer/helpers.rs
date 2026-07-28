fn validate_prepared_identity(prepared: &PreparedRustIndex) -> Result<(), SqliteStoreError> {
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
        let identity = file.artifact_identity();
        let expected = hash_analysis_artifact_key(&AnalysisArtifactKey::new(
            file.content_digest(),
            identity.producer_manifest(),
            identity.configuration(),
            identity.schema(),
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
            "DELETE FROM artifact_fact_correspondence
             WHERE artifact_digest IN (
                SELECT artifact_digest FROM analysis_artifacts
                WHERE lifecycle_state = 'staging'
             )",
            [],
        )
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
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
