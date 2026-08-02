const FIXED_REPOSITORY_TOPOLOGY_ENTRY_OUTPUT_BYTES: u64 = 64;

use sha2::Digest;

fn execute_repository_topology_command(connection: &mut Connection, command: RepositoryTopologyCommand) {
    let RepositoryTopologyCommand { repository, limits, cancelled, deadline, reply } = command;
    let result = read_active_repository_topology(connection, repository, limits, cancelled, deadline);
    let _ = reply.try_send(result);
}

fn read_active_repository_topology(
    connection: &mut Connection,
    repository: RepositoryIdentityDigest,
    limits: RepositoryTopologyLimits,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
) -> Result<RepositoryTopologyPortResult<GenerationId>, SqliteStoreError> {
    check_control(&cancelled, deadline)?;
    let progress_cancelled = Arc::clone(&cancelled);
    connection.progress_handler(
        PROGRESS_OPCODES,
        Some(move || progress_cancelled.load(Ordering::Acquire) || Instant::now() >= deadline),
    ).map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    let result = repository_topology_transaction(connection, repository, limits);
    connection.progress_handler(0, None::<fn() -> bool>).map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    match result {
        Ok(result) => {
            check_control(&cancelled, deadline)?;
            Ok(result)
        }
        Err(SearchFailure::Sqlite(error)) if is_interrupted(&error) => {
            check_control(&cancelled, deadline)?;
            Err(SqliteStoreError::DatabaseOperationFailed)
        }
        Err(SearchFailure::Sqlite(_)) => Err(SqliteStoreError::DatabaseOperationFailed),
        Err(SearchFailure::Store(error)) => Err(error),
    }
}

fn repository_topology_transaction(
    connection: &mut Connection,
    repository: RepositoryIdentityDigest,
    limits: RepositoryTopologyLimits,
) -> Result<RepositoryTopologyPortResult<GenerationId>, SearchFailure> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Deferred)?;
    let state = active_generation_state(&transaction, repository)?;
    let (profile_version, digest, coverage, total_paths) =
        repository_topology_metadata(&transaction, state.generation)?;
    verify_repository_topology_digest(
        &transaction,
        state.generation,
        profile_version,
        &digest,
        total_paths,
    )?;
    let category_summaries = repository_topology_category_summaries(&transaction, state.generation)?;
    let (entries, output_bytes) = repository_topology_entries(&transaction, state.generation, limits)?;
    transaction.commit()?;
    Ok(RepositoryTopologyPortResult::new(
        RepositoryTopologyReceipt::new(
            state.snapshot,
            state.generation,
            profile_version,
            digest,
        ),
        coverage,
        entries,
        category_summaries,
        total_paths,
        output_bytes,
    ))
}

fn repository_topology_metadata(
    transaction: &Transaction<'_>,
    generation: GenerationId,
) -> Result<(u16, [u8; 32], RepositoryTopologyCoverage, u64), SearchFailure> {
    let metadata: Option<(i64, i64, Vec<u8>, i64, i64, i64)> = transaction.query_row(
        "SELECT publication.topology_profile_version, requirement.topology_profile_version,
                publication.topology_digest, publication.discovered_path_count,
                publication.omitted_path_count, publication.total_path_count
         FROM generation_repository_topology_publications AS publication
         JOIN generation_repository_topology_requirements AS requirement
           ON requirement.generation_id = publication.generation_id
         WHERE publication.generation_id = ?1 AND publication.lifecycle_state = 'complete'",
        [generation.get()],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)),
    ).optional()?;
    let Some((publication_profile, requirement_profile, digest, discovered, omitted, total)) = metadata else {
        return Err(SearchFailure::Store(SqliteStoreError::GenerationUnavailable));
    };
    let profile = u16::try_from(publication_profile)
        .ok()
        .filter(|profile| *profile == repowitness_application::REPOSITORY_TOPOLOGY_PROFILE_VERSION)
        .filter(|_| requirement_profile == publication_profile)
        .ok_or(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
    let digest: [u8; 32] = digest.try_into().map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
    let discovered = persisted_count(discovered)?;
    let omitted = persisted_count(omitted)?;
    let total = persisted_count(total)?;
    let entries: i64 = transaction.query_row(
        "SELECT count(*) FROM generation_repository_topology_entries WHERE generation_id = ?1",
        [generation.get()],
        |row| row.get(0),
    )?;
    if omitted != 0 || discovered != total || persisted_count(entries)? != total {
        return Err(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed));
    }
    Ok((profile, digest, RepositoryTopologyCoverage::new(discovered, omitted), total))
}

