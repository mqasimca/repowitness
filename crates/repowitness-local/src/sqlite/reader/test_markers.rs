/// Complete bounded repository-scoped raw test-marker answer.
#[derive(Eq, PartialEq)]
pub struct RawSyntaxTestMarkersReadResult {
    snapshot: SourceSnapshotDigest,
    generation: GenerationId,
    index_coverage: RustIndexCoverage,
    availability: RawSyntaxSiteProjectionAvailability,
    language_coverage: Box<[repowitness_application::TestMarkerLanguageCoverage]>,
    markers: Box<[RawSyntaxSiteRecord]>,
    total_markers: u64,
    output_bytes: u64,
}

impl RawSyntaxTestMarkersReadResult {
    #[must_use]
    pub const fn snapshot(&self) -> SourceSnapshotDigest {
        self.snapshot
    }

    #[must_use]
    pub const fn generation(&self) -> GenerationId {
        self.generation
    }

    #[must_use]
    pub const fn index_coverage(&self) -> RustIndexCoverage {
        self.index_coverage
    }

    #[must_use]
    pub const fn availability(&self) -> RawSyntaxSiteProjectionAvailability {
        self.availability
    }

    #[must_use]
    pub const fn language_coverage(&self) -> &[repowitness_application::TestMarkerLanguageCoverage] {
        &self.language_coverage
    }

    #[must_use]
    pub const fn markers(&self) -> &[RawSyntaxSiteRecord] {
        &self.markers
    }

    #[must_use]
    pub const fn total_markers(&self) -> u64 {
        self.total_markers
    }

    #[must_use]
    pub const fn output_bytes(&self) -> u64 {
        self.output_bytes
    }
}

impl fmt::Debug for RawSyntaxTestMarkersReadResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RawSyntaxTestMarkersReadResult")
            .field("snapshot", &self.snapshot)
            .field("generation", &self.generation)
            .field("availability", &self.availability)
            .field("returned_markers", &self.markers.len())
            .field("total_markers", &self.total_markers)
            .finish()
    }
}

impl OwnedSqliteReader {
    /// Reads only parser-attributed raw `test_marker` observations for the active generation.
    pub fn raw_syntax_test_markers(
        &self,
        repository: RepositoryIdentityDigest,
        query: TestMarkersQuery,
        limits: RawSyntaxSiteReadLimits,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<RawSyntaxTestMarkersReadResult, SqliteStoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(
            ReaderCommand::RawSyntaxTestMarkers(Box::new(RawSyntaxTestMarkersCommand {
                repository,
                query,
                limits,
                cancelled: Arc::clone(&cancelled),
                deadline,
                reply,
            })),
            deadline,
        )?;
        match receive_reply(&receiver, deadline) {
            Ok(result) => Ok(result),
            Err(error) => {
                cancelled.store(true, Ordering::Release);
                Err(error)
            }
        }
    }
}

impl repowitness_application::TestMarkersPort for OwnedSqliteReader {
    type Generation = GenerationId;
    type Error = SqliteStoreError;

