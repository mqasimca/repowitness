fn symbol_search_transaction(
    connection: &mut Connection,
    repository: RepositoryIdentityDigest,
    query: &SymbolSearchQuery,
    limits: SearchLimits,
) -> Result<SearchResults, SearchFailure> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Deferred)?;
    let state = active_search_state(&transaction, repository)?;
    let fts_query = symbol_fts_query(query.name(), query.name_match())
        .map_err(SearchFailure::Store)?;
    let language = query.language().map(SourceLanguage::as_str);
    let kind = query.kind().map(RustSymbolKind::as_str);
    let path_prefix = query.path_prefix().map(str::as_bytes);
    let name_match = query.name_match().as_str();
    let name = query.name();
    let total_matches = transaction.query_row(
        state.sql.symbol_count,
        params![
            fts_query,
            state.generation.get(),
            language,
            kind,
            path_prefix,
            name_match,
            name,
        ],
        |row| row.get::<_, i64>(0),
    )?;
    let total_matches = persisted_count(total_matches)?;
    let (hits, output_bytes) =
        symbol_search_hits(&transaction, state.sql.symbol_search, state.generation, query, limits)?;
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

fn workspace_symbol_search_transaction(
    connection: &mut Connection,
    view: &PinnedWorkspaceView,
    source_slot: repowitness_domain::SourceSlotId,
    query: &SymbolSearchQuery,
    limits: SearchLimits,
) -> Result<SearchResults, SearchFailure> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Deferred)?;
    let state = workspace_search_state(&transaction, view, source_slot)?;
    let fts_query = symbol_fts_query(query.name(), query.name_match())
        .map_err(SearchFailure::Store)?;
    let language = query.language().map(SourceLanguage::as_str);
    let kind = query.kind().map(RustSymbolKind::as_str);
    let path_prefix = query.path_prefix().map(str::as_bytes);
    let name_match = query.name_match().as_str();
    let name = query.name();
    let total_matches = transaction.query_row(
        state.sql.symbol_count,
        params![
            fts_query,
            state.generation.get(),
            language,
            kind,
            path_prefix,
            name_match,
            name,
        ],
        |row| row.get::<_, i64>(0),
    )?;
    let total_matches = persisted_count(total_matches)?;
    let (hits, output_bytes) =
        symbol_search_hits(&transaction, state.sql.symbol_search, state.generation, query, limits)?;
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

fn symbol_search_hits(
    transaction: &Transaction<'_>,
    sql: &str,
    generation: GenerationId,
    query: &SymbolSearchQuery,
    limits: SearchLimits,
) -> Result<(Box<[SearchHit]>, u64), SearchFailure> {
    let fts_query = symbol_fts_query(query.name(), query.name_match())
        .map_err(SearchFailure::Store)?;
    let language = query.language().map(SourceLanguage::as_str);
    let kind = query.kind().map(RustSymbolKind::as_str);
    let path_prefix = query.path_prefix().map(str::as_bytes);
    let name_match = query.name_match().as_str();
    let name = query.name();
    let mut statement = transaction.prepare(sql)?;
    let mut rows = statement.query(params![
        fts_query,
        generation.get(),
        language,
        kind,
        path_prefix,
        name_match,
        name,
        i64::from(limits.max_results()),
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
