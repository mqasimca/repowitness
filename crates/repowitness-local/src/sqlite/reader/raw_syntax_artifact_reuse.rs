const MAX_REUSABLE_RAW_SYNTAX_BYTES: u64 = 512 * 1024 * 1024;
const FIXED_RAW_SYNTAX_SITE_BYTES: u64 = 192;

impl OwnedSqliteReader {
    pub(crate) fn load_reusable_raw_syntax_artifacts(
        &self,
        requested: &[AnalysisArtifactDigest],
        identities: SourceArtifactIdentities,
        limits: RustIndexLimits,
        raw_syntax_limits: RawSyntaxSiteAnalysisLimits,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<BTreeMap<AnalysisArtifactDigest, RawSyntaxSiteAnalysis>, SqliteStoreError> {
        validate_artifact_request(requested, limits)?;
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(
            ReaderCommand::LoadRawSyntaxArtifacts(Box::new(RawSyntaxArtifactCommand {
                requested: requested.to_vec().into_boxed_slice(),
                identities,
                limits,
                raw_syntax_limits,
                cancelled: Arc::clone(&cancelled),
                deadline,
                reply,
            })),
            deadline,
        )?;
        match receive_reply(&receiver, deadline) {
            Ok(results) => Ok(results),
            Err(error) => {
                cancelled.store(true, Ordering::Release);
                Err(error)
            }
        }
    }
}

fn load_reusable_raw_syntax_artifacts(
    connection: &mut Connection,
    requested: &[AnalysisArtifactDigest],
    identities: SourceArtifactIdentities,
    limits: RustIndexLimits,
    raw_syntax_limits: RawSyntaxSiteAnalysisLimits,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
) -> Result<BTreeMap<AnalysisArtifactDigest, RawSyntaxSiteAnalysis>, SqliteStoreError> {
    check_control(&cancelled, deadline)?;
    let progress_cancelled = Arc::clone(&cancelled);
    connection
        .progress_handler(
            PROGRESS_OPCODES,
            Some(move || progress_cancelled.load(Ordering::Acquire) || Instant::now() >= deadline),
        )
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    let result = raw_syntax_artifact_transaction(
        connection,
        requested,
        identities,
        limits,
        raw_syntax_limits,
        &cancelled,
        deadline,
    );
    connection
        .progress_handler(0, None::<fn() -> bool>)
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    match result {
        Ok(artifacts) => {
            check_control(&cancelled, deadline)?;
            Ok(artifacts)
        }
        Err(SearchFailure::Sqlite(error)) if is_interrupted(&error) => {
            check_control(&cancelled, deadline)?;
            Err(SqliteStoreError::DatabaseOperationFailed)
        }
        Err(SearchFailure::Sqlite(_)) => Err(SqliteStoreError::DatabaseOperationFailed),
        Err(SearchFailure::Store(error)) => Err(error),
    }
}

fn raw_syntax_artifact_transaction(
    connection: &mut Connection,
    requested: &[AnalysisArtifactDigest],
    identities: SourceArtifactIdentities,
    limits: RustIndexLimits,
    raw_syntax_limits: RawSyntaxSiteAnalysisLimits,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<BTreeMap<AnalysisArtifactDigest, RawSyntaxSiteAnalysis>, SearchFailure> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Deferred)?;
    let mut artifacts = BTreeMap::new();
    let mut budget = RawSyntaxArtifactLoadBudget::default();
    let context = RawSyntaxArtifactReadContext {
        identities,
        limits,
        raw_syntax_limits,
        cancelled,
        deadline,
    };
    for requested_digest in requested {
        check_control(cancelled, deadline).map_err(SearchFailure::Store)?;
        let Some(analysis) = read_reusable_raw_syntax_artifact(
            &transaction,
            *requested_digest,
            context,
            &mut budget,
        )? else {
            continue;
        };
        if artifacts.insert(*requested_digest, analysis).is_some() {
            return Err(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed));
        }
    }
    transaction.commit()?;
    Ok(artifacts)
}

#[derive(Clone, Copy)]
struct RawSyntaxArtifactReadContext<'a> {
    identities: SourceArtifactIdentities,
    limits: RustIndexLimits,
    raw_syntax_limits: RawSyntaxSiteAnalysisLimits,
    cancelled: &'a AtomicBool,
    deadline: Instant,
}

