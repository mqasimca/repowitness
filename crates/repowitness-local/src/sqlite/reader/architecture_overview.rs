// These cover the fixed JSON/MCP envelope, keys, bounded numeric values, and
// fixed-size digest receipts. Repository paths are accounted for separately as
// canonical `rwp1:h:` text, which expands every source byte to two hex bytes.
const FIXED_ARCHITECTURE_OVERVIEW_OUTPUT_BYTES: u64 = 2_048;
const FIXED_ARCHITECTURE_OVERVIEW_LANGUAGE_OUTPUT_BYTES: u64 = 128;
const FIXED_ARCHITECTURE_OVERVIEW_KIND_OUTPUT_BYTES: u64 = 128;
const FIXED_ARCHITECTURE_OVERVIEW_ROOT_OUTPUT_BYTES: u64 = 128;
const FIXED_ARCHITECTURE_OVERVIEW_CANDIDATE_OUTPUT_BYTES: u64 = 768;
const FIXED_ARCHITECTURE_OVERVIEW_FILE_OUTPUT_BYTES: u64 = 512;
const PATH_TEXT_PREFIX_BYTES: u64 = 7;

fn execute_architecture_overview_command(
    connection: &mut Connection,
    command: ArchitectureOverviewCommand,
) {
    let ArchitectureOverviewCommand {
        repository,
        limits,
        cancelled,
        deadline,
        reply,
    } = command;
    let result = read_active_architecture_overview(connection, repository, limits, cancelled, deadline);
    let _ = reply.try_send(result);
}

fn read_active_architecture_overview(
    connection: &mut Connection,
    repository: RepositoryIdentityDigest,
    limits: ArchitectureOverviewLimits,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
) -> Result<ArchitectureOverviewPortResult<GenerationId>, SqliteStoreError> {
    check_control(&cancelled, deadline)?;
    let progress_cancelled = Arc::clone(&cancelled);
    connection
        .progress_handler(
            PROGRESS_OPCODES,
            Some(move || progress_cancelled.load(Ordering::Acquire) || Instant::now() >= deadline),
        )
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    let result = architecture_overview_transaction(connection, repository, limits);
    connection
        .progress_handler(0, None::<fn() -> bool>)
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
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

fn architecture_overview_transaction(
    connection: &mut Connection,
    repository: RepositoryIdentityDigest,
    limits: ArchitectureOverviewLimits,
) -> Result<ArchitectureOverviewPortResult<GenerationId>, SearchFailure> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Deferred)?;
    let state = active_search_state(&transaction, repository)?;
    let (total_files, total_declarations) = architecture_map_totals(&transaction, state.generation)?;
    let language_summaries = architecture_map_language_summaries(&transaction, state.generation)?;
    let kind_summaries = architecture_overview_kind_summaries(&transaction, state.generation)?;
    let mut output_bytes = architecture_overview_fixed_output_bytes(
        &language_summaries,
        &kind_summaries,
        limits.max_output_bytes(),
    )?;
    let (total_source_roots, source_roots) = architecture_overview_source_roots(
        &transaction,
        state.generation,
        limits,
        &mut output_bytes,
    )?;
    let (total_entry_point_candidates, entry_point_candidates) =
        architecture_overview_entry_point_candidates(
            &transaction,
            state,
            limits,
            &mut output_bytes,
        )?;
    let files = architecture_overview_files(
        &transaction,
        state.generation,
        limits,
        &mut output_bytes,
    )?;
    transaction.commit()?;
    Ok(ArchitectureOverviewPortResult::new(
        state.snapshot,
        state.generation,
        state.producer_manifest,
        state.index_coverage,
        language_summaries,
        kind_summaries,
        source_roots,
        entry_point_candidates,
        files,
        total_files,
        total_declarations,
        total_source_roots,
        total_entry_point_candidates,
        output_bytes,
    ))
}

