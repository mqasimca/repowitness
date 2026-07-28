enum SearchFailure {
    Sqlite(rusqlite::Error),
    Store(SqliteStoreError),
}

impl From<rusqlite::Error> for SearchFailure {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

fn artifact_transaction(
    connection: &mut Connection,
    requested: &[AnalysisArtifactDigest],
    language: SourceLanguage,
    identity: RustArtifactIdentity,
    limits: RustIndexLimits,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<BTreeMap<AnalysisArtifactDigest, RustSourceAnalysis>, SearchFailure> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Deferred)?;
    let mut artifacts = BTreeMap::new();
    let mut budget = ArtifactLoadBudget { facts: 0, bytes: 0 };
    let context = ArtifactReadContext {
        language,
        identity,
        limits,
        cancelled,
        deadline,
    };
    for requested_digest in requested {
        check_control(cancelled, deadline).map_err(SearchFailure::Store)?;
        let Some(artifact) =
            read_reusable_artifact(&transaction, *requested_digest, context, &mut budget)?
        else {
            continue;
        };
        if artifacts.insert(*requested_digest, artifact).is_some() {
            return Err(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed));
        }
    }
    transaction.commit()?;
    Ok(artifacts)
}

type PersistedArtifactMetadata = (
    Vec<u8>,
    Vec<u8>,
    Vec<u8>,
    Vec<u8>,
    i64,
    i64,
    i64,
    i64,
    String,
    Vec<u8>,
);

#[derive(Clone, Copy)]
struct ArtifactReadContext<'a> {
    language: SourceLanguage,
    identity: RustArtifactIdentity,
    limits: RustIndexLimits,
    cancelled: &'a AtomicBool,
    deadline: Instant,
}

struct ArtifactLoadBudget {
    facts: u64,
    bytes: u64,
}

fn read_reusable_artifact(
    transaction: &Transaction<'_>,
    requested_digest: AnalysisArtifactDigest,
    context: ArtifactReadContext<'_>,
    budget: &mut ArtifactLoadBudget,
) -> Result<Option<RustSourceAnalysis>, SearchFailure> {
    check_control(context.cancelled, context.deadline).map_err(SearchFailure::Store)?;
    let persisted: Option<PersistedArtifactMetadata> = transaction
        .query_row(
            "SELECT source_content_digest, producer_manifest_digest,
                    configuration_digest, analysis_schema_digest,
                    canonicalization_version, fact_count, visited_nodes,
                    syntax_error_nodes, language, payload_digest
             FROM analysis_artifacts
             WHERE artifact_digest = ?1
               AND lifecycle_state = 'complete'
               AND payload_digest IS NOT NULL
               AND length(source_content_digest) = 32
               AND length(producer_manifest_digest) = 32
               AND length(configuration_digest) = 32
               AND length(analysis_schema_digest) = 32
               AND length(payload_digest) = 32",
            [requested_digest.as_bytes().as_slice()],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                    row.get(9)?,
                ))
            },
        )
        .optional()?;
    let Some(persisted) = persisted else {
        let eligible = transaction.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM analysis_artifacts
                 WHERE artifact_digest = ?1
                   AND lifecycle_state = 'complete'
                   AND payload_digest IS NOT NULL
             )",
            [requested_digest.as_bytes().as_slice()],
            |row| row.get::<_, i64>(0),
        )?;
        return if eligible == 0 {
            Ok(None)
        } else {
            Err(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))
        };
    };
    let content_digest = SourceContentDigest::try_from_slice(&persisted.0)
        .map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
    if persisted.1.as_slice() != context.identity.producer_manifest().as_bytes()
        || persisted.2.as_slice() != context.identity.configuration().as_bytes()
        || persisted.3.as_slice() != context.identity.schema().as_bytes()
        || u32::try_from(persisted.4).ok() != Some(context.identity.canonicalization_version())
        || SourceLanguage::from_stable_str(&persisted.8) != Some(context.language)
    {
        return Err(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed));
    }
    let artifact_key = AnalysisArtifactKey::new(
        content_digest,
        context.identity.producer_manifest(),
        context.identity.configuration(),
        context.identity.schema(),
        context.identity.canonicalization_version(),
    );
    if hash_analysis_artifact_key(&artifact_key) != requested_digest {
        return Err(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed));
    }
    let expected_fact_count = persisted_nonnegative_u32(persisted.5)?;
    let visited_nodes = persisted_nonnegative_u32(persisted.6)?;
    let syntax_error_nodes = persisted_nonnegative_u32(persisted.7)?;
    if expected_fact_count > context.limits.per_file().max_symbol_facts()
        || visited_nodes == 0
        || visited_nodes > context.limits.per_file().max_syntax_nodes()
        || syntax_error_nodes > visited_nodes
    {
        return Err(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed));
    }
    let payload_digest = AnalysisArtifactPayloadDigest::try_from_slice(&persisted.9)
        .map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
    let facts = read_reusable_facts(
        transaction,
        requested_digest,
        expected_fact_count,
        context,
        budget,
    )?;
    let analysis = RustSourceAnalysis::try_from_parts(
        facts,
        visited_nodes,
        syntax_error_nodes,
        context.limits.per_file(),
    )
    .map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
    if hash_analysis_artifact_payload(&analysis) != payload_digest {
        return Err(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed));
    }
    Ok(Some(analysis))
}

