const MAX_RAW_SYNTAX_SITE_RESULTS: u16 = 1_000;
const MAX_RAW_SYNTAX_SITE_OUTPUT_BYTES: u64 = 4 * 1024 * 1024;
const FIXED_RAW_SYNTAX_SITE_OUTPUT_BYTES: u64 = 176;

/// Bounded read limits for immutable raw syntax observations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RawSyntaxSiteReadLimits {
    max_results: u16,
    max_output_bytes: u64,
}

impl RawSyntaxSiteReadLimits {
    /// Validates read limits against the compiled raw-site profile ceilings.
    pub const fn try_new(max_results: u16, max_output_bytes: u64) -> Result<Self, SqliteStoreError> {
        if max_results == 0
            || max_results > MAX_RAW_SYNTAX_SITE_RESULTS
            || max_output_bytes == 0
            || max_output_bytes > MAX_RAW_SYNTAX_SITE_OUTPUT_BYTES
        {
            return Err(SqliteStoreError::InvalidRawSyntaxSiteReadLimits);
        }
        Ok(Self {
            max_results,
            max_output_bytes,
        })
    }

    /// Returns the maximum retained exact site records.
    #[must_use]
    pub const fn max_results(self) -> u16 {
        self.max_results
    }

    /// Returns the maximum conservative encoded result bytes.
    #[must_use]
    pub const fn max_output_bytes(self) -> u64 {
        self.max_output_bytes
    }
}

impl Default for RawSyntaxSiteReadLimits {
    fn default() -> Self {
        Self {
            max_results: 100,
            max_output_bytes: 512 * 1024,
        }
    }
}

/// Categorical status of the generation-local raw syntax-site projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RawSyntaxSiteProjectionAvailability {
    /// The immutable projection is complete for the active generation.
    Complete,
    /// This generation predates the projection or did not publish it.
    NotProduced,
}

/// One exact unresolved immutable syntax observation.
#[derive(Clone, Eq, PartialEq)]
pub struct RawSyntaxSiteRecord {
    path: RepositoryPath,
    content_digest: SourceContentDigest,
    artifact_digest: AnalysisArtifactDigest,
    language: SourceLanguage,
    site: RawSyntaxSite,
}

impl RawSyntaxSiteRecord {
    /// Returns the exact repository-relative source path.
    #[must_use]
    pub const fn path(&self) -> &RepositoryPath {
        &self.path
    }

    /// Returns the exact source-content identity.
    #[must_use]
    pub const fn content_digest(&self) -> SourceContentDigest {
        self.content_digest
    }

    /// Returns the raw-site artifact identity, never a target identity.
    #[must_use]
    pub const fn artifact_digest(&self) -> AnalysisArtifactDigest {
        self.artifact_digest
    }

    /// Returns the parser language/dialect that produced the observation.
    #[must_use]
    pub const fn language(&self) -> SourceLanguage {
        self.language
    }

    /// Returns the exact unresolved raw syntax observation.
    #[must_use]
    pub const fn site(&self) -> &RawSyntaxSite {
        &self.site
    }
}

impl fmt::Debug for RawSyntaxSiteRecord {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RawSyntaxSiteRecord")
            .field("path", &self.path)
            .field("content_digest", &self.content_digest)
            .field("artifact_digest", &self.artifact_digest)
            .field("language", &self.language)
            .field("site", &self.site)
            .finish()
    }
}

/// Complete bounded answer for one exact declaration selector.
#[derive(Eq, PartialEq)]
pub struct RawSyntaxSitesReadResult {
    snapshot: SourceSnapshotDigest,
    generation: GenerationId,
    index_coverage: RustIndexCoverage,
    declaration: Option<SearchHit>,
    availability: RawSyntaxSiteProjectionAvailability,
    sites: Box<[RawSyntaxSiteRecord]>,
    total_sites: u64,
    output_bytes: u64,
}