fn architecture_overview_kind_summaries(
    transaction: &Transaction<'_>,
    generation: GenerationId,
) -> Result<Vec<ArchitectureOverviewKindSummary>, SearchFailure> {
    let mut statement = transaction.prepare(
        "SELECT artifact.language, fact.kind, COUNT(*)
         FROM generation_files AS file
         JOIN analysis_artifacts AS artifact
           ON artifact.artifact_digest = file.artifact_digest
          AND artifact.source_content_digest = file.content_digest
          AND artifact.lifecycle_state = 'complete'
         JOIN artifact_facts AS fact
           ON fact.artifact_digest = artifact.artifact_digest
         WHERE file.generation_id = ?1
         GROUP BY artifact.language, fact.kind
         ORDER BY artifact.language ASC, fact.kind ASC",
    )?;
    let mut rows = statement.query([generation.get()])?;
    let mut summaries = Vec::new();
    while let Some(row) = rows.next()? {
        let language: String = row.get(0)?;
        let kind: String = row.get(1)?;
        let declarations: i64 = row.get(2)?;
        let language = SourceLanguage::from_stable_str(&language)
            .ok_or(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
        let kind = RustSymbolKind::from_stable_str(&kind)
            .ok_or(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
        summaries.push(ArchitectureOverviewKindSummary::new(
            language,
            kind,
            persisted_count(declarations)?,
        ));
    }
    Ok(summaries)
}

fn architecture_overview_source_roots(
    transaction: &Transaction<'_>,
    generation: GenerationId,
    limits: ArchitectureOverviewLimits,
    output_bytes: &mut u64,
) -> Result<(u64, Vec<ArchitectureOverviewSourceRootSummary>), SearchFailure> {
    let total: i64 = transaction.query_row(
        "SELECT COUNT(*) FROM (
             SELECT CASE WHEN instr(file.repository_path, X'2f') = 0 THEN 0 ELSE 1 END,
                    CASE WHEN instr(file.repository_path, X'2f') = 0 THEN NULL
                         ELSE substr(file.repository_path, 1, instr(file.repository_path, X'2f') - 1)
                    END
             FROM generation_files AS file
             JOIN analysis_artifacts AS artifact
               ON artifact.artifact_digest = file.artifact_digest
              AND artifact.source_content_digest = file.content_digest
              AND artifact.lifecycle_state = 'complete'
             WHERE file.generation_id = ?1
             GROUP BY 1, 2
         )",
        [generation.get()],
        |row| row.get(0),
    )?;
    let total = persisted_count(total)?;
    let mut statement = transaction.prepare(
        "SELECT CASE WHEN instr(file.repository_path, X'2f') = 0 THEN 0 ELSE 1 END,
                CASE WHEN instr(file.repository_path, X'2f') = 0 THEN NULL
                     ELSE substr(file.repository_path, 1, instr(file.repository_path, X'2f') - 1)
                END,
                COUNT(*), COALESCE(SUM(artifact.fact_count), 0)
         FROM generation_files AS file
         JOIN analysis_artifacts AS artifact
           ON artifact.artifact_digest = file.artifact_digest
          AND artifact.source_content_digest = file.content_digest
          AND artifact.lifecycle_state = 'complete'
         WHERE file.generation_id = ?1
         GROUP BY 1, 2
         ORDER BY 1 ASC, 2 ASC
         LIMIT ?2",
    )?;
    let mut rows = statement.query(params![generation.get(), i64::from(limits.max_roots())])?;
    let mut roots = Vec::with_capacity(usize::from(limits.max_roots()));
    while let Some(row) = rows.next()? {
        let root_tag: i64 = row.get(0)?;
        let component: Option<Vec<u8>> = row.get(1)?;
        let files: i64 = row.get(2)?;
        let declarations: i64 = row.get(3)?;
        let root = match (root_tag, component) {
            (0, None) => ArchitectureOverviewSourceRoot::repository_root(),
            (1, Some(component)) => ArchitectureOverviewSourceRoot::top_level_directory(
                RepositoryPath::try_from_bytes(&component, PERSISTED_PATH_LIMITS)
                    .map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?,
            ),
            _ => return Err(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed)),
        };
        let summary = ArchitectureOverviewSourceRootSummary::new(
            root,
            persisted_count(files)?,
            persisted_count(declarations)?,
        );
        let next = architecture_overview_root_output_bytes(*output_bytes, &summary)?;
        if next > limits.max_output_bytes() {
            break;
        }
        *output_bytes = next;
        roots.push(summary);
    }
    Ok((total, roots))
}

