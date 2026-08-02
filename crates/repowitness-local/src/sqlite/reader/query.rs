fn search_transaction(
    connection: &mut Connection,
    repository: RepositoryIdentityDigest,
    query: &str,
    limits: SearchLimits,
) -> Result<SearchResults, SearchFailure> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Deferred)?;
    let state = active_search_state(&transaction, repository)?;
    let total_matches = transaction.query_row(
        state.sql.count,
        params![query, state.generation.get()],
        |row| row.get::<_, i64>(0),
    )?;
    let total_matches = persisted_count(total_matches)?;
    let (hits, output_bytes) = search_hits(
        &transaction,
        state.sql.search,
        query,
        state.generation,
        limits,
    )?;
    transaction.commit()?;
    Ok(SearchResults {
        snapshot: state.snapshot,
        generation: state.generation,
        producer_manifest: state.producer_manifest,
        index_coverage: state.index_coverage,
        hits,
        total_matches,
        output_bytes,
    })
}

fn workspace_search_transaction(
    connection: &mut Connection,
    view: &PinnedWorkspaceView,
    source_slot: repowitness_domain::SourceSlotId,
    query: &str,
    limits: SearchLimits,
) -> Result<SearchResults, SearchFailure> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Deferred)?;
    let state = workspace_search_state(&transaction, view, source_slot)?;
    let total_matches = transaction.query_row(
        state.sql.count,
        params![query, state.generation.get()],
        |row| row.get::<_, i64>(0),
    )?;
    let total_matches = persisted_count(total_matches)?;
    let (hits, output_bytes) = search_hits(
        &transaction,
        state.sql.search,
        query,
        state.generation,
        limits,
    )?;
    transaction.commit()?;
    Ok(SearchResults {
        snapshot: state.snapshot,
        generation: state.generation,
        producer_manifest: state.producer_manifest,
        index_coverage: state.index_coverage,
        hits,
        total_matches,
        output_bytes,
    })
}

fn symbol_transaction(
    connection: &mut Connection,
    repository: RepositoryIdentityDigest,
    expected_snapshot: SourceSnapshotDigest,
    expected_generation: GenerationId,
    selector: &SymbolGetSelector,
) -> Result<SymbolLookupResults, SearchFailure> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Deferred)?;
    let state = active_generation_state(&transaction, repository)?;
    if state.snapshot != expected_snapshot || state.generation != expected_generation {
        return Err(SearchFailure::Store(
            SqliteStoreError::GenerationUnavailable,
        ));
    }
    let hit = exact_symbol_hit(&transaction, state.generation, selector)?;
    transaction.commit()?;
    Ok(SymbolLookupResults {
        snapshot: state.snapshot,
        generation: state.generation,
        producer_manifest: state.producer_manifest,
        index_coverage: state.index_coverage,
        hit,
    })
}

#[derive(Clone, Copy)]
struct SearchProjectionSql {
    search: &'static str,
    count: &'static str,
    symbol_search: &'static str,
    symbol_count: &'static str,
}

#[derive(Clone, Copy)]
struct ActiveSearchState {
    snapshot: SourceSnapshotDigest,
    generation: GenerationId,
    producer_manifest: ProducerManifestDigest,
    index_coverage: RustIndexCoverage,
    sql: SearchProjectionSql,
}

#[derive(Clone, Copy)]
struct ActiveGenerationState {
    snapshot: SourceSnapshotDigest,
    generation: GenerationId,
    source_epoch: u64,
    producer_manifest: ProducerManifestDigest,
    index_coverage: RustIndexCoverage,
}