#[derive(Default)]
struct RawSyntaxArtifactLoadBudget {
    sites: u64,
    bytes: u64,
}

struct PersistedRawSyntaxArtifactMetadata {
    lifecycle_state: String,
    source_content_digest: Vec<u8>,
    producer_manifest_digest: Vec<u8>,
    configuration_digest: Vec<u8>,
    analysis_schema_digest: Vec<u8>,
    canonicalization_version: i64,
    fact_count: i64,
    visited_nodes: i64,
    syntax_error_nodes: i64,
    known_parser_limitation_nodes: i64,
    language: String,
    payload_digest: Option<Vec<u8>>,
    site_profile_version: Option<i64>,
    site_count: Option<i64>,
    max_observed_depth: Option<i64>,
    owned_text_bytes: Option<i64>,
    import_support: Option<String>,
    reference_support: Option<String>,
    call_support: Option<String>,
    test_marker_support: Option<String>,
    import_emitted: Option<i64>,
    reference_emitted: Option<i64>,
    call_emitted: Option<i64>,
    test_marker_emitted: Option<i64>,
}

struct ValidatedRawSyntaxArtifactMetadata {
    language: RawSyntaxLanguage,
    visited_nodes: u32,
    syntax_error_nodes: u32,
    site_count: u32,
    max_observed_depth: u16,
    owned_text_bytes: u64,
    supports: [RawSyntaxSiteSupport; 4],
    emitted: [u32; 4],
    payload_digest: [u8; 32],
}

fn read_reusable_raw_syntax_artifact(
    transaction: &Transaction<'_>,
    requested_digest: AnalysisArtifactDigest,
    context: RawSyntaxArtifactReadContext<'_>,
    budget: &mut RawSyntaxArtifactLoadBudget,
) -> Result<Option<RawSyntaxSiteAnalysis>, SearchFailure> {
    check_control(context.cancelled, context.deadline).map_err(SearchFailure::Store)?;
    let persisted = read_raw_syntax_artifact_metadata(transaction, requested_digest)?;
    let Some(persisted) = persisted else {
        return Ok(None);
    };
    if persisted.lifecycle_state == "staging" || persisted.payload_digest.is_none() {
        return Ok(None);
    }
    let metadata = validate_raw_syntax_artifact_metadata(persisted, requested_digest, context)?;
    if !budget.can_admit(&metadata, context.limits.max_files())? {
        return Ok(None);
    }
    let sites = read_reusable_raw_syntax_sites(
        transaction,
        requested_digest,
        &metadata,
        context,
    )?;
    let analysis = RawSyntaxSiteAnalysis::try_from_parts_with_control(
        metadata.language,
        sites,
        metadata.visited_nodes,
        metadata.syntax_error_nodes,
        metadata.max_observed_depth,
        metadata.owned_text_bytes,
        context.raw_syntax_limits,
        RawSyntaxSiteAnalysisControl::new(context.cancelled, context.deadline),
    )
    .map_err(map_raw_syntax_reuse_analysis_error)?;
    for (index, kind) in raw_syntax_kinds().iter().enumerate() {
        let coverage = analysis.coverage().for_kind(*kind);
        if coverage.support() != metadata.supports[index]
            || coverage.emitted() != metadata.emitted[index]
        {
            return Err(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed));
        }
    }
    let payload_digest = super::syntax_sites::artifact_payload_digest(
        &analysis,
        super::RawSyntaxPreparationControl::new(context.cancelled, context.deadline),
    )
    .map_err(map_raw_syntax_reuse_preparation_error)?;
    if payload_digest != metadata.payload_digest {
        return Err(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed));
    }
    budget.admit(&metadata)?;
    Ok(Some(analysis))
}