fn architecture_overview_entry_point_candidates(
    transaction: &Transaction<'_>,
    state: ActiveSearchState,
    limits: ArchitectureOverviewLimits,
    output_bytes: &mut u64,
) -> Result<(u64, Vec<ArchitectureOverviewEntryPointCandidate>), SearchFailure> {
    let fts_query = symbol_fts_query("main", SymbolSearchNameMatch::Exact)
        .map_err(SearchFailure::Store)?;
    let kind = RustSymbolKind::Function.as_str();
    let total: i64 = transaction.query_row(
        state.sql.symbol_count,
        params![
            fts_query,
            state.generation.get(),
            Option::<&str>::None,
            kind,
            Option::<&[u8]>::None,
            SymbolSearchNameMatch::Exact.as_str(),
            "main",
        ],
        |row| row.get(0),
    )?;
    let total = persisted_count(total)?;
    let mut statement = transaction.prepare(state.sql.symbol_search)?;
    let mut rows = statement.query(params![
        fts_query,
        state.generation.get(),
        Option::<&str>::None,
        kind,
        Option::<&[u8]>::None,
        SymbolSearchNameMatch::Exact.as_str(),
        "main",
        i64::from(limits.max_entry_point_candidates()),
    ])?;
    let mut candidates = Vec::with_capacity(usize::from(limits.max_entry_point_candidates()));
    while let Some(row) = rows.next()? {
        let hit = read_search_hit(row)?;
        let candidate = architecture_overview_entry_point_candidate(hit)?;
        let next = architecture_overview_candidate_output_bytes(*output_bytes, &candidate)?;
        if next > limits.max_output_bytes() {
            break;
        }
        *output_bytes = next;
        candidates.push(candidate);
    }
    Ok((total, candidates))
}

fn architecture_overview_entry_point_candidate(
    hit: SearchHit,
) -> Result<ArchitectureOverviewEntryPointCandidate, SearchFailure> {
    let occurrence = RustSymbolOccurrence::try_new(
        hit.fact_ordinal,
        SourceArtifactEvidence::new(hit.artifact_digest, hit.producer_manifest),
        hit.kind,
        hit.name,
        hit.qualified_name,
        hit.name_span,
        hit.declaration_span,
    )
    .map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?
    .with_language(hit.language);
    Ok(ArchitectureOverviewEntryPointCandidate::new(
        hit.path,
        hit.content_digest,
        occurrence,
    ))
}

fn architecture_overview_files(
    transaction: &Transaction<'_>,
    generation: GenerationId,
    limits: ArchitectureOverviewLimits,
    output_bytes: &mut u64,
) -> Result<Vec<ArchitectureMapFile>, SearchFailure> {
    let mut statement = transaction.prepare(
        "SELECT file.repository_path, file.content_digest, file.artifact_digest,
                artifact.producer_manifest_digest, artifact.language, artifact.fact_count
         FROM generation_files AS file
         JOIN analysis_artifacts AS artifact
           ON artifact.artifact_digest = file.artifact_digest
          AND artifact.source_content_digest = file.content_digest
          AND artifact.lifecycle_state = 'complete'
         WHERE file.generation_id = ?1
         ORDER BY file.repository_path ASC
         LIMIT ?2",
    )?;
    let mut rows = statement.query(params![generation.get(), i64::from(limits.max_files())])?;
    let mut files = Vec::with_capacity(usize::from(limits.max_files()));
    while let Some(row) = rows.next()? {
        let file = read_architecture_map_file(row)?;
        let next = architecture_overview_file_output_bytes(*output_bytes, &file)?;
        if next > limits.max_output_bytes() {
            break;
        }
        *output_bytes = next;
        files.push(file);
    }
    Ok(files)
}

fn architecture_overview_fixed_output_bytes(
    languages: &[ArchitectureMapLanguageSummary],
    kinds: &[ArchitectureOverviewKindSummary],
    maximum: u64,
) -> Result<u64, SearchFailure> {
    let mut output = FIXED_ARCHITECTURE_OVERVIEW_OUTPUT_BYTES;
    for summary in languages {
        output = checked_architecture_overview_output_bytes(
            output,
            FIXED_ARCHITECTURE_OVERVIEW_LANGUAGE_OUTPUT_BYTES,
            architecture_overview_usize_output_bytes(summary.language().as_str().len())?,
        )?;
    }
    for summary in kinds {
        output = checked_architecture_overview_output_bytes(
            output,
            FIXED_ARCHITECTURE_OVERVIEW_KIND_OUTPUT_BYTES,
            architecture_overview_usize_output_bytes(summary.language().as_str().len())?
                .checked_add(architecture_overview_usize_output_bytes(
                    summary.kind().as_str().len(),
                )?)
                .ok_or(SearchFailure::Store(SqliteStoreError::CountNotRepresentable))?,
        )?;
    }
    if output > maximum {
        return Err(SearchFailure::Store(SqliteStoreError::CountNotRepresentable));
    }
    Ok(output)
}

fn architecture_overview_root_output_bytes(
    current: u64,
    summary: &ArchitectureOverviewSourceRootSummary,
) -> Result<u64, SearchFailure> {
    let path_bytes = match summary.root() {
        ArchitectureOverviewSourceRoot::RepositoryRoot => 0,
        ArchitectureOverviewSourceRoot::TopLevelDirectory(path) => {
            architecture_overview_encoded_path_output_bytes(path)?
        }
    };
    checked_architecture_overview_output_bytes(
        current,
        FIXED_ARCHITECTURE_OVERVIEW_ROOT_OUTPUT_BYTES,
        path_bytes,
    )
}