fn active_search_state(
    transaction: &Transaction<'_>,
    repository: RepositoryIdentityDigest,
) -> Result<ActiveSearchState, SearchFailure> {
    let state = active_generation_state(transaction, repository)?;
    let projection_slot = transaction
        .query_row(
            "SELECT active_slot
             FROM search_projection_state
             WHERE singleton = 1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .ok_or(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
    let sql = match projection_slot {
        0 => SearchProjectionSql {
            search: PRIMARY_SEARCH_SQL,
            count: PRIMARY_COUNT_SQL,
            symbol_search: PRIMARY_SYMBOL_SEARCH_SQL,
            symbol_count: PRIMARY_SYMBOL_COUNT_SQL,
        },
        1 => SearchProjectionSql {
            search: REBUILD_SEARCH_SQL,
            count: REBUILD_COUNT_SQL,
            symbol_search: REBUILD_SYMBOL_SEARCH_SQL,
            symbol_count: REBUILD_SYMBOL_COUNT_SQL,
        },
        _ => {
            return Err(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed));
        }
    };
    Ok(ActiveSearchState {
        snapshot: state.snapshot,
        generation: state.generation,
        producer_manifest: state.producer_manifest,
        index_coverage: state.index_coverage,
        sql,
    })
}

fn workspace_search_state(
    transaction: &Transaction<'_>,
    view: &PinnedWorkspaceView,
    source_slot: repowitness_domain::SourceSlotId,
) -> Result<ActiveSearchState, SearchFailure> {
    let state = workspace_generation_state(transaction, view, source_slot)?;
    let projection_slot = transaction
        .query_row(
            "SELECT active_slot FROM search_projection_state WHERE singleton = 1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .ok_or(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
    let sql = match projection_slot {
        0 => SearchProjectionSql {
            search: PRIMARY_SEARCH_SQL,
            count: PRIMARY_COUNT_SQL,
            symbol_search: PRIMARY_SYMBOL_SEARCH_SQL,
            symbol_count: PRIMARY_SYMBOL_COUNT_SQL,
        },
        1 => SearchProjectionSql {
            search: REBUILD_SEARCH_SQL,
            count: REBUILD_COUNT_SQL,
            symbol_search: REBUILD_SYMBOL_SEARCH_SQL,
            symbol_count: REBUILD_SYMBOL_COUNT_SQL,
        },
        _ => return Err(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed)),
    };
    Ok(ActiveSearchState {
        snapshot: state.snapshot,
        generation: state.generation,
        producer_manifest: state.producer_manifest,
        index_coverage: state.index_coverage,
        sql,
    })
}

fn active_generation_state(
    transaction: &Transaction<'_>,
    repository: RepositoryIdentityDigest,
) -> Result<ActiveGenerationState, SearchFailure> {
    let persisted = transaction
        .query_row(
            "SELECT generation.generation_id, generation.snapshot_digest,
                    generation.source_epoch,
                    snapshot.producer_manifest_digest,
                    generation.searched_count, generation.skipped_count,
                    generation.unresolved_count, generation.truncated_count
             FROM workspaces AS workspace
             JOIN index_generations AS generation
              ON generation.generation_id = workspace.active_generation_id
             AND generation.workspace_id = workspace.workspace_id
             AND generation.lifecycle_state = 'active'
             JOIN source_snapshots AS snapshot
               ON snapshot.snapshot_digest = generation.snapshot_digest
              AND snapshot.lifecycle_state = 'complete'
             WHERE workspace.repository_identity = ?1
            ",
            [repository.as_bytes().as_slice()],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, Vec<u8>>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, i64>(7)?,
                ))
            },
        )
        .optional()?
        .ok_or(SearchFailure::Store(
            SqliteStoreError::GenerationUnavailable,
        ))?;
    let (
        generation,
        snapshot,
        source_epoch,
        producer_manifest,
        searched,
        skipped,
        unresolved,
        truncated,
    ) = persisted;
    let snapshot = SourceSnapshotDigest::try_from_slice(&snapshot)
        .map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
    let producer_manifest = ProducerManifestDigest::try_from_slice(&producer_manifest)
        .map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
    Ok(ActiveGenerationState {
        snapshot,
        generation: GenerationId::from_database(generation),
        source_epoch: persisted_count(source_epoch)?,
        producer_manifest,
        index_coverage: RustIndexCoverage::new(
            persisted_count(searched)?,
            persisted_count(skipped)?,
            persisted_count(unresolved)?,
            persisted_count(truncated)?,
        ),
    })
}

