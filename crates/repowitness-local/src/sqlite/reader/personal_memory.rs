fn execute_personal_memory_read_command(
    connection: &mut Connection,
    command: PersonalMemoryReadCommand,
) {
    let PersonalMemoryReadCommand {
        profile,
        repository,
        limit,
        cancelled,
        deadline,
        reply,
    } = command;
    let result = read_personal_memory_records(connection, profile, repository, limit, &cancelled, deadline);
    let _ = reply.try_send(result);
}

fn read_personal_memory_records(
    connection: &Connection,
    profile: PersonalMemoryProfileId,
    repository: RepositoryIdentityDigest,
    limit: u16,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<Vec<PersonalMemoryRecord>, SqliteStoreError> {
    check_control(cancelled, deadline)?;
    let table_present = connection
        .query_row(
            "SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = 'personal_memory_records'",
            [],
            |_| Ok(()),
        )
        .optional()
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?
        .is_some();
    if !table_present {
        return Ok(Vec::new());
    }
    let mut statement = connection
        .prepare(
            "SELECT profile_id, repository_identity, record_id, revision_digest, kind,
                    title, body, lifecycle, recorded_at_unix_ms
               FROM personal_memory_records
              WHERE profile_id = ?1 AND repository_identity = ?2
              ORDER BY recorded_at_unix_ms, record_id, revision_digest
              LIMIT ?3",
        )
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    let mut rows = statement
        .query(params![
            profile.as_bytes().as_slice(),
            repository.as_bytes().as_slice(),
            i64::from(limit),
        ])
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    let mut records = Vec::with_capacity(usize::from(limit));
    while let Some(row) = rows
        .next()
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?
    {
        check_control(cancelled, deadline)?;
        let profile_bytes = row.get::<_, Vec<u8>>(0).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        let repository_bytes = row.get::<_, Vec<u8>>(1).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        let record_id = row.get::<_, Vec<u8>>(2).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        let revision = row.get::<_, Vec<u8>>(3).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        let kind = row.get::<_, String>(4).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        let title = row.get::<_, String>(5).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        let body = row.get::<_, String>(6).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        let lifecycle = row.get::<_, String>(7).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        let recorded_at = row.get::<_, i64>(8).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        let profile = <[u8; 16]>::try_from(profile_bytes.as_slice()).map(PersonalMemoryProfileId::new)
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        let repository = <[u8; 32]>::try_from(repository_bytes.as_slice()).map(RepositoryIdentityDigest::new)
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        let record_id = <[u8; 16]>::try_from(record_id.as_slice()).map(PersonalMemoryId::new)
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        let revision = <[u8; 32]>::try_from(revision.as_slice()).map(PersonalMemoryRevision::new)
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        let kind = personal_memory_kind_from_text(&kind).ok_or(SqliteStoreError::IntegrityCheckFailed)?;
        let lifecycle = memory_lifecycle_from_text(&lifecycle).ok_or(SqliteStoreError::IntegrityCheckFailed)?;
        let recorded_at = u64::try_from(recorded_at).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        let title = TaskText::try_new(title).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        let body = TaskText::try_new(body).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        records.push(PersonalMemoryRecord::new(profile, repository, record_id, revision, kind, title, body, lifecycle, recorded_at));
    }
    Ok(records)
}

fn personal_memory_kind_from_text(value: &str) -> Option<PersonalMemoryKind> {
    match value {
        "fact" => Some(PersonalMemoryKind::Fact), "decision" => Some(PersonalMemoryKind::Decision),
        "procedure" => Some(PersonalMemoryKind::Procedure), "episode" => Some(PersonalMemoryKind::Episode),
        "preference" => Some(PersonalMemoryKind::Preference), "policy" => Some(PersonalMemoryKind::Policy),
        "failure" => Some(PersonalMemoryKind::Failure), _ => None,
    }
}

fn memory_lifecycle_from_text(value: &str) -> Option<MemoryLifecycle> {
    match value {
        "active" => Some(MemoryLifecycle::Active), "needs_review" => Some(MemoryLifecycle::NeedsReview),
        "stale" => Some(MemoryLifecycle::Stale), "contradicted" => Some(MemoryLifecycle::Contradicted),
        "superseded" => Some(MemoryLifecycle::Superseded), "quarantined" => Some(MemoryLifecycle::Quarantined),
        "tombstoned" => Some(MemoryLifecycle::Tombstoned), _ => None,
    }
}