    fn test_markers(
        &self,
        request: repowitness_application::TestMarkersPortRequest,
    ) -> Result<
        repowitness_application::TestMarkersPortResult<Self::Generation>,
        Self::Error,
    > {
        let limits = RawSyntaxSiteReadLimits::try_new(
            request.limits().max_results(),
            request.limits().max_output_bytes(),
        )?;
        let result = self.raw_syntax_test_markers(
            request.repository(),
            request.query().clone(),
            limits,
            request.cancelled(),
            request.deadline(),
        )?;
        let availability = match result.availability() {
            RawSyntaxSiteProjectionAvailability::Complete => {
                repowitness_application::TestMarkersAvailability::Complete
            }
            RawSyntaxSiteProjectionAvailability::NotProduced => {
                repowitness_application::TestMarkersAvailability::NotProduced
            }
        };
        let markers = result
            .markers()
            .iter()
            .map(|record| {
                repowitness_application::OutboundSyntaxSite::new(
                    record.path().clone(),
                    record.content_digest(),
                    record.artifact_digest(),
                    record.language(),
                    record.site().clone(),
                )
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Ok(repowitness_application::TestMarkersPortResult::new(
            result.snapshot(),
            result.generation(),
            result.index_coverage(),
            repowitness_application::TestMarkersPortPayload::new(
                availability,
                result.language_coverage().to_vec().into_boxed_slice(),
                markers,
                result.total_markers(),
                result.output_bytes(),
            ),
        ))
    }
}

fn execute_raw_syntax_test_markers_command(
    connection: &mut Connection,
    command: RawSyntaxTestMarkersCommand,
) {
    let RawSyntaxTestMarkersCommand {
        repository,
        query,
        limits,
        cancelled,
        deadline,
        reply,
    } = command;
    let result = raw_syntax_test_markers(
        connection,
        repository,
        &query,
        limits,
        cancelled,
        deadline,
    );
    let _ = reply.try_send(result);
}

fn raw_syntax_test_markers(
    connection: &mut Connection,
    repository: RepositoryIdentityDigest,
    query: &TestMarkersQuery,
    limits: RawSyntaxSiteReadLimits,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
) -> Result<RawSyntaxTestMarkersReadResult, SqliteStoreError> {
    check_control(&cancelled, deadline)?;
    let progress_source = Arc::clone(&cancelled);
    connection
        .progress_handler(
            PROGRESS_OPCODES,
            Some(move || progress_source.load(Ordering::Acquire) || Instant::now() >= deadline),
        )
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    let result = raw_syntax_test_markers_transaction(
        connection,
        repository,
        query,
        limits,
        &cancelled,
        deadline,
    );
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

fn raw_syntax_test_markers_transaction(
    connection: &mut Connection,
    repository: RepositoryIdentityDigest,
    query: &TestMarkersQuery,
    limits: RawSyntaxSiteReadLimits,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<RawSyntaxTestMarkersReadResult, SearchFailure> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Deferred)?;
    let state = active_generation_state(&transaction, repository)?;
    let availability = raw_syntax_site_availability(&transaction, state.generation)?;
    let (language_coverage, markers, total_markers, output_bytes) = match availability {
        RawSyntaxSiteProjectionAvailability::Complete => {
            let language_coverage = load_test_marker_language_coverage(
                &transaction,
                state.generation,
                query,
            )?;
            let (markers, total_markers, output_bytes) = load_raw_syntax_test_markers(
                &transaction,
                state.generation,
                query,
                limits,
                cancelled,
                deadline,
            )?;
            let emitted_markers = language_coverage.iter().try_fold(0_u64, |total, coverage| {
                total
                    .checked_add(coverage.emitted_markers())
                    .ok_or(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))
            })?;
            if emitted_markers != total_markers {
                return Err(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed));
            }
            (language_coverage, markers, total_markers, output_bytes)
        }
        RawSyntaxSiteProjectionAvailability::NotProduced => {
            (Vec::new().into_boxed_slice(), Vec::new().into_boxed_slice(), 0, 0)
        }
    };
    check_control(cancelled, deadline).map_err(SearchFailure::Store)?;
    transaction.commit()?;
    Ok(RawSyntaxTestMarkersReadResult {
        snapshot: state.snapshot,
        generation: state.generation,
        index_coverage: state.index_coverage,
        availability,
        language_coverage,
        markers,
        total_markers,
        output_bytes,
    })
}