fn read_raw_syntax_artifact_metadata(
    transaction: &Transaction<'_>,
    requested_digest: AnalysisArtifactDigest,
) -> Result<Option<PersistedRawSyntaxArtifactMetadata>, SearchFailure> {
    transaction
        .query_row(
            "SELECT base.lifecycle_state, base.source_content_digest,
                    base.producer_manifest_digest, base.configuration_digest,
                    base.analysis_schema_digest, base.canonicalization_version,
                    base.fact_count, base.visited_nodes, base.syntax_error_nodes,
                    base.known_parser_limitation_nodes, base.language,
                    base.payload_digest, raw.site_profile_version, raw.site_count,
                    raw.max_observed_depth, raw.owned_text_bytes, raw.import_support,
                    raw.reference_support, raw.call_support, raw.test_marker_support,
                    raw.import_emitted, raw.reference_emitted, raw.call_emitted,
                    raw.test_marker_emitted
             FROM analysis_artifacts AS base
             LEFT JOIN syntax_site_artifacts AS raw
               ON raw.artifact_digest = base.artifact_digest
             WHERE base.artifact_digest = ?1",
            [requested_digest.as_bytes().as_slice()],
            |row| {
                Ok(PersistedRawSyntaxArtifactMetadata {
                    lifecycle_state: row.get(0)?,
                    source_content_digest: row.get(1)?,
                    producer_manifest_digest: row.get(2)?,
                    configuration_digest: row.get(3)?,
                    analysis_schema_digest: row.get(4)?,
                    canonicalization_version: row.get(5)?,
                    fact_count: row.get(6)?,
                    visited_nodes: row.get(7)?,
                    syntax_error_nodes: row.get(8)?,
                    known_parser_limitation_nodes: row.get(9)?,
                    language: row.get(10)?,
                    payload_digest: row.get(11)?,
                    site_profile_version: row.get(12)?,
                    site_count: row.get(13)?,
                    max_observed_depth: row.get(14)?,
                    owned_text_bytes: row.get(15)?,
                    import_support: row.get(16)?,
                    reference_support: row.get(17)?,
                    call_support: row.get(18)?,
                    test_marker_support: row.get(19)?,
                    import_emitted: row.get(20)?,
                    reference_emitted: row.get(21)?,
                    call_emitted: row.get(22)?,
                    test_marker_emitted: row.get(23)?,
                })
            },
        )
        .optional()
        .map_err(SearchFailure::Sqlite)
}

fn validate_raw_syntax_artifact_metadata(
    persisted: PersistedRawSyntaxArtifactMetadata,
    requested_digest: AnalysisArtifactDigest,
    context: RawSyntaxArtifactReadContext<'_>,
) -> Result<ValidatedRawSyntaxArtifactMetadata, SearchFailure> {
    let integrity = || SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed);
    let language = SourceLanguage::from_stable_str(&persisted.language).ok_or_else(integrity)?;
    let raw_language = raw_syntax_language(language);
    let identity = context.identities.for_language(language);
    if persisted.lifecycle_state != "complete"
        || persisted.producer_manifest_digest.as_slice()
            != identity.producer_manifest().as_bytes()
        || persisted.configuration_digest.as_slice() != identity.configuration().as_bytes()
        || persisted.analysis_schema_digest.as_slice() != identity.schema().as_bytes()
        || u32::try_from(persisted.canonicalization_version).ok()
            != Some(identity.canonicalization_version())
        || persisted.fact_count != 0
        || persisted.known_parser_limitation_nodes != 0
        || u32::try_from(persisted.site_profile_version.ok_or_else(integrity)?).ok()
            != Some(repowitness_analysis::RAW_SYNTAX_SITE_PROFILE_VERSION)
    {
        return Err(integrity());
    }
    let content_digest = SourceContentDigest::try_from_slice(&persisted.source_content_digest)
        .map_err(|_| integrity())?;
    let key = AnalysisArtifactKey::new(
        content_digest,
        identity.producer_manifest(),
        identity.configuration(),
        identity.schema(),
        identity.canonicalization_version(),
    );
    if hash_analysis_artifact_key(&key) != requested_digest {
        return Err(integrity());
    }
    let visited_nodes = raw_syntax_reuse_u32(persisted.visited_nodes)?;
    let syntax_error_nodes = raw_syntax_reuse_u32(persisted.syntax_error_nodes)?;
    let site_count = raw_syntax_reuse_u32(persisted.site_count.ok_or_else(integrity)?)?;
    let max_observed_depth = u16::try_from(persisted.max_observed_depth.ok_or_else(integrity)?)
        .map_err(|_| integrity())?;
    let owned_text_bytes = u64::try_from(persisted.owned_text_bytes.ok_or_else(integrity)?)
        .map_err(|_| integrity())?;
    let supports = [
        raw_syntax_support(persisted.import_support.as_deref().ok_or_else(integrity)?)?,
        raw_syntax_support(persisted.reference_support.as_deref().ok_or_else(integrity)?)?,
        raw_syntax_support(persisted.call_support.as_deref().ok_or_else(integrity)?)?,
        raw_syntax_support(persisted.test_marker_support.as_deref().ok_or_else(integrity)?)?,
    ];
    let emitted = [
        raw_syntax_reuse_u32(persisted.import_emitted.ok_or_else(integrity)?)?,
        raw_syntax_reuse_u32(persisted.reference_emitted.ok_or_else(integrity)?)?,
        raw_syntax_reuse_u32(persisted.call_emitted.ok_or_else(integrity)?)?,
        raw_syntax_reuse_u32(persisted.test_marker_emitted.ok_or_else(integrity)?)?,
    ];
    let total_emitted = emitted.into_iter().try_fold(0_u32, |total, count| {
        total.checked_add(count).ok_or_else(integrity)
    })?;
    if visited_nodes == 0
        || visited_nodes > context.raw_syntax_limits.max_syntax_nodes()
        || syntax_error_nodes > visited_nodes
        || site_count > context.raw_syntax_limits.max_sites()
        || max_observed_depth > context.raw_syntax_limits.max_syntax_depth()
        || owned_text_bytes > context.raw_syntax_limits.max_owned_text_bytes()
        || total_emitted != site_count
    {
        return Err(integrity());
    }
    let payload = persisted.payload_digest.ok_or_else(integrity)?;
    let payload_digest = payload.as_slice().try_into().map_err(|_| integrity())?;
    Ok(ValidatedRawSyntaxArtifactMetadata {
        language: raw_language,
        visited_nodes,
        syntax_error_nodes,
        site_count,
        max_observed_depth,
        owned_text_bytes,
        supports,
        emitted,
        payload_digest,
    })
}