fn workspace_generation_state(
    transaction: &Transaction<'_>,
    view: &PinnedWorkspaceView,
    source_slot: repowitness_domain::SourceSlotId,
) -> Result<ActiveGenerationState, SearchFailure> {
    let member = view
        .members()
        .iter()
        .find(|member| member.source_slot() == source_slot)
        .ok_or(SearchFailure::Store(SqliteStoreError::InvalidWorkspaceView))?;
    let persisted = transaction
        .query_row(
            "SELECT generation.generation_id, generation.snapshot_digest,
                    generation.source_epoch, snapshot.producer_manifest_digest,
                    generation.searched_count, generation.skipped_count,
                    generation.unresolved_count, generation.truncated_count
             FROM workspace_view_members AS member
             JOIN active_workspace_views AS active
               ON active.connected_workspace_id = member.connected_workspace_id
              AND active.workspace_view_id = member.workspace_view_id
             JOIN index_generations AS generation
               ON generation.workspace_id = member.generation_workspace_id
              AND generation.generation_id = member.generation_id
             JOIN source_snapshots AS snapshot
               ON snapshot.snapshot_digest = generation.snapshot_digest
              AND snapshot.lifecycle_state = 'complete'
             WHERE member.workspace_view_id = ?1
               AND member.connected_workspace_id = ?2
               AND member.source_slot_id = ?3
               AND member.source_epoch = ?4
               AND member.generation_id = ?5",
            params![
                view.view().get(),
                view.connected_workspace().as_bytes().as_slice(),
                source_slot.as_bytes().as_slice(),
                i64::try_from(member.source_epoch().get())
                    .map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?,
                member.generation().get(),
            ],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, Vec<u8>>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, i64>(7)?,
                ))
            },
        )
        .optional()?
        .ok_or(SearchFailure::Store(SqliteStoreError::GenerationUnavailable))?;
    let (
        generation,
        snapshot,
        source_epoch,
        producer_manifest,
        searched,
        skipped,
        unresolved,
        truncated,
    ) = persisted;
    let snapshot = SourceSnapshotDigest::try_from_slice(&snapshot)
        .map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
    let producer_manifest = ProducerManifestDigest::try_from_slice(&producer_manifest)
        .map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
    Ok(ActiveGenerationState {
        snapshot,
        generation: GenerationId::from_database(generation),
        source_epoch: persisted_count(source_epoch)?,
        producer_manifest,
        index_coverage: RustIndexCoverage::new(
            persisted_count(searched)?,
            persisted_count(skipped)?,
            persisted_count(unresolved)?,
            persisted_count(truncated)?,
        ),
    })
}

fn search_hits(
    transaction: &Transaction<'_>,
    search_sql: &str,
    query: &str,
    generation: GenerationId,
    limits: SearchLimits,
) -> Result<(Box<[SearchHit]>, u64), SearchFailure> {
    let mut statement = transaction.prepare(search_sql)?;
    let mut rows = statement.query(params![
        query,
        generation.get(),
        i64::from(limits.max_results())
    ])?;
    let mut hits = Vec::with_capacity(usize::from(limits.max_results()));
    let mut output_bytes = 0_u64;
    while let Some(row) = rows.next()? {
        let hit = read_search_hit(row)?;
        output_bytes = checked_output_bytes(
            output_bytes,
            &hit.path,
            hit.language,
            hit.kind,
            &hit.name,
            &hit.qualified_name,
            limits.max_output_bytes(),
        )
        .map_err(SearchFailure::Store)?;
        hits.push(hit);
    }
    drop(rows);
    drop(statement);
    Ok((hits.into_boxed_slice(), output_bytes))
}

fn exact_symbol_hit(
    transaction: &Transaction<'_>,
    generation: GenerationId,
    selector: &SymbolGetSelector,
) -> Result<Option<SearchHit>, SearchFailure> {
    let mut statement = transaction.prepare(
        "SELECT file.repository_path, fact.ordinal, fact.kind, fact.name,
                fact.qualified_name, file.content_digest, file.artifact_digest,
                fact.name_start, fact.name_end,
                fact.declaration_start, fact.declaration_end, artifact.language,
                artifact.producer_manifest_digest
         FROM generation_files AS file
         JOIN artifact_facts AS fact
           ON fact.artifact_digest = file.artifact_digest
         JOIN analysis_artifacts AS artifact
           ON artifact.artifact_digest = file.artifact_digest
          AND artifact.lifecycle_state = 'complete'
         WHERE file.generation_id = ?1
           AND file.repository_path = ?2
           AND file.content_digest = ?3
           AND file.artifact_digest = ?4
           AND fact.ordinal = ?5",
    )?;
    let mut rows = statement.query(params![
        generation.get(),
        selector.path().as_bytes(),
        selector.content_digest().as_bytes().as_slice(),
        selector.artifact_digest().as_bytes().as_slice(),
        persisted_ordinal(selector.fact_ordinal())?,
    ])?;
    let hit = rows.next()?.map(read_search_hit).transpose()?;
    if rows.next()?.is_some() {
        return Err(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed));
    }
    drop(rows);
    drop(statement);
    Ok(hit)
}