impl RawSyntaxSitesReadResult {
    /// Returns the active immutable snapshot used by the complete read.
    #[must_use]
    pub const fn snapshot(&self) -> SourceSnapshotDigest {
        self.snapshot
    }

    /// Returns the active immutable generation used by the complete read.
    #[must_use]
    pub const fn generation(&self) -> GenerationId {
        self.generation
    }

    /// Returns source-index coverage recorded before activation.
    #[must_use]
    pub const fn index_coverage(&self) -> RustIndexCoverage {
        self.index_coverage
    }

    /// Returns the selected declaration when it still exists in the pinned generation.
    #[must_use]
    pub const fn declaration(&self) -> Option<&SearchHit> {
        self.declaration.as_ref()
    }

    /// Returns whether raw syntax sites were produced for this generation.
    #[must_use]
    pub const fn availability(&self) -> RawSyntaxSiteProjectionAvailability {
        self.availability
    }

    /// Returns retained observations in deterministic source order.
    #[must_use]
    pub const fn sites(&self) -> &[RawSyntaxSiteRecord] {
        &self.sites
    }

    /// Returns the exact count before the explicit output limit.
    #[must_use]
    pub const fn total_sites(&self) -> u64 {
        self.total_sites
    }

    /// Returns conservative retained-output bytes.
    #[must_use]
    pub const fn output_bytes(&self) -> u64 {
        self.output_bytes
    }
}

impl fmt::Debug for RawSyntaxSitesReadResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RawSyntaxSitesReadResult")
            .field("snapshot", &self.snapshot)
            .field("generation", &self.generation)
            .field("declaration_present", &self.declaration.is_some())
            .field("availability", &self.availability)
            .field("site_count", &self.sites.len())
            .field("total_sites", &self.total_sites)
            .finish()
    }
}

/// Complete bounded answer for one exact raw target across the active generation.
#[derive(Eq, PartialEq)]
pub struct RawSyntaxSiteSearchReadResult {
    snapshot: SourceSnapshotDigest,
    generation: GenerationId,
    index_coverage: RustIndexCoverage,
    availability: RawSyntaxSiteProjectionAvailability,
    sites: Box<[RawSyntaxSiteRecord]>,
    total_sites: u64,
    output_bytes: u64,
}

impl RawSyntaxSiteSearchReadResult {
    /// Returns the active immutable snapshot used by the complete read.
    #[must_use]
    pub const fn snapshot(&self) -> SourceSnapshotDigest {
        self.snapshot
    }

    /// Returns the active immutable generation used by the complete read.
    #[must_use]
    pub const fn generation(&self) -> GenerationId {
        self.generation
    }

    /// Returns source-index coverage recorded before activation.
    #[must_use]
    pub const fn index_coverage(&self) -> RustIndexCoverage {
        self.index_coverage
    }

    /// Returns whether raw syntax sites were produced for this generation.
    #[must_use]
    pub const fn availability(&self) -> RawSyntaxSiteProjectionAvailability {
        self.availability
    }

    /// Returns exact matching observations in canonical path and source order.
    #[must_use]
    pub const fn sites(&self) -> &[RawSyntaxSiteRecord] {
        &self.sites
    }

    /// Returns the exact count before the explicit output limit.
    #[must_use]
    pub const fn total_sites(&self) -> u64 {
        self.total_sites
    }

    /// Returns conservative retained-output bytes.
    #[must_use]
    pub const fn output_bytes(&self) -> u64 {
        self.output_bytes
    }
}

impl fmt::Debug for RawSyntaxSiteSearchReadResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RawSyntaxSiteSearchReadResult")
            .field("snapshot", &self.snapshot)
            .field("generation", &self.generation)
            .field("availability", &self.availability)
            .field("site_count", &self.sites.len())
            .field("total_sites", &self.total_sites)
            .finish()
    }
}