impl RawSyntaxArtifactLoadBudget {
    fn can_admit(
        &self,
        metadata: &ValidatedRawSyntaxArtifactMetadata,
        max_files: u64,
    ) -> Result<bool, SearchFailure> {
        let sites = self
            .sites
            .checked_add(u64::from(metadata.site_count))
            .ok_or_else(raw_syntax_reuse_count_error)?;
        let fixed_bytes = u64::from(metadata.site_count)
            .checked_mul(FIXED_RAW_SYNTAX_SITE_BYTES)
            .ok_or_else(raw_syntax_reuse_count_error)?;
        let bytes = self
            .bytes
            .checked_add(fixed_bytes)
            .and_then(|value| value.checked_add(metadata.owned_text_bytes))
            .ok_or_else(raw_syntax_reuse_count_error)?;
        let max_sites = u64::from(RawSyntaxSiteAnalysisLimits::DEFAULT.max_sites())
            .checked_mul(max_files)
            .ok_or_else(raw_syntax_reuse_count_error)?;
        Ok(sites <= max_sites && bytes <= MAX_REUSABLE_RAW_SYNTAX_BYTES)
    }

    fn admit(
        &mut self,
        metadata: &ValidatedRawSyntaxArtifactMetadata,
    ) -> Result<(), SearchFailure> {
        self.sites = self
            .sites
            .checked_add(u64::from(metadata.site_count))
            .ok_or_else(raw_syntax_reuse_count_error)?;
        self.bytes = self
            .bytes
            .checked_add(
                u64::from(metadata.site_count)
                    .checked_mul(FIXED_RAW_SYNTAX_SITE_BYTES)
                    .ok_or_else(raw_syntax_reuse_count_error)?,
            )
            .and_then(|value| value.checked_add(metadata.owned_text_bytes))
            .ok_or_else(raw_syntax_reuse_count_error)?;
        Ok(())
    }
}

fn read_reusable_raw_syntax_sites(
    transaction: &Transaction<'_>,
    artifact_digest: AnalysisArtifactDigest,
    metadata: &ValidatedRawSyntaxArtifactMetadata,
    context: RawSyntaxArtifactReadContext<'_>,
) -> Result<Vec<RawSyntaxSite>, SearchFailure> {
    let capacity = usize::try_from(metadata.site_count)
        .map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
    let mut statement = transaction.prepare(
        "SELECT ordinal, site_kind, extraction_evidence,
                occurrence_start, occurrence_end, target_start, target_end, raw_target
         FROM syntax_sites
         WHERE artifact_digest = ?1
         ORDER BY ordinal",
    )?;
    let mut rows = statement.query([artifact_digest.as_bytes().as_slice()])?;
    let mut sites = Vec::with_capacity(capacity);
    while let Some(row) = rows.next()? {
        check_control(context.cancelled, context.deadline).map_err(SearchFailure::Store)?;
        sites.push(decode_reusable_raw_syntax_site(
            row,
            sites.len(),
            context.raw_syntax_limits,
        )?);
    }
    if sites.len() != capacity {
        return Err(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed));
    }
    Ok(sites)
}