fn read_search_hit(row: &rusqlite::Row<'_>) -> Result<SearchHit, SearchFailure> {
    let path_bytes: Vec<u8> = row.get(0)?;
    let fact_ordinal: i64 = row.get(1)?;
    let kind: String = row.get(2)?;
    let name: String = row.get(3)?;
    let qualified_name: String = row.get(4)?;
    let content_digest: Vec<u8> = row.get(5)?;
    let artifact_digest: Vec<u8> = row.get(6)?;
    let name_start: i64 = row.get(7)?;
    let name_end: i64 = row.get(8)?;
    let declaration_start: i64 = row.get(9)?;
    let declaration_end: i64 = row.get(10)?;
    let language: String = row.get(11)?;
    let producer_manifest: Vec<u8> = row.get(12)?;
    let path = RepositoryPath::try_from_bytes(&path_bytes, PERSISTED_PATH_LIMITS)
        .map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
    let fact_ordinal = u64::try_from(fact_ordinal)
        .map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
    let kind = parse_symbol_kind(&kind)
        .ok_or(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
    let content_digest = SourceContentDigest::try_from_slice(&content_digest)
        .map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
    let artifact_digest = AnalysisArtifactDigest::try_from_slice(&artifact_digest)
        .map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
    let producer_manifest = ProducerManifestDigest::try_from_slice(&producer_manifest)
        .map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
    let name_span = persisted_span(name_start, name_end).map_err(SearchFailure::Store)?;
    let declaration_span =
        persisted_span(declaration_start, declaration_end).map_err(SearchFailure::Store)?;
    let language = SourceLanguage::from_stable_str(&language)
        .ok_or(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
    if !language.matches_repository_path(&path) {
        return Err(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed));
    }
    Ok(SearchHit {
        path,
        language,
        fact_ordinal,
        content_digest,
        artifact_digest,
        producer_manifest,
        kind,
        name,
        qualified_name,
        name_span,
        declaration_span,
    })
}

fn persisted_ordinal(ordinal: u64) -> Result<i64, SearchFailure> {
    i64::try_from(ordinal).map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))
}

const PRIMARY_SEARCH_SQL: &str = "SELECT repository_path, fact_ordinal, kind, name, qualified_name,
       content_digest, generation_search.artifact_digest, name_start, name_end,
       declaration_start, declaration_end, artifact.language,
       artifact.producer_manifest_digest,
       bm25(
           generation_search,
           0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
           0.0, 0.0, 0.0, 1.0, 3.0, 2.0
       ) AS rank
FROM generation_search
JOIN analysis_artifacts AS artifact
  ON artifact.artifact_digest = generation_search.artifact_digest
 AND artifact.lifecycle_state = 'complete'
WHERE generation_search MATCH ?1 AND generation_id = ?2
ORDER BY rank ASC, repository_path ASC, fact_ordinal ASC
LIMIT ?3";

const REBUILD_SEARCH_SQL: &str = "SELECT repository_path, fact_ordinal, kind, name, qualified_name,
            content_digest, generation_search_rebuild.artifact_digest, name_start, name_end,
            declaration_start, declaration_end, artifact.language,
            artifact.producer_manifest_digest,
            bm25(
                generation_search_rebuild,
                0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
                0.0, 0.0, 0.0, 1.0, 3.0, 2.0
            ) AS rank
     FROM generation_search_rebuild
     JOIN analysis_artifacts AS artifact
       ON artifact.artifact_digest = generation_search_rebuild.artifact_digest
      AND artifact.lifecycle_state = 'complete'
     WHERE generation_search_rebuild MATCH ?1 AND generation_id = ?2
     ORDER BY rank ASC, repository_path ASC, fact_ordinal ASC
     LIMIT ?3";

const PRIMARY_COUNT_SQL: &str = "SELECT COUNT(*)
FROM generation_search
JOIN analysis_artifacts AS artifact
  ON artifact.artifact_digest = generation_search.artifact_digest
 AND artifact.lifecycle_state = 'complete'
WHERE generation_search MATCH ?1 AND generation_id = ?2";

const REBUILD_COUNT_SQL: &str = "SELECT COUNT(*)
FROM generation_search_rebuild
JOIN analysis_artifacts AS artifact
  ON artifact.artifact_digest = generation_search_rebuild.artifact_digest
 AND artifact.lifecycle_state = 'complete'
WHERE generation_search_rebuild MATCH ?1 AND generation_id = ?2";

