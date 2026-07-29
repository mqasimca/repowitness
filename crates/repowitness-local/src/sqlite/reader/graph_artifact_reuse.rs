const MAX_REUSABLE_GRAPH_BYTES: u64 = 512 * 1024 * 1024;
const FIXED_GRAPH_SITE_BYTES: u64 = 192;

impl OwnedSqliteReader {
    pub(crate) fn load_reusable_graph_artifacts(
        &self,
        requested: &[AnalysisArtifactDigest],
        identity: RustArtifactIdentity,
        limits: RustIndexLimits,
        graph_limits: RustGraphAnalysisLimits,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<BTreeMap<AnalysisArtifactDigest, RustGraphSiteAnalysis>, SqliteStoreError> {
        validate_artifact_request(requested, limits)?;
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(
            ReaderCommand::LoadGraphArtifacts(Box::new(GraphArtifactCommand {
                requested: requested.to_vec().into_boxed_slice(),
                identity,
                limits,
                graph_limits,
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

fn load_reusable_graph_artifacts(
    connection: &mut Connection,
    requested: &[AnalysisArtifactDigest],
    identity: RustArtifactIdentity,
    limits: RustIndexLimits,
    graph_limits: RustGraphAnalysisLimits,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
) -> Result<BTreeMap<AnalysisArtifactDigest, RustGraphSiteAnalysis>, SqliteStoreError> {
    check_control(&cancelled, deadline)?;
    let progress_cancelled = Arc::clone(&cancelled);
    connection
        .progress_handler(
            PROGRESS_OPCODES,
            Some(move || progress_cancelled.load(Ordering::Acquire) || Instant::now() >= deadline),
        )
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    let result = graph_artifact_transaction(
        connection,
        requested,
        identity,
        limits,
        graph_limits,
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

fn graph_artifact_transaction(
    connection: &mut Connection,
    requested: &[AnalysisArtifactDigest],
    identity: RustArtifactIdentity,
    limits: RustIndexLimits,
    graph_limits: RustGraphAnalysisLimits,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<BTreeMap<AnalysisArtifactDigest, RustGraphSiteAnalysis>, SearchFailure> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Deferred)?;
    let mut artifacts = BTreeMap::new();
    let mut budget = GraphArtifactLoadBudget::default();
    let context = GraphArtifactReadContext {
        identity,
        limits,
        graph_limits,
        cancelled,
        deadline,
    };
    for requested_digest in requested {
        check_control(cancelled, deadline).map_err(SearchFailure::Store)?;
        let Some(analysis) =
            read_reusable_graph_artifact(&transaction, *requested_digest, context, &mut budget)?
        else {
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
struct GraphArtifactReadContext<'a> {
    identity: RustArtifactIdentity,
    limits: RustIndexLimits,
    graph_limits: RustGraphAnalysisLimits,
    cancelled: &'a AtomicBool,
    deadline: Instant,
}

#[derive(Default)]
struct GraphArtifactLoadBudget {
    sites: u64,
    bytes: u64,
}

struct PersistedGraphArtifactMetadata {
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
}

struct ValidatedGraphArtifactMetadata {
    visited_nodes: u32,
    syntax_error_nodes: u32,
    site_count: u32,
    max_observed_depth: u16,
    owned_text_bytes: u64,
    payload_digest: [u8; 32],
}

fn read_reusable_graph_artifact(
    transaction: &Transaction<'_>,
    requested_digest: AnalysisArtifactDigest,
    context: GraphArtifactReadContext<'_>,
    budget: &mut GraphArtifactLoadBudget,
) -> Result<Option<RustGraphSiteAnalysis>, SearchFailure> {
    check_control(context.cancelled, context.deadline).map_err(SearchFailure::Store)?;
    let persisted = read_graph_artifact_metadata(transaction, requested_digest)?;
    let Some(persisted) = persisted else {
        return Ok(None);
    };
    if persisted.lifecycle_state == "staging" || persisted.payload_digest.is_none() {
        return Ok(None);
    }
    let metadata = validate_graph_artifact_metadata(persisted, requested_digest, context)?;
    if !budget.can_admit(
        &metadata,
        context.limits.max_files(),
        context.graph_limits.max_graph_sites(),
    )? {
        return Ok(None);
    }
    let sites = read_reusable_graph_sites(transaction, requested_digest, &metadata, context)?;
    let analysis = RustGraphSiteAnalysis::try_from_parts_with_control(
        sites,
        metadata.visited_nodes,
        metadata.syntax_error_nodes,
        metadata.max_observed_depth,
        metadata.owned_text_bytes,
        context.graph_limits,
        RustGraphAnalysisControl::new(context.cancelled, context.deadline),
    )
    .map_err(map_graph_reuse_analysis_error)?;
    let payload_digest = super::graph::artifact_payload_digest_with_control(
        &analysis,
        RustGraphPreparationControl::new(context.cancelled, context.deadline),
    )
    .map_err(map_graph_reuse_preparation_error)?;
    if payload_digest != metadata.payload_digest {
        return Err(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed));
    }
    budget.admit(&metadata)?;
    Ok(Some(analysis))
}

fn read_graph_artifact_metadata(
    transaction: &Transaction<'_>,
    requested_digest: AnalysisArtifactDigest,
) -> Result<Option<PersistedGraphArtifactMetadata>, SearchFailure> {
    transaction
        .query_row(
            "SELECT base.lifecycle_state, base.source_content_digest,
                    base.producer_manifest_digest, base.configuration_digest,
                    base.analysis_schema_digest, base.canonicalization_version,
                    base.fact_count, base.visited_nodes, base.syntax_error_nodes,
                    base.known_parser_limitation_nodes, base.language,
                    base.payload_digest, graph.site_profile_version,
                    graph.site_count, graph.max_observed_depth,
                    graph.owned_text_bytes
             FROM analysis_artifacts AS base
             LEFT JOIN rust_graph_artifacts AS graph
               ON graph.artifact_digest = base.artifact_digest
             WHERE base.artifact_digest = ?1",
            [requested_digest.as_bytes().as_slice()],
            |row| {
                Ok(PersistedGraphArtifactMetadata {
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
                })
            },
        )
        .optional()
        .map_err(SearchFailure::Sqlite)
}

fn validate_graph_artifact_metadata(
    persisted: PersistedGraphArtifactMetadata,
    requested_digest: AnalysisArtifactDigest,
    context: GraphArtifactReadContext<'_>,
) -> Result<ValidatedGraphArtifactMetadata, SearchFailure> {
    let integrity = || SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed);
    if persisted.lifecycle_state != "complete"
        || persisted.producer_manifest_digest.as_slice()
            != context.identity.producer_manifest().as_bytes()
        || persisted.configuration_digest.as_slice() != context.identity.configuration().as_bytes()
        || persisted.analysis_schema_digest.as_slice() != context.identity.schema().as_bytes()
        || u32::try_from(persisted.canonicalization_version).ok()
            != Some(context.identity.canonicalization_version())
        || persisted.fact_count != 0
        || persisted.known_parser_limitation_nodes != 0
        || SourceLanguage::from_stable_str(&persisted.language) != Some(SourceLanguage::Rust)
        || u32::try_from(persisted.site_profile_version.ok_or_else(integrity)?).ok()
            != Some(RUST_GRAPH_SITE_PROFILE_VERSION)
    {
        return Err(integrity());
    }
    let content_digest = SourceContentDigest::try_from_slice(&persisted.source_content_digest)
        .map_err(|_| integrity())?;
    let artifact_key = AnalysisArtifactKey::new(
        content_digest,
        context.identity.producer_manifest(),
        context.identity.configuration(),
        context.identity.schema(),
        context.identity.canonicalization_version(),
    );
    if hash_analysis_artifact_key(&artifact_key) != requested_digest {
        return Err(integrity());
    }
    let visited_nodes = graph_reuse_u32(persisted.visited_nodes)?;
    let syntax_error_nodes = graph_reuse_u32(persisted.syntax_error_nodes)?;
    let site_count = graph_reuse_u32(persisted.site_count.ok_or_else(integrity)?)?;
    let max_observed_depth = u16::try_from(
        persisted.max_observed_depth.ok_or_else(integrity)?,
    )
    .map_err(|_| integrity())?;
    let owned_text_bytes = u64::try_from(persisted.owned_text_bytes.ok_or_else(integrity)?)
        .map_err(|_| integrity())?;
    if visited_nodes == 0
        || visited_nodes > context.graph_limits.max_syntax_nodes()
        || syntax_error_nodes > visited_nodes
        || site_count > context.graph_limits.max_graph_sites()
        || max_observed_depth > context.graph_limits.max_syntax_depth()
        || owned_text_bytes > context.graph_limits.max_owned_text_bytes()
    {
        return Err(integrity());
    }
    let payload = persisted.payload_digest.ok_or_else(integrity)?;
    let payload_digest = payload.as_slice().try_into().map_err(|_| integrity())?;
    Ok(ValidatedGraphArtifactMetadata {
        visited_nodes,
        syntax_error_nodes,
        site_count,
        max_observed_depth,
        owned_text_bytes,
        payload_digest,
    })
}

impl GraphArtifactLoadBudget {
    fn can_admit(
        &self,
        metadata: &ValidatedGraphArtifactMetadata,
        max_files: u64,
        max_sites_per_file: u32,
    ) -> Result<bool, SearchFailure> {
        let sites = self
            .sites
            .checked_add(u64::from(metadata.site_count))
            .ok_or_else(graph_reuse_count_error)?;
        let fixed_bytes = u64::from(metadata.site_count)
            .checked_mul(FIXED_GRAPH_SITE_BYTES)
            .ok_or_else(graph_reuse_count_error)?;
        let bytes = self
            .bytes
            .checked_add(fixed_bytes)
            .and_then(|value| value.checked_add(metadata.owned_text_bytes))
            .ok_or_else(graph_reuse_count_error)?;
        let max_sites = u64::from(max_sites_per_file)
            .checked_mul(max_files)
            .ok_or_else(graph_reuse_count_error)?;
        Ok(sites <= max_sites && bytes <= MAX_REUSABLE_GRAPH_BYTES)
    }

    fn admit(&mut self, metadata: &ValidatedGraphArtifactMetadata) -> Result<(), SearchFailure> {
        self.sites = self
            .sites
            .checked_add(u64::from(metadata.site_count))
            .ok_or_else(graph_reuse_count_error)?;
        self.bytes = self
            .bytes
            .checked_add(
                u64::from(metadata.site_count)
                    .checked_mul(FIXED_GRAPH_SITE_BYTES)
                    .ok_or_else(graph_reuse_count_error)?,
            )
            .and_then(|value| value.checked_add(metadata.owned_text_bytes))
            .ok_or_else(graph_reuse_count_error)?;
        Ok(())
    }
}

fn read_reusable_graph_sites(
    transaction: &Transaction<'_>,
    artifact_digest: AnalysisArtifactDigest,
    metadata: &ValidatedGraphArtifactMetadata,
    context: GraphArtifactReadContext<'_>,
) -> Result<Vec<RustGraphSite>, SearchFailure> {
    let capacity = usize::try_from(metadata.site_count)
        .map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
    let mut statement = transaction.prepare(
        "SELECT ordinal, site_kind, extraction_evidence,
                occurrence_start, occurrence_end, target_start, target_end,
                raw_target, enclosing_kind, enclosing_name,
                enclosing_qualified_name, enclosing_name_start,
                enclosing_name_end, enclosing_declaration_start,
                enclosing_declaration_end
         FROM rust_graph_sites
         WHERE artifact_digest = ?1
         ORDER BY ordinal",
    )?;
    let mut rows = statement.query([artifact_digest.as_bytes().as_slice()])?;
    let mut sites = Vec::with_capacity(capacity);
    while let Some(row) = rows.next()? {
        check_control(context.cancelled, context.deadline).map_err(SearchFailure::Store)?;
        sites.push(decode_reusable_graph_site(row, sites.len(), context.graph_limits)?);
    }
    if sites.len() != capacity {
        return Err(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed));
    }
    Ok(sites)
}

fn decode_reusable_graph_site(
    row: &rusqlite::Row<'_>,
    expected_ordinal: usize,
    limits: RustGraphAnalysisLimits,
) -> Result<RustGraphSite, SearchFailure> {
    let integrity = || SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed);
    let ordinal = graph_reuse_u32(row.get(0)?)?;
    if usize::try_from(ordinal).ok() != Some(expected_ordinal) {
        return Err(integrity());
    }
    let kind = RustGraphSiteKind::from_stable_str(&row.get::<_, String>(1)?)
        .ok_or_else(integrity)?;
    let evidence =
        repowitness_analysis::RustGraphSiteEvidence::from_stable_str(&row.get::<_, String>(2)?)
            .ok_or_else(integrity)?;
    let occurrence_span = graph_reuse_span(row.get(3)?, row.get(4)?)?;
    let target_span = graph_reuse_span(row.get(5)?, row.get(6)?)?;
    let raw_target = row.get(7)?;
    let enclosing = decode_reusable_graph_enclosing(row, limits)?;
    RustGraphSite::try_new(
        RustGraphSiteOrdinal::new(ordinal),
        kind,
        evidence,
        occurrence_span,
        target_span,
        raw_target,
        enclosing,
        limits,
    )
    .map_err(|_| integrity())
}

fn decode_reusable_graph_enclosing(
    row: &rusqlite::Row<'_>,
    limits: RustGraphAnalysisLimits,
) -> Result<Option<RustGraphEnclosingDefinition>, SearchFailure> {
    let values = (
        row.get::<_, Option<String>>(8)?,
        row.get::<_, Option<String>>(9)?,
        row.get::<_, Option<String>>(10)?,
        row.get::<_, Option<i64>>(11)?,
        row.get::<_, Option<i64>>(12)?,
        row.get::<_, Option<i64>>(13)?,
        row.get::<_, Option<i64>>(14)?,
    );
    match values {
        (None, None, None, None, None, None, None) => Ok(None),
        (Some(kind), Some(name), Some(qualified), Some(ns), Some(ne), Some(ds), Some(de)) => {
            let kind = RustSymbolKind::from_stable_str(&kind).ok_or_else(|| {
                SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed)
            })?;
            RustGraphEnclosingDefinition::try_new(
                kind,
                name,
                qualified,
                graph_reuse_span(ns, ne)?,
                graph_reuse_span(ds, de)?,
                limits,
            )
            .map(Some)
            .map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))
        }
        _ => Err(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed)),
    }
}

fn graph_reuse_span(start: i64, end: i64) -> Result<ByteSpan, SearchFailure> {
    ByteSpan::try_new(
        ByteOffset::new(graph_reuse_u64(start)?),
        ByteOffset::new(graph_reuse_u64(end)?),
    )
    .map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))
}

fn graph_reuse_u32(value: i64) -> Result<u32, SearchFailure> {
    u32::try_from(value)
        .map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))
}

fn graph_reuse_u64(value: i64) -> Result<u64, SearchFailure> {
    u64::try_from(value)
        .map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))
}

fn graph_reuse_count_error() -> SearchFailure {
    SearchFailure::Store(SqliteStoreError::CountNotRepresentable)
}

fn map_graph_reuse_analysis_error(error: RustGraphAnalysisError) -> SearchFailure {
    SearchFailure::Store(match error {
        RustGraphAnalysisError::Cancelled => SqliteStoreError::Cancelled,
        RustGraphAnalysisError::DeadlineExceeded => SqliteStoreError::DeadlineExceeded,
        _ => SqliteStoreError::IntegrityCheckFailed,
    })
}

fn map_graph_reuse_preparation_error(error: RustGraphPreparationError) -> SearchFailure {
    SearchFailure::Store(match error {
        RustGraphPreparationError::Cancelled => SqliteStoreError::Cancelled,
        RustGraphPreparationError::DeadlineExceeded => SqliteStoreError::DeadlineExceeded,
        _ => SqliteStoreError::IntegrityCheckFailed,
    })
}