fn decode_reusable_raw_syntax_site(
    row: &rusqlite::Row<'_>,
    expected_ordinal: usize,
    limits: RawSyntaxSiteAnalysisLimits,
) -> Result<RawSyntaxSite, SearchFailure> {
    let integrity = || SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed);
    let ordinal = raw_syntax_reuse_u32(row.get(0)?)?;
    if usize::try_from(ordinal).ok() != Some(expected_ordinal) {
        return Err(integrity());
    }
    let kind = RawSyntaxSiteKind::from_stable_str(&row.get::<_, String>(1)?)
        .ok_or_else(integrity)?;
    let evidence = RawSyntaxSiteEvidence::from_stable_str(&row.get::<_, String>(2)?)
        .ok_or_else(integrity)?;
    RawSyntaxSite::try_new(
        RawSyntaxSiteOrdinal::new(ordinal),
        kind,
        evidence,
        raw_syntax_reuse_span(row.get(3)?, row.get(4)?)?,
        raw_syntax_reuse_span(row.get(5)?, row.get(6)?)?,
        row.get(7)?,
        limits,
    )
    .map_err(|_| integrity())
}

const fn raw_syntax_kinds() -> [RawSyntaxSiteKind; 4] {
    [
        RawSyntaxSiteKind::Import,
        RawSyntaxSiteKind::Reference,
        RawSyntaxSiteKind::Call,
        RawSyntaxSiteKind::TestMarker,
    ]
}

fn raw_syntax_language(language: SourceLanguage) -> RawSyntaxLanguage {
    match language {
        SourceLanguage::Rust => RawSyntaxLanguage::Rust,
        SourceLanguage::Go => RawSyntaxLanguage::Go,
        SourceLanguage::TypeScript => RawSyntaxLanguage::TypeScript,
        SourceLanguage::Tsx => RawSyntaxLanguage::Tsx,
        SourceLanguage::Python => RawSyntaxLanguage::Python,
    }
}

fn raw_syntax_support(value: &str) -> Result<RawSyntaxSiteSupport, SearchFailure> {
    match value {
        "available" => Ok(RawSyntaxSiteSupport::Available),
        "unsupported" => Ok(RawSyntaxSiteSupport::Unsupported),
        _ => Err(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed)),
    }
}

fn raw_syntax_reuse_span(start: i64, end: i64) -> Result<ByteSpan, SearchFailure> {
    ByteSpan::try_new(
        ByteOffset::new(raw_syntax_reuse_u64(start)?),
        ByteOffset::new(raw_syntax_reuse_u64(end)?),
    )
    .map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))
}

fn raw_syntax_reuse_u32(value: i64) -> Result<u32, SearchFailure> {
    u32::try_from(value).map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))
}

fn raw_syntax_reuse_u64(value: i64) -> Result<u64, SearchFailure> {
    u64::try_from(value).map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))
}

fn raw_syntax_reuse_count_error() -> SearchFailure {
    SearchFailure::Store(SqliteStoreError::CountNotRepresentable)
}

fn map_raw_syntax_reuse_analysis_error(error: RawSyntaxSiteAnalysisError) -> SearchFailure {
    SearchFailure::Store(match error {
        RawSyntaxSiteAnalysisError::Cancelled => SqliteStoreError::Cancelled,
        RawSyntaxSiteAnalysisError::DeadlineExceeded => SqliteStoreError::DeadlineExceeded,
        _ => SqliteStoreError::IntegrityCheckFailed,
    })
}

fn map_raw_syntax_reuse_preparation_error(
    error: super::RawSyntaxPreparationError,
) -> SearchFailure {
    SearchFailure::Store(match error {
        super::RawSyntaxPreparationError::Cancelled => SqliteStoreError::Cancelled,
        super::RawSyntaxPreparationError::DeadlineExceeded => SqliteStoreError::DeadlineExceeded,
        _ => SqliteStoreError::IntegrityCheckFailed,
    })
}