fn load_test_marker_language_coverage(
    transaction: &Transaction<'_>,
    generation: GenerationId,
    query: &TestMarkersQuery,
) -> Result<Box<[repowitness_application::TestMarkerLanguageCoverage]>, SearchFailure> {
    let language = query.language().map(SourceLanguage::as_str);
    let path_prefix = query.path_prefix().map(str::as_bytes);
    let mut statement = transaction.prepare(
        "SELECT artifact.language,
                count(*),
                sum(CASE WHEN raw.test_marker_support = 'available' THEN 1 ELSE 0 END),
                sum(CASE WHEN raw.test_marker_support = 'unsupported' THEN 1 ELSE 0 END),
                sum(raw.test_marker_emitted)
         FROM generation_syntax_site_artifacts AS occurrence
         JOIN generation_files AS file
           ON file.generation_id = occurrence.generation_id
          AND file.repository_path = occurrence.repository_path
         JOIN analysis_artifacts AS artifact
           ON artifact.artifact_digest = occurrence.syntax_site_artifact_digest
          AND artifact.lifecycle_state = 'complete'
          AND artifact.source_content_digest = file.content_digest
         JOIN syntax_site_artifacts AS raw
           ON raw.artifact_digest = occurrence.syntax_site_artifact_digest
         WHERE occurrence.generation_id = ?1
           AND (?2 IS NULL OR artifact.language = ?2)
           AND (?3 IS NULL OR substr(file.repository_path, 1, length(?3)) = ?3)
         GROUP BY artifact.language
         ORDER BY CASE artifact.language
                    WHEN 'rust' THEN 0
                    WHEN 'go' THEN 1
                    WHEN 'typescript' THEN 2
                    WHEN 'tsx' THEN 3
                    WHEN 'python' THEN 4
                  END",
    )?;
    let mut rows = statement.query(params![generation.get(), language, path_prefix])?;
    let mut coverage = Vec::new();
    while let Some(row) = rows.next()? {
        let language: String = row.get(0)?;
        let language = SourceLanguage::from_stable_str(&language)
            .ok_or(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
        let indexed_files = persisted_count(row.get::<_, i64>(1)?)?;
        let supported_files = persisted_count(row.get::<_, i64>(2)?)?;
        let unsupported_files = persisted_count(row.get::<_, i64>(3)?)?;
        let emitted_markers = persisted_count(row.get::<_, i64>(4)?)?;
        coverage.push(repowitness_application::TestMarkerLanguageCoverage::new(
            language,
            indexed_files,
            supported_files,
            unsupported_files,
            emitted_markers,
        ));
    }
    Ok(coverage.into_boxed_slice())
}

fn load_raw_syntax_test_markers(
    transaction: &Transaction<'_>,
    generation: GenerationId,
    query: &TestMarkersQuery,
    limits: RawSyntaxSiteReadLimits,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(Box<[RawSyntaxSiteRecord]>, u64, u64), SearchFailure> {
    let language = query.language().map(SourceLanguage::as_str);
    let path_prefix = query.path_prefix().map(str::as_bytes);
    let total_markers = transaction.query_row(
        "SELECT count(*)
         FROM generation_syntax_site_artifacts AS occurrence
         JOIN generation_files AS file
           ON file.generation_id = occurrence.generation_id
          AND file.repository_path = occurrence.repository_path
         JOIN analysis_artifacts AS artifact
           ON artifact.artifact_digest = occurrence.syntax_site_artifact_digest
          AND artifact.lifecycle_state = 'complete'
          AND artifact.source_content_digest = file.content_digest
         JOIN syntax_sites AS site
           ON site.artifact_digest = occurrence.syntax_site_artifact_digest
         WHERE occurrence.generation_id = ?1
           AND site.site_kind = 'test_marker'
           AND (?2 IS NULL OR artifact.language = ?2)
           AND (?3 IS NULL OR substr(file.repository_path, 1, length(?3)) = ?3)",
        params![generation.get(), language, path_prefix],
        |row| row.get::<_, i64>(0),
    )?;
    let total_markers = persisted_count(total_markers)?;
    let mut statement = transaction.prepare(
        "SELECT file.repository_path, file.content_digest,
                occurrence.syntax_site_artifact_digest, artifact.language,
                site.ordinal, site.site_kind, site.extraction_evidence,
                site.occurrence_start, site.occurrence_end,
                site.target_start, site.target_end, site.raw_target
         FROM generation_syntax_site_artifacts AS occurrence
         JOIN generation_files AS file
           ON file.generation_id = occurrence.generation_id
          AND file.repository_path = occurrence.repository_path
         JOIN analysis_artifacts AS artifact
           ON artifact.artifact_digest = occurrence.syntax_site_artifact_digest
          AND artifact.lifecycle_state = 'complete'
          AND artifact.source_content_digest = file.content_digest
         JOIN syntax_sites AS site
           ON site.artifact_digest = occurrence.syntax_site_artifact_digest
         WHERE occurrence.generation_id = ?1
           AND site.site_kind = 'test_marker'
           AND (?2 IS NULL OR artifact.language = ?2)
           AND (?3 IS NULL OR substr(file.repository_path, 1, length(?3)) = ?3)
         ORDER BY file.repository_path, site.occurrence_start, site.occurrence_end,
                  site.target_start, site.target_end, site.ordinal
         LIMIT ?4",
    )?;
    let mut rows = statement.query(params![
        generation.get(),
        language,
        path_prefix,
        i64::from(limits.max_results()),
    ])?;
    let mut markers = Vec::new();
    let mut output_bytes = repowitness_application::FIXED_TEST_MARKER_OUTPUT_BYTES;
    while let Some(row) = rows.next()? {
        check_control(cancelled, deadline).map_err(SearchFailure::Store)?;
        let marker = decode_raw_syntax_test_marker(row)?;
        output_bytes = output_bytes
            .checked_add(
                repowitness_application::test_marker_record_output_bytes(
                    u64::try_from(marker.path().as_bytes().len())
                        .map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?,
                    u64::try_from(marker.site().raw_target().len())
                        .map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?,
                )
                .ok_or(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?,
            )
            .ok_or(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
        if output_bytes > limits.max_output_bytes() {
            return Err(SearchFailure::Store(
                SqliteStoreError::RawSyntaxSiteReadOutputLimitExceeded,
            ));
        }
        markers.push(marker);
    }
    Ok((markers.into_boxed_slice(), total_markers, output_bytes))
}

fn decode_raw_syntax_test_marker(
    row: &rusqlite::Row<'_>,
) -> Result<RawSyntaxSiteRecord, SearchFailure> {
    let path: Vec<u8> = row.get(0)?;
    let content_digest: Vec<u8> = row.get(1)?;
    let artifact_digest: Vec<u8> = row.get(2)?;
    let language: String = row.get(3)?;
    let ordinal: i64 = row.get(4)?;
    let kind: String = row.get(5)?;
    let evidence: String = row.get(6)?;
    let occurrence_start: i64 = row.get(7)?;
    let occurrence_end: i64 = row.get(8)?;
    let target_start: i64 = row.get(9)?;
    let target_end: i64 = row.get(10)?;
    let raw_target: String = row.get(11)?;
    let integrity = || SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed);
    let path = RepositoryPath::try_from_bytes(&path, PERSISTED_PATH_LIMITS).map_err(|_| integrity())?;
    let content_digest = SourceContentDigest::try_from_slice(&content_digest).map_err(|_| integrity())?;
    let artifact_digest = AnalysisArtifactDigest::try_from_slice(&artifact_digest).map_err(|_| integrity())?;
    let language = SourceLanguage::from_stable_str(&language).ok_or_else(integrity)?;
    let ordinal = u32::try_from(ordinal).map_err(|_| integrity())?;
    let kind = RawSyntaxSiteKind::from_stable_str(&kind).ok_or_else(integrity)?;
    if kind != RawSyntaxSiteKind::TestMarker {
        return Err(integrity());
    }
    let evidence = RawSyntaxSiteEvidence::from_stable_str(&evidence).ok_or_else(integrity)?;
    let site = RawSyntaxSite::try_new(
        RawSyntaxSiteOrdinal::new(ordinal),
        kind,
        evidence,
        raw_syntax_span(occurrence_start, occurrence_end)?,
        raw_syntax_span(target_start, target_end)?,
        raw_target,
        repowitness_analysis::RawSyntaxSiteAnalysisLimits::DEFAULT,
    ).map_err(|_| integrity())?;
    Ok(RawSyntaxSiteRecord {
        path,
        content_digest,
        artifact_digest,
        language,
        site,
    })
}