const PRIMARY_SYMBOL_SEARCH_SQL: &str = "SELECT repository_path, fact_ordinal, kind, name,
       qualified_name, content_digest, generation_search.artifact_digest,
       name_start, name_end, declaration_start, declaration_end, artifact.language,
       artifact.producer_manifest_digest
FROM generation_search
JOIN analysis_artifacts AS artifact
  ON artifact.artifact_digest = generation_search.artifact_digest
 AND artifact.lifecycle_state = 'complete'
WHERE generation_search MATCH ?1 AND generation_id = ?2
  AND (?3 IS NULL OR artifact.language = ?3)
  AND (?4 IS NULL OR kind = ?4)
  AND (?5 IS NULL OR substr(repository_path, 1, length(?5)) = ?5)
  AND ((?6 = 'exact' AND name = ?7)
       OR (?6 = 'prefix' AND substr(name, 1, length(?7)) = ?7))
ORDER BY name ASC, repository_path ASC, fact_ordinal ASC
LIMIT ?8";

const PRIMARY_SYMBOL_COUNT_SQL: &str = "SELECT COUNT(*)
FROM generation_search
JOIN analysis_artifacts AS artifact
  ON artifact.artifact_digest = generation_search.artifact_digest
 AND artifact.lifecycle_state = 'complete'
WHERE generation_search MATCH ?1 AND generation_id = ?2
  AND (?3 IS NULL OR artifact.language = ?3)
  AND (?4 IS NULL OR kind = ?4)
  AND (?5 IS NULL OR substr(repository_path, 1, length(?5)) = ?5)
  AND ((?6 = 'exact' AND name = ?7)
       OR (?6 = 'prefix' AND substr(name, 1, length(?7)) = ?7))";

const REBUILD_SYMBOL_SEARCH_SQL: &str = "SELECT repository_path, fact_ordinal, kind, name,
       qualified_name, content_digest, generation_search_rebuild.artifact_digest,
       name_start, name_end, declaration_start, declaration_end, artifact.language,
       artifact.producer_manifest_digest
FROM generation_search_rebuild
JOIN analysis_artifacts AS artifact
  ON artifact.artifact_digest = generation_search_rebuild.artifact_digest
 AND artifact.lifecycle_state = 'complete'
WHERE generation_search_rebuild MATCH ?1 AND generation_id = ?2
  AND (?3 IS NULL OR artifact.language = ?3)
  AND (?4 IS NULL OR kind = ?4)
  AND (?5 IS NULL OR substr(repository_path, 1, length(?5)) = ?5)
  AND ((?6 = 'exact' AND name = ?7)
       OR (?6 = 'prefix' AND substr(name, 1, length(?7)) = ?7))
ORDER BY name ASC, repository_path ASC, fact_ordinal ASC
LIMIT ?8";

const REBUILD_SYMBOL_COUNT_SQL: &str = "SELECT COUNT(*)
FROM generation_search_rebuild
JOIN analysis_artifacts AS artifact
  ON artifact.artifact_digest = generation_search_rebuild.artifact_digest
 AND artifact.lifecycle_state = 'complete'
WHERE generation_search_rebuild MATCH ?1 AND generation_id = ?2
  AND (?3 IS NULL OR artifact.language = ?3)
  AND (?4 IS NULL OR kind = ?4)
  AND (?5 IS NULL OR substr(repository_path, 1, length(?5)) = ?5)
  AND ((?6 = 'exact' AND name = ?7)
       OR (?6 = 'prefix' AND substr(name, 1, length(?7)) = ?7))";

fn literal_fts_query(query: &str) -> Result<String, SqliteStoreError> {
    if query.is_empty() || query.len() > MAX_QUERY_BYTES {
        return Err(SqliteStoreError::InvalidSearchQuery);
    }
    let terms: Vec<&str> = query.split_whitespace().collect();
    if terms.is_empty() || terms.len() > MAX_QUERY_TERMS {
        return Err(SqliteStoreError::InvalidSearchQuery);
    }
    let mut output = String::with_capacity(query.len().saturating_mul(2).saturating_add(16));
    for (index, term) in terms.into_iter().enumerate() {
        if term.len() > MAX_TERM_BYTES {
            return Err(SqliteStoreError::InvalidSearchQuery);
        }
        if index != 0 {
            output.push(' ');
        }
        output.push('"');
        for character in term.chars() {
            if character == '"' {
                output.push('"');
            }
            output.push(character);
        }
        output.push('"');
    }
    Ok(output)
}