fn architecture_overview_candidate_output_bytes(
    current: u64,
    candidate: &ArchitectureOverviewEntryPointCandidate,
) -> Result<u64, SearchFailure> {
    let occurrence = candidate.occurrence();
    let variable = architecture_overview_encoded_path_output_bytes(candidate.path())?
        .checked_add(architecture_overview_usize_output_bytes(
            occurrence.language().as_str().len(),
        )?)
        .and_then(|bytes| {
            bytes.checked_add(architecture_overview_usize_output_bytes(
                occurrence.kind().as_str().len(),
            ).ok()?)
        })
        .and_then(|bytes| {
            bytes.checked_add(architecture_overview_usize_output_bytes(occurrence.name().len()).ok()?)
        })
        .and_then(|bytes| {
            bytes.checked_add(
                architecture_overview_json_string_content_bytes(occurrence.qualified_name())
                    .ok()?,
            )
        })
        .ok_or(SearchFailure::Store(SqliteStoreError::CountNotRepresentable))?;
    checked_architecture_overview_output_bytes(
        current,
        FIXED_ARCHITECTURE_OVERVIEW_CANDIDATE_OUTPUT_BYTES,
        variable,
    )
}

fn architecture_overview_file_output_bytes(
    current: u64,
    file: &ArchitectureMapFile,
) -> Result<u64, SearchFailure> {
    let variable = architecture_overview_encoded_path_output_bytes(file.path())?
        .checked_add(architecture_overview_usize_output_bytes(
            file.language().as_str().len(),
        )?)
        .ok_or(SearchFailure::Store(SqliteStoreError::CountNotRepresentable))?;
    checked_architecture_overview_output_bytes(
        current,
        FIXED_ARCHITECTURE_OVERVIEW_FILE_OUTPUT_BYTES,
        variable,
    )
}

fn architecture_overview_encoded_path_output_bytes(
    path: &RepositoryPath,
) -> Result<u64, SearchFailure> {
    path.byte_count()
        .get()
        .checked_mul(2)
        .and_then(|bytes| bytes.checked_add(PATH_TEXT_PREFIX_BYTES))
        .ok_or(SearchFailure::Store(SqliteStoreError::CountNotRepresentable))
}

fn architecture_overview_json_string_content_bytes(value: &str) -> Result<u64, SearchFailure> {
    value.bytes().try_fold(0_u64, |total, byte| {
        total
            .checked_add(if matches!(byte, b'"' | b'\\') { 2 } else { 1 })
            .ok_or(SearchFailure::Store(SqliteStoreError::CountNotRepresentable))
    })
}

fn checked_architecture_overview_output_bytes(
    current: u64,
    fixed: u64,
    variable: u64,
) -> Result<u64, SearchFailure> {
    current
        .checked_add(fixed)
        .and_then(|bytes| bytes.checked_add(variable))
        .ok_or(SearchFailure::Store(SqliteStoreError::CountNotRepresentable))
}

fn architecture_overview_usize_output_bytes(value: usize) -> Result<u64, SearchFailure> {
    u64::try_from(value).map_err(|_| SearchFailure::Store(SqliteStoreError::CountNotRepresentable))
}

#[cfg(test)]
mod architecture_overview_output_tests {
    use super::{
        PERSISTED_PATH_LIMITS, architecture_overview_encoded_path_output_bytes,
        architecture_overview_json_string_content_bytes,
    };
    use repowitness_domain::RepositoryPath;

    #[test]
    fn encoded_path_budget_includes_the_canonical_hex_expansion() {
        let value = format!("{}leaf.rs", "nested/".repeat(32));
        let path = RepositoryPath::try_from_bytes(value.as_bytes(), PERSISTED_PATH_LIMITS)
            .expect("fixture path should be valid");
        assert_eq!(
            architecture_overview_encoded_path_output_bytes(&path)
                .unwrap_or_else(|_| panic!("fixture path budget should fit")),
            7 + (2 * path.byte_count().get())
        );
    }

    #[test]
    fn escaped_qualified_name_budget_covers_json_expansion() {
        let qualified_name = "module::\"quoted\"\\member";
        assert_eq!(
            architecture_overview_json_string_content_bytes(qualified_name)
                .unwrap_or_else(|_| panic!("fixture qualified name budget should fit")),
            u64::try_from(qualified_name.len()).expect("fixture length should fit") + 3
        );
    }
}