fn read_reusable_facts(
    transaction: &Transaction<'_>,
    artifact_digest: AnalysisArtifactDigest,
    expected_count: u32,
    context: ArtifactReadContext<'_>,
    budget: &mut ArtifactLoadBudget,
) -> Result<Vec<RustSymbolFact>, SearchFailure> {
    let projected_total_facts =
        budget
            .facts
            .checked_add(u64::from(expected_count))
            .ok_or(SearchFailure::Store(
                SqliteStoreError::CountNotRepresentable,
            ))?;
    if projected_total_facts > context.limits.max_total_facts() {
        return Err(SearchFailure::Store(
            SqliteStoreError::ArtifactReuseLimitExceeded,
        ));
    }
    let capacity = usize::try_from(expected_count)
        .map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
    let mut statement = transaction.prepare(
        "SELECT facts.ordinal, facts.kind, facts.name, facts.qualified_name,
                facts.name_start, facts.name_end,
                facts.declaration_start, facts.declaration_end,
                correspondence.declaration_digest,
                correspondence.name_elided_digest
         FROM artifact_facts AS facts
         LEFT JOIN artifact_fact_correspondence AS correspondence
           ON correspondence.artifact_digest = facts.artifact_digest
          AND correspondence.fact_ordinal = facts.ordinal
          AND correspondence.profile_id = ?4
          AND correspondence.profile_version = ?5
         WHERE facts.artifact_digest = ?1
           AND length(CAST(facts.kind AS BLOB)) BETWEEN 1 AND 16
           AND length(CAST(facts.name AS BLOB)) BETWEEN 1 AND ?2
           AND length(CAST(facts.qualified_name AS BLOB)) BETWEEN 1 AND ?3
         ORDER BY facts.ordinal",
    )?;
    let mut rows = statement.query(params![
        artifact_digest.as_bytes().as_slice(),
        i64::from(context.limits.per_file().max_symbol_name_bytes()),
        i64::from(context.limits.per_file().max_qualified_name_bytes()),
        RUST_CORRESPONDENCE_PROFILE_ID,
        i64::from(RUST_CORRESPONDENCE_PROFILE_VERSION),
    ])?;
    let mut facts = Vec::with_capacity(capacity);
    while let Some(row) = rows.next()? {
        check_control(context.cancelled, context.deadline).map_err(SearchFailure::Store)?;
        if facts.len() >= capacity {
            return Err(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed));
        }
        let expected_ordinal = i64::try_from(facts.len())
            .map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
        let (fact, bytes) = decode_reusable_fact(
            row,
            expected_ordinal,
            context.limits.per_file(),
            budget.bytes,
        )?;
        budget.bytes = bytes;
        facts.push(fact);
    }
    if facts.len() != capacity {
        return Err(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed));
    }
    budget.facts = projected_total_facts;
    Ok(facts)
}