fn symbol_fts_query(
    name: &str,
    name_match: repowitness_application::SymbolSearchNameMatch,
) -> Result<String, SqliteStoreError> {
    if name.is_empty() || name.len() > MAX_SYMBOL_SEARCH_NAME_BYTES {
        return Err(SqliteStoreError::InvalidSearchQuery);
    }
    let mut output = String::with_capacity(name.len().saturating_mul(2).saturating_add(12));
    output.push_str("name : \"");
    for character in name.chars() {
        if character == '"' {
            output.push('"');
        }
        output.push(character);
    }
    output.push('"');
    if matches!(name_match, repowitness_application::SymbolSearchNameMatch::Prefix) {
        output.push('*');
    }
    Ok(output)
}

fn checked_output_bytes(
    current: u64,
    path: &RepositoryPath,
    language: SourceLanguage,
    kind: RustSymbolKind,
    name: &str,
    qualified_name: &str,
    limit: u64,
) -> Result<u64, SqliteStoreError> {
    let row_bytes = path
        .byte_count()
        .get()
        .checked_add(FIXED_SEARCH_HIT_OUTPUT_BYTES)
        .and_then(|value| {
            value.checked_add(u64::try_from(language.as_str().len()).unwrap_or(u64::MAX))
        })
        .and_then(|value| value.checked_add(u64::try_from(kind.as_str().len()).unwrap_or(u64::MAX)))
        .and_then(|value| value.checked_add(u64::try_from(name.len()).unwrap_or(u64::MAX)))
        .and_then(|value| {
            value.checked_add(u64::try_from(qualified_name.len()).unwrap_or(u64::MAX))
        })
        .ok_or(SqliteStoreError::CountNotRepresentable)?;
    let total = current
        .checked_add(row_bytes)
        .ok_or(SqliteStoreError::CountNotRepresentable)?;
    if total > limit {
        return Err(SqliteStoreError::SearchOutputLimitExceeded);
    }
    Ok(total)
}

fn persisted_span(start: i64, end: i64) -> Result<ByteSpan, SqliteStoreError> {
    let start = u64::try_from(start).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
    let end = u64::try_from(end).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
    ByteSpan::try_new(ByteOffset::new(start), ByteOffset::new(end))
        .map_err(|_| SqliteStoreError::IntegrityCheckFailed)
}

fn persisted_count(value: i64) -> Result<u64, SearchFailure> {
    u64::try_from(value).map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))
}

fn parse_symbol_kind(kind: &str) -> Option<RustSymbolKind> {
    RustSymbolKind::from_stable_str(kind)
}

fn check_control(cancelled: &AtomicBool, deadline: Instant) -> Result<(), SqliteStoreError> {
    if cancelled.load(Ordering::Acquire) {
        Err(SqliteStoreError::Cancelled)
    } else if Instant::now() >= deadline {
        Err(SqliteStoreError::DeadlineExceeded)
    } else {
        Ok(())
    }
}

fn is_interrupted(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(code, _) if code.code == ErrorCode::OperationInterrupted
    )
}

#[cfg(test)]
mod query_tests {
    use repowitness_application::{
        MAX_SYMBOL_SEARCH_NAME_BYTES, SymbolSearchNameMatch,
    };

    use super::symbol_fts_query;

    #[test]
    fn symbol_fts_query_keeps_the_public_selector_bound() {
        let maximum_name = "a".repeat(MAX_SYMBOL_SEARCH_NAME_BYTES);
        assert!(symbol_fts_query(&maximum_name, SymbolSearchNameMatch::Prefix).is_ok());
        let too_long_name = "a".repeat(MAX_SYMBOL_SEARCH_NAME_BYTES + 1);
        assert!(symbol_fts_query(&too_long_name, SymbolSearchNameMatch::Exact).is_err());
    }
}

fn receive_reply<T>(
    receiver: &Receiver<Result<T, SqliteStoreError>>,
    deadline: Instant,
) -> Result<T, SqliteStoreError> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(SqliteStoreError::DeadlineExceeded);
    }
    receiver
        .recv_timeout(remaining)
        .map_err(|error| match error {
            mpsc::RecvTimeoutError::Timeout => SqliteStoreError::ReplyTimeout,
            mpsc::RecvTimeoutError::Disconnected => SqliteStoreError::WorkerUnavailable,
        })?
}
