struct RawMemoryVersion {
    record_id: Vec<u8>,
    revision: Vec<u8>,
    canonical_json: Vec<u8>,
    display_revision: i64,
    locally_approved: bool,
}

fn memory_record_id(bytes: &[u8]) -> Result<MemoryRecordId, SqliteStoreError> {
    <[u8; 16]>::try_from(bytes)
        .map(MemoryRecordId::new)
        .map_err(|_| SqliteStoreError::IntegrityCheckFailed)
}

fn decode_commit(format: &str, bytes: &[u8]) -> Result<MemoryCommitId, SqliteStoreError> {
    match format {
        "sha1" => <[u8; 20]>::try_from(bytes)
            .map(MemoryCommitId::Sha1)
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed),
        "sha256" => <[u8; 32]>::try_from(bytes)
            .map(MemoryCommitId::Sha256)
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed),
        _ => Err(SqliteStoreError::IntegrityCheckFailed),
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

fn persisted_span(start: i64, end: i64) -> Result<ByteSpan, SqliteStoreError> {
    let start = nonnegative(start)?;
    let end = nonnegative(end)?;
    ByteSpan::try_new(ByteOffset::new(start), ByteOffset::new(end))
        .map_err(|_| SqliteStoreError::IntegrityCheckFailed)
}

fn nonnegative(value: i64) -> Result<u64, SqliteStoreError> {
    u64::try_from(value).map_err(|_| SqliteStoreError::IntegrityCheckFailed)
}

pub(super) fn with_progress_handler<T>(
    connection: &mut Connection,
    control: WriteControl<'_>,
    operation: impl FnOnce(&mut Connection) -> Result<T, SqliteStoreError>,
) -> Result<T, SqliteStoreError> {
    check_control(control)?;
    let cancelled = Arc::clone(control.cancelled);
    let deadline = control.deadline;
    connection
        .progress_handler(
            PROGRESS_INSTRUCTIONS,
            Some(move || cancelled.load(Ordering::Acquire) || Instant::now() >= deadline),
        )
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    let result = operation(connection);
    let clear = connection
        .progress_handler(0, None::<fn() -> bool>)
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed);
    match (result, clear) {
        (Err(error), _) | (Ok(_), Err(error)) => Err(error),
        (Ok(value), Ok(())) => Ok(value),
    }
}

pub(super) fn with_mutation_progress_handler<T>(
    connection: &mut Connection,
    control: WriteControl<'_>,
    force_clear_failure: bool,
    operation: impl FnOnce(&mut Connection) -> Result<T, SqliteStoreError>,
) -> WriterMutationResult<T> {
    if let Err(error) = check_control(control) {
        return WriterMutationResult::new(Err(error), true);
    }
    let cancelled = Arc::clone(control.cancelled);
    let deadline = control.deadline;
    if connection
        .progress_handler(
            PROGRESS_INSTRUCTIONS,
            Some(move || cancelled.load(Ordering::Acquire) || Instant::now() >= deadline),
        )
        .is_err()
    {
        return WriterMutationResult::new(Err(SqliteStoreError::DatabaseOperationFailed), false);
    }
    let result = operation(connection);
    let handler_cleared =
        connection.progress_handler(0, None::<fn() -> bool>).is_ok() && !force_clear_failure;
    WriterMutationResult::new(result, handler_cleared)
}

pub(super) fn control_database_error(control: WriteControl<'_>) -> SqliteStoreError {
    match check_control(control) {
        Ok(()) => SqliteStoreError::DatabaseOperationFailed,
        Err(error) => error,
    }
}

pub(super) fn check_control(control: WriteControl<'_>) -> Result<(), SqliteStoreError> {
    if control.cancelled.load(Ordering::Acquire) {
        Err(SqliteStoreError::Cancelled)
    } else if Instant::now() >= control.deadline {
        Err(SqliteStoreError::DeadlineExceeded)
    } else {
        Ok(())
    }
}