fn decode_reusable_fact(
    row: &rusqlite::Row<'_>,
    expected_ordinal: i64,
    limits: RustAnalysisLimits,
    current_bytes: u64,
) -> Result<(RustSymbolFact, u64), SearchFailure> {
    let ordinal: i64 = row.get(0)?;
    let kind: String = row.get(1)?;
    let name: String = row.get(2)?;
    let qualified_name: String = row.get(3)?;
    let name_start: i64 = row.get(4)?;
    let name_end: i64 = row.get(5)?;
    let declaration_start: i64 = row.get(6)?;
    let declaration_end: i64 = row.get(7)?;
    let declaration_digest: Option<Vec<u8>> = row.get(8)?;
    let name_elided_digest: Option<Vec<u8>> = row.get(9)?;
    if ordinal != expected_ordinal {
        return Err(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed));
    }
    let correspondence =
        persisted_correspondence(declaration_digest.as_deref(), name_elided_digest.as_deref())?;
    let bytes = checked_artifact_bytes(
        current_bytes,
        &kind,
        &name,
        &qualified_name,
        correspondence.is_some(),
    )
    .map_err(SearchFailure::Store)?;
    let kind = RustSymbolKind::from_stable_str(&kind)
        .ok_or(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
    let name_span = persisted_span(name_start, name_end).map_err(SearchFailure::Store)?;
    let declaration_span =
        persisted_span(declaration_start, declaration_end).map_err(SearchFailure::Store)?;
    let fact = if let Some(correspondence) = correspondence {
        RustSymbolFact::try_new_with_correspondence(
            kind,
            name,
            qualified_name,
            name_span,
            declaration_span,
            correspondence,
            limits,
        )
    } else {
        RustSymbolFact::try_new(
            kind,
            name,
            qualified_name,
            name_span,
            declaration_span,
            limits,
        )
    }
    .map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?;
    Ok((fact, bytes))
}

fn persisted_correspondence(
    declaration: Option<&[u8]>,
    name_elided: Option<&[u8]>,
) -> Result<Option<RustOccurrenceFingerprint>, SearchFailure> {
    match (declaration, name_elided) {
        (None, None) => Ok(None),
        (Some(declaration), Some(name_elided)) => Ok(Some(RustOccurrenceFingerprint::new(
            DeclarationDigest::try_from_slice(declaration)
                .map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?,
            CorrespondenceFingerprintDigest::try_from_slice(name_elided)
                .map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))?,
        ))),
        _ => Err(SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed)),
    }
}

fn persisted_nonnegative_u32(value: i64) -> Result<u32, SearchFailure> {
    u32::try_from(value).map_err(|_| SearchFailure::Store(SqliteStoreError::IntegrityCheckFailed))
}

fn checked_artifact_bytes(
    current: u64,
    kind: &str,
    name: &str,
    qualified_name: &str,
    has_correspondence: bool,
) -> Result<u64, SqliteStoreError> {
    let row_bytes = (if has_correspondence { 136_u64 } else { 72_u64 })
        .checked_add(u64::try_from(kind.len()).unwrap_or(u64::MAX))
        .and_then(|value| value.checked_add(u64::try_from(name.len()).unwrap_or(u64::MAX)))
        .and_then(|value| {
            value.checked_add(u64::try_from(qualified_name.len()).unwrap_or(u64::MAX))
        })
        .ok_or(SqliteStoreError::CountNotRepresentable)?;
    let total = current
        .checked_add(row_bytes)
        .ok_or(SqliteStoreError::CountNotRepresentable)?;
    if total > MAX_REUSABLE_ARTIFACT_BYTES {
        return Err(SqliteStoreError::ArtifactReuseLimitExceeded);
    }
    Ok(total)
}