impl OwnedSqliteReader {
    /// Reads only raw unresolved syntax observations physically contained in one exact declaration.
    #[allow(
        clippy::too_many_arguments,
        reason = "the immutable context, selector, bounds, cancellation, and deadline are all trust inputs"
    )]
    pub fn raw_syntax_sites_for_symbol(
        &self,
        repository: RepositoryIdentityDigest,
        expected_snapshot: SourceSnapshotDigest,
        expected_generation: GenerationId,
        selector: SymbolGetSelector,
        limits: RawSyntaxSiteReadLimits,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<RawSyntaxSitesReadResult, SqliteStoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(
            ReaderCommand::RawSyntaxSites(Box::new(RawSyntaxSitesCommand {
                repository,
                expected_snapshot,
                expected_generation,
                selector,
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

    /// Searches immutable all-language raw observations by exact target spelling.
    pub fn search_raw_syntax_sites(
        &self,
        repository: RepositoryIdentityDigest,
        target: String,
        limits: RawSyntaxSiteReadLimits,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<RawSyntaxSiteSearchReadResult, SqliteStoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(
            ReaderCommand::RawSyntaxSiteSearch(Box::new(RawSyntaxSiteSearchCommand {
                repository,
                target,
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

impl repowitness_application::OutboundSitesPort for OwnedSqliteReader {
    type Generation = GenerationId;
    type Error = SqliteStoreError;

    fn outbound_sites(
        &self,
        request: repowitness_application::OutboundSitesPortRequest<Self::Generation>,
    ) -> Result<
        repowitness_application::OutboundSitesPortResult<Self::Generation>,
        Self::Error,
    > {
        let limits = RawSyntaxSiteReadLimits::try_new(
            request.limits().max_results(),
            request.limits().max_output_bytes(),
        )?;
        let result = self.raw_syntax_sites_for_symbol(
            request.repository(),
            request.expected_snapshot(),
            *request.expected_generation(),
            request.selector().clone(),
            limits,
            request.cancelled(),
            request.deadline(),
        )?;
        let declaration = result.declaration().map(|declaration| {
            repowitness_application::OutboundSitesDeclaration::new(
                declaration.language(),
                declaration.declaration_span(),
            )
        });
        let availability = match result.availability() {
            RawSyntaxSiteProjectionAvailability::Complete => {
                repowitness_application::OutboundSitesAvailability::Complete
            }
            RawSyntaxSiteProjectionAvailability::NotProduced => {
                repowitness_application::OutboundSitesAvailability::NotProduced
            }
        };
        let sites = result
            .sites()
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
        Ok(repowitness_application::OutboundSitesPortResult::new(
            result.snapshot(),
            result.generation(),
            result.index_coverage(),
            declaration,
            availability,
            sites,
            result.total_sites(),
            result.output_bytes(),
        ))
    }
}

impl SyntaxSiteSearchPort for OwnedSqliteReader {
    type Generation = GenerationId;
    type Error = SqliteStoreError;

    fn syntax_site_search(
        &self,
        request: SyntaxSiteSearchPortRequest,
    ) -> Result<SyntaxSiteSearchPortResult<Self::Generation>, Self::Error> {
        let limits = RawSyntaxSiteReadLimits::try_new(
            request.limits().max_results(),
            request.limits().max_output_bytes(),
        )?;
        let result = self.search_raw_syntax_sites(
            request.repository(),
            request.query().as_str().to_owned(),
            limits,
            request.cancelled(),
            request.deadline(),
        )?;
        let availability = match result.availability() {
            RawSyntaxSiteProjectionAvailability::Complete => {
                repowitness_application::OutboundSitesAvailability::Complete
            }
            RawSyntaxSiteProjectionAvailability::NotProduced => {
                repowitness_application::OutboundSitesAvailability::NotProduced
            }
        };
        let sites = result
            .sites()
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
        Ok(SyntaxSiteSearchPortResult::new(
            result.snapshot(),
            result.generation(),
            result.index_coverage(),
            availability,
            sites,
            result.total_sites(),
            result.output_bytes(),
        ))
    }
}

fn execute_raw_syntax_sites_command(connection: &mut Connection, command: RawSyntaxSitesCommand) {
    let RawSyntaxSitesCommand {
        repository,
        expected_snapshot,
        expected_generation,
        selector,
        limits,
        cancelled,
        deadline,
        reply,
    } = command;
    let result = raw_syntax_sites_for_symbol(
        connection,
        repository,
        expected_snapshot,
        expected_generation,
        &selector,
        limits,
        cancelled,
        deadline,
    );
    let _ = reply.try_send(result);
}

fn execute_raw_syntax_site_search_command(
    connection: &mut Connection,
    command: RawSyntaxSiteSearchCommand,
) {
    let RawSyntaxSiteSearchCommand {
        repository,
        target,
        limits,
        cancelled,
        deadline,
        reply,
    } = command;
    let result = search_raw_syntax_sites(
        connection,
        repository,
        &target,
        limits,
        cancelled,
        deadline,
    );
    let _ = reply.try_send(result);
}

fn search_raw_syntax_sites(
    connection: &mut Connection,
    repository: RepositoryIdentityDigest,
    target: &str,
    limits: RawSyntaxSiteReadLimits,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
) -> Result<RawSyntaxSiteSearchReadResult, SqliteStoreError> {
    check_control(&cancelled, deadline)?;
    let progress_source = Arc::clone(&cancelled);
    connection
        .progress_handler(
            PROGRESS_OPCODES,
            Some(move || progress_source.load(Ordering::Acquire) || Instant::now() >= deadline),
        )
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    let result = raw_syntax_site_search_transaction(
        connection, repository, target, limits, &cancelled, deadline,
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

fn raw_syntax_site_search_transaction(
    connection: &mut Connection,
    repository: RepositoryIdentityDigest,
    target: &str,
    limits: RawSyntaxSiteReadLimits,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<RawSyntaxSiteSearchReadResult, SearchFailure> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Deferred)?;
    let state = active_generation_state(&transaction, repository)?;
    let availability = raw_syntax_site_availability(&transaction, state.generation)?;
    let (sites, total_sites, output_bytes) = match availability {
        RawSyntaxSiteProjectionAvailability::Complete => load_raw_syntax_site_matches(
            &transaction,
            state.generation,
            target,
            limits,
            cancelled,
            deadline,
        )?,
        RawSyntaxSiteProjectionAvailability::NotProduced => {
            (Vec::new().into_boxed_slice(), 0, 0)
        }
    };
    check_control(cancelled, deadline).map_err(SearchFailure::Store)?;
    transaction.commit()?;
    Ok(RawSyntaxSiteSearchReadResult {
        snapshot: state.snapshot,
        generation: state.generation,
        index_coverage: state.index_coverage,
        availability,
        sites,
        total_sites,
        output_bytes,
    })
}

#[allow(
    clippy::too_many_arguments,
    reason = "all immutable selector and control values are validated at the SQLite boundary"
)]
fn raw_syntax_sites_for_symbol(
    connection: &mut Connection,
    repository: RepositoryIdentityDigest,
    expected_snapshot: SourceSnapshotDigest,
    expected_generation: GenerationId,
    selector: &SymbolGetSelector,
    limits: RawSyntaxSiteReadLimits,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
) -> Result<RawSyntaxSitesReadResult, SqliteStoreError> {
    check_control(&cancelled, deadline)?;
    let progress_source = Arc::clone(&cancelled);
    connection
        .progress_handler(
            PROGRESS_OPCODES,
            Some(move || progress_source.load(Ordering::Acquire) || Instant::now() >= deadline),
        )
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    let result = raw_syntax_sites_transaction(
        connection,
        repository,
        expected_snapshot,
        expected_generation,
        selector,
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

#[allow(
    clippy::too_many_arguments,
    reason = "the transaction validates exact active context before touching the raw projection"
)]
fn raw_syntax_sites_transaction(
    connection: &mut Connection,
    repository: RepositoryIdentityDigest,
    expected_snapshot: SourceSnapshotDigest,
    expected_generation: GenerationId,
    selector: &SymbolGetSelector,
    limits: RawSyntaxSiteReadLimits,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<RawSyntaxSitesReadResult, SearchFailure> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Deferred)?;
    let state = active_generation_state(&transaction, repository)?;
    if state.snapshot != expected_snapshot || state.generation != expected_generation {
        return Err(SearchFailure::Store(SqliteStoreError::GenerationUnavailable));
    }
    let availability = raw_syntax_site_availability(&transaction, state.generation)?;
    let declaration = exact_symbol_hit(&transaction, state.generation, selector)?;
    let (sites, total_sites, output_bytes) = match (availability, declaration.as_ref()) {
        (RawSyntaxSiteProjectionAvailability::Complete, Some(declaration)) => {
            load_raw_syntax_sites(
                &transaction,
                state.generation,
                selector,
                declaration,
                limits,
                cancelled,
                deadline,
            )?
        }
        _ => (Vec::new().into_boxed_slice(), 0, 0),
    };
    check_control(cancelled, deadline).map_err(SearchFailure::Store)?;
    transaction.commit()?;
    Ok(RawSyntaxSitesReadResult {
        snapshot: state.snapshot,
        generation: state.generation,
        index_coverage: state.index_coverage,
        declaration,
        availability,
        sites,
        total_sites,
        output_bytes,
    })
}

fn raw_syntax_site_availability(
    transaction: &Transaction<'_>,
    generation: GenerationId,
) -> Result<RawSyntaxSiteProjectionAvailability, SearchFailure> {
    let publication: Option<(i64, Option<String>, Option<i64>)> = transaction
        .query_row(
            "SELECT requirement.site_profile_version, publication.lifecycle_state,
                    publication.site_profile_version
             FROM generation_syntax_site_requirements AS requirement
             LEFT JOIN generation_syntax_site_publications AS publication
               ON publication.generation_id = requirement.generation_id
             WHERE requirement.generation_id = ?1",
            [generation.get()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let expected_profile = i64::from(repowitness_analysis::RAW_SYNTAX_SITE_PROFILE_VERSION);
    match publication {
        None => Ok(RawSyntaxSiteProjectionAvailability::NotProduced),
        Some((requirement_profile, Some(lifecycle), Some(publication_profile)))
            if lifecycle == "complete"
                && requirement_profile == expected_profile
                && publication_profile == expected_profile =>
        {
            Ok(RawSyntaxSiteProjectionAvailability::Complete)
        }
        Some(_) => Err(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed)),
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "the query must bind every exact selector and enclosing-span condition"
)]
fn load_raw_syntax_sites(
    transaction: &Transaction<'_>,
    generation: GenerationId,
    selector: &SymbolGetSelector,
    declaration: &SearchHit,
    limits: RawSyntaxSiteReadLimits,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(Box<[RawSyntaxSiteRecord]>, u64, u64), SearchFailure> {
    let content_digest = selector.content_digest();
    let bindings = params![
        generation.get(),
        selector.path().as_bytes(),
        content_digest.as_bytes().as_slice(),
        content_digest.as_bytes().as_slice(),
        declaration.language().as_str(),
        persisted_raw_syntax_offset(declaration.declaration_span().start().get())?,
        persisted_raw_syntax_offset(declaration.declaration_span().end().get())?,
    ];
    let total_sites = transaction.query_row(
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
           AND occurrence.repository_path = ?2
           AND file.content_digest = ?3
           AND artifact.source_content_digest = ?4
           AND artifact.language = ?5
           AND site.occurrence_start >= ?6
           AND site.occurrence_end <= ?7",
        bindings,
        |row| row.get::<_, i64>(0),
    )?;
    let total_sites = persisted_count(total_sites)?;
    let mut statement = transaction.prepare(
        "SELECT occurrence.syntax_site_artifact_digest, artifact.language,
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
           AND occurrence.repository_path = ?2
           AND file.content_digest = ?3
           AND artifact.source_content_digest = ?4
           AND artifact.language = ?5
           AND site.occurrence_start >= ?6
           AND site.occurrence_end <= ?7
         ORDER BY site.occurrence_start, site.occurrence_end,
                  site.target_start, site.target_end, site.ordinal
         LIMIT ?8",
    )?;
    let mut rows = statement.query(params![
        generation.get(),
        selector.path().as_bytes(),
        content_digest.as_bytes().as_slice(),
        content_digest.as_bytes().as_slice(),
        declaration.language().as_str(),
        persisted_raw_syntax_offset(declaration.declaration_span().start().get())?,
        persisted_raw_syntax_offset(declaration.declaration_span().end().get())?,
        i64::from(limits.max_results()),
    ])?;
    let mut sites = Vec::new();
    let mut output_bytes = 0_u64;
    while let Some(row) = rows.next()? {
        check_control(cancelled, deadline).map_err(SearchFailure::Store)?;
        let record = decode_raw_syntax_site_record(row, selector)?;
        output_bytes = output_bytes
            .checked_add(FIXED_RAW_SYNTAX_SITE_OUTPUT_BYTES)
            .and_then(|value| {
                value.checked_add(
                    u64::try_from(record.path.as_bytes().len()).ok()?,
                )
            })
            .and_then(|value| {
                value.checked_add(u64::try_from(record.site.raw_target().len()).ok()?)
            })
            .ok_or(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
        if output_bytes > limits.max_output_bytes() {
            return Err(SearchFailure::Store(
                SqliteStoreError::RawSyntaxSiteReadOutputLimitExceeded,
            ));
        }
        sites.push(record);
    }
    Ok((sites.into_boxed_slice(), total_sites, output_bytes))
}

#[allow(
    clippy::too_many_arguments,
    reason = "the query binds the active generation, exact target, bounds, and cancellation controls"
)]
fn load_raw_syntax_site_matches(
    transaction: &Transaction<'_>,
    generation: GenerationId,
    target: &str,
    limits: RawSyntaxSiteReadLimits,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(Box<[RawSyntaxSiteRecord]>, u64, u64), SearchFailure> {
    let total_sites = transaction.query_row(
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
           AND site.raw_target = ?2",
        params![generation.get(), target],
        |row| row.get::<_, i64>(0),
    )?;
    let total_sites = persisted_count(total_sites)?;
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
           AND site.raw_target = ?2
         ORDER BY file.repository_path, site.occurrence_start, site.occurrence_end,
                  site.target_start, site.target_end, site.ordinal
         LIMIT ?3",
    )?;
    let mut rows = statement.query(params![
        generation.get(),
        target,
        i64::from(limits.max_results()),
    ])?;
    let mut sites = Vec::new();
    let mut output_bytes = 0_u64;
    while let Some(row) = rows.next()? {
        check_control(cancelled, deadline).map_err(SearchFailure::Store)?;
        let record = decode_raw_syntax_site_match_record(row)?;
        output_bytes = output_bytes
            .checked_add(FIXED_RAW_SYNTAX_SITE_OUTPUT_BYTES)
            .and_then(|value| value.checked_add(u64::try_from(record.path.as_bytes().len()).ok()?))
            .and_then(|value| {
                value.checked_add(u64::try_from(record.site.raw_target().len()).ok()?)
            })
            .ok_or(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
        if output_bytes > limits.max_output_bytes() {
            return Err(SearchFailure::Store(
                SqliteStoreError::RawSyntaxSiteReadOutputLimitExceeded,
            ));
        }
        sites.push(record);
    }
    Ok((sites.into_boxed_slice(), total_sites, output_bytes))
}

fn decode_raw_syntax_site_match_record(
    row: &rusqlite::Row<'_>,
) -> Result<RawSyntaxSiteRecord, SearchFailure> {
    let path: Vec<u8> = row.get(0)?;
    let content_digest: Vec<u8> = row.get(1)?;
    let digest: Vec<u8> = row.get(2)?;
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
    let artifact_digest = AnalysisArtifactDigest::try_from_slice(&digest).map_err(|_| integrity())?;
    let language = SourceLanguage::from_stable_str(&language).ok_or_else(integrity)?;
    let ordinal = u32::try_from(ordinal).map_err(|_| integrity())?;
    let kind = RawSyntaxSiteKind::from_stable_str(&kind).ok_or_else(integrity)?;
    let evidence = RawSyntaxSiteEvidence::from_stable_str(&evidence).ok_or_else(integrity)?;
    let site = RawSyntaxSite::try_new(
        RawSyntaxSiteOrdinal::new(ordinal),
        kind,
        evidence,
        raw_syntax_span(occurrence_start, occurrence_end)?,
        raw_syntax_span(target_start, target_end)?,
        raw_target,
        repowitness_analysis::RawSyntaxSiteAnalysisLimits::DEFAULT,
    )
    .map_err(|_| integrity())?;
    Ok(RawSyntaxSiteRecord {
        path,
        content_digest,
        artifact_digest,
        language,
        site,
    })
}

fn decode_raw_syntax_site_record(
    row: &rusqlite::Row<'_>,
    selector: &SymbolGetSelector,
) -> Result<RawSyntaxSiteRecord, SearchFailure> {
    let digest: Vec<u8> = row.get(0)?;
    let language: String = row.get(1)?;
    let ordinal: i64 = row.get(2)?;
    let kind: String = row.get(3)?;
    let evidence: String = row.get(4)?;
    let occurrence_start: i64 = row.get(5)?;
    let occurrence_end: i64 = row.get(6)?;
    let target_start: i64 = row.get(7)?;
    let target_end: i64 = row.get(8)?;
    let raw_target: String = row.get(9)?;
    let integrity = || SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed);
    let artifact_digest = AnalysisArtifactDigest::try_from_slice(&digest).map_err(|_| integrity())?;
    let language = SourceLanguage::from_stable_str(&language).ok_or_else(integrity)?;
    let ordinal = u32::try_from(ordinal).map_err(|_| integrity())?;
    let kind = RawSyntaxSiteKind::from_stable_str(&kind).ok_or_else(integrity)?;
    let evidence = RawSyntaxSiteEvidence::from_stable_str(&evidence).ok_or_else(integrity)?;
    let site = RawSyntaxSite::try_new(
        RawSyntaxSiteOrdinal::new(ordinal),
        kind,
        evidence,
        raw_syntax_span(occurrence_start, occurrence_end)?,
        raw_syntax_span(target_start, target_end)?,
        raw_target,
        repowitness_analysis::RawSyntaxSiteAnalysisLimits::DEFAULT,
    )
    .map_err(|_| integrity())?;
    Ok(RawSyntaxSiteRecord {
        path: selector.path().clone(),
        content_digest: selector.content_digest(),
        artifact_digest,
        language,
        site,
    })
}

fn raw_syntax_span(start: i64, end: i64) -> Result<ByteSpan, SearchFailure> {
    let start = u64::try_from(start)
        .map(ByteOffset::new)
        .map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
    let end = u64::try_from(end)
        .map(ByteOffset::new)
        .map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
    ByteSpan::try_new(start, end)
        .map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))
}

fn persisted_raw_syntax_offset(value: u64) -> Result<i64, SearchFailure> {
    i64::try_from(value).map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))
}