fn verify_repository_topology_digest(
    transaction: &Transaction<'_>,
    generation: GenerationId,
    profile_version: u16,
    expected_digest: &[u8; 32],
    total_paths: u64,
) -> Result<(), SearchFailure> {
    let mut statement = transaction.prepare(
        "SELECT repository_path, category FROM generation_repository_topology_entries
         WHERE generation_id = ?1 ORDER BY repository_path ASC",
    )?;
    let mut rows = statement.query([generation.get()])?;
    let mut hasher = crate::repository_topology::repository_topology_hasher(profile_version);
    let mut count = 0_u64;
    while let Some(row) = rows.next()? {
        let path_bytes: Vec<u8> = row.get(0)?;
        let category: String = row.get(1)?;
        let path = RepositoryPath::try_from_bytes(&path_bytes, PERSISTED_PATH_LIMITS)
            .map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
        let category = RepositoryTopologyCategory::from_stable_str(&category)
            .ok_or(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
        crate::repository_topology::update_repository_topology_hasher(&mut hasher, &path, category)
            .map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
        count = count
            .checked_add(1)
            .ok_or(SearchFailure::Store(SqliteStoreError::CountNotRepresentable))?;
    }
    let actual_digest: [u8; 32] = hasher.finalize().into();
    if count != total_paths || actual_digest != *expected_digest {
        return Err(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed));
    }
    Ok(())
}

fn repository_topology_category_summaries(
    transaction: &Transaction<'_>,
    generation: GenerationId,
) -> Result<Vec<RepositoryTopologyCategorySummary>, SearchFailure> {
    let mut statement = transaction.prepare(
        "SELECT category, count(*) FROM generation_repository_topology_entries
         WHERE generation_id = ?1 GROUP BY category ORDER BY category ASC",
    )?;
    let mut rows = statement.query([generation.get()])?;
    let mut counts = BTreeMap::new();
    while let Some(row) = rows.next()? {
        let category: String = row.get(0)?;
        let category = RepositoryTopologyCategory::from_stable_str(&category)
            .ok_or(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
        if counts.insert(category, persisted_count(row.get::<_, i64>(1)?)?).is_some() {
            return Err(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed));
        }
    }
    Ok(RepositoryTopologyCategory::all().into_iter().map(|category| {
        RepositoryTopologyCategorySummary::new(category, counts.remove(&category).unwrap_or(0))
    }).collect())
}

fn repository_topology_entries(
    transaction: &Transaction<'_>,
    generation: GenerationId,
    limits: RepositoryTopologyLimits,
) -> Result<(Vec<RepositoryTopologyEntry>, u64), SearchFailure> {
    let mut statement = transaction.prepare(
        "SELECT repository_path, category FROM generation_repository_topology_entries
         WHERE generation_id = ?1 ORDER BY repository_path ASC LIMIT ?2",
    )?;
    let mut rows = statement.query(params![generation.get(), i64::from(limits.max_paths())])?;
    let mut entries = Vec::with_capacity(usize::from(limits.max_paths()));
    let mut output_bytes = 0_u64;
    while let Some(row) = rows.next()? {
        let path_bytes: Vec<u8> = row.get(0)?;
        let category: String = row.get(1)?;
        let path = RepositoryPath::try_from_bytes(&path_bytes, PERSISTED_PATH_LIMITS)
            .map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
        let category = RepositoryTopologyCategory::from_stable_str(&category)
            .ok_or(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
        let next = output_bytes
            .checked_add(FIXED_REPOSITORY_TOPOLOGY_ENTRY_OUTPUT_BYTES)
            .and_then(|value| value.checked_add(path.byte_count().get()))
            .and_then(|value| value.checked_add(u64::try_from(category.as_str().len()).ok()?))
            .ok_or(SearchFailure::Store(SqliteStoreError::CountNotRepresentable))?;
        if next > limits.max_output_bytes() { break; }
        output_bytes = next;
        entries.push(RepositoryTopologyEntry::new(path, category));
    }
    Ok((entries, output_bytes))
}
