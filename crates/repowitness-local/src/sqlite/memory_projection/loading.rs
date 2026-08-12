#[allow(
    clippy::too_many_lines,
    reason = "one bounded read verifies normalized audit and canonical-record integrity before returning any journal"
)]
fn load_memory_journal_inner(
    connection: &Connection,
    repository: RepositoryIdentityDigest,
    limits: MemoryProjectionLoadLimits,
    control: WriteControl<'_>,
) -> Result<LoadedMemoryJournal, SqliteStoreError> {
    check_control(control)?;
    let source = load_active_source(connection, repository)?;
    let query_limit = i64::from(limits.max_versions()) + 1;
    let raw_versions = {
        let mut statement = connection
            .prepare(
                "SELECT version.record_id, version.revision_digest, version.canonical_json,
                        (
                            SELECT audit.display_revision
                            FROM memory_audit_all AS audit
                            WHERE audit.workspace_id = version.workspace_id
                              AND audit.record_id = version.record_id
                              AND audit.revision_digest = version.revision_digest
                            ORDER BY
                              CASE audit.operation WHEN 'locally_approved' THEN 0 ELSE 1 END,
                              audit.event_id
                            LIMIT 1
                        ),
                        EXISTS (
                            SELECT 1
                            FROM memory_current_trust AS audit
                            WHERE audit.workspace_id = version.workspace_id
                              AND audit.record_id = version.record_id
                              AND audit.revision_digest = version.revision_digest
                        )
                 FROM memory_versions_all AS version
                 WHERE version.workspace_id = ?1
                 ORDER BY version.record_id, version.revision_digest
                 LIMIT ?2",
            )
            .map_err(|_| control_database_error(control))?;
        let rows = statement
            .query_map(params![source.workspace_id, query_limit], |row| {
                Ok(RawMemoryVersion {
                    record_id: row.get(0)?,
                    revision: row.get(1)?,
                    canonical_json: row.get(2)?,
                    display_revision: row.get(3)?,
                    locally_approved: row.get(4)?,
                })
            })
            .map_err(|_| control_database_error(control))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|_| control_database_error(control))?
    };
    if raw_versions.len()
        > usize::try_from(limits.max_versions())
            .map_err(|_| SqliteStoreError::CountNotRepresentable)?
    {
        return Err(SqliteStoreError::MemoryProjectionLimitExceeded);
    }

    let mut canonical_bytes = 0_u64;
    let mut versions = Vec::with_capacity(raw_versions.len());
    for raw in raw_versions {
        check_control(control)?;
        canonical_bytes = canonical_bytes
            .checked_add(
                u64::try_from(raw.canonical_json.len())
                    .map_err(|_| SqliteStoreError::CountNotRepresentable)?,
            )
            .ok_or(SqliteStoreError::CountNotRepresentable)?;
        if canonical_bytes > limits.max_canonical_bytes() {
            return Err(SqliteStoreError::MemoryProjectionLimitExceeded);
        }
        let record_id = memory_record_id(&raw.record_id)?;
        let revision = CanonicalMemoryDigest::try_from_slice(&raw.revision)
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        let display_revision = u32::try_from(raw.display_revision)
            .ok()
            .and_then(|value| MemoryDisplayRevision::try_new(value).ok())
            .ok_or(SqliteStoreError::IntegrityCheckFailed)?;
        let parsed = parse_persisted_canonical_memory_record(
            &raw.canonical_json,
            display_revision,
            revision,
            MemoryFormatControl::new(control.cancelled.as_ref(), control.deadline),
        )
        .map_err(|error| match error {
            crate::MemoryFormatError::Cancelled => SqliteStoreError::Cancelled,
            crate::MemoryFormatError::DeadlineExceeded => SqliteStoreError::DeadlineExceeded,
            _ => SqliteStoreError::IntegrityCheckFailed,
        })?;
        if parsed.record().header().record_id() != record_id
            || parsed.record().scope().repository() != repository
        {
            return Err(SqliteStoreError::IntegrityCheckFailed);
        }
        let approval_git_source = if raw.locally_approved {
            unique_approval_git_source(
                connection,
                source.workspace_id,
                record_id,
                revision,
                control,
            )?
        } else {
            None
        };
        versions.push(LoadedMemoryVersion {
            revision,
            record: parsed.into_record(),
            locally_approved: raw.locally_approved,
            approval_git_source,
        });
    }
    Ok(LoadedMemoryJournal { source, versions })
}

pub(super) fn load_active_source(
    connection: &Connection,
    repository: RepositoryIdentityDigest,
) -> Result<MemoryProjectionSource, SqliteStoreError> {
    let row = connection
        .query_row(
            "SELECT workspace.workspace_id, generation.generation_id,
                    generation.source_epoch, generation.snapshot_digest,
                    snapshot.git_state_digest, generation.searched_count,
                    generation.skipped_count, generation.unresolved_count,
                    generation.truncated_count
             FROM workspaces AS workspace
             JOIN index_generations AS generation
               ON generation.generation_id = workspace.active_generation_id
              AND generation.workspace_id = workspace.workspace_id
             JOIN source_snapshots AS snapshot
               ON snapshot.snapshot_digest = generation.snapshot_digest
              AND snapshot.repository_identity = workspace.repository_identity
             WHERE workspace.repository_identity = ?1
               AND generation.lifecycle_state = 'active'
               AND snapshot.lifecycle_state = 'complete'",
            [repository.as_bytes().as_slice()],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, Vec<u8>>(3)?,
                    row.get::<_, Vec<u8>>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, i64>(8)?,
                ))
            },
        )
        .optional()
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?
        .ok_or(SqliteStoreError::GenerationUnavailable)?;
    Ok(MemoryProjectionSource {
        repository,
        workspace_id: row.0,
        generation: GenerationId::from_database(row.1),
        source_epoch: nonnegative(row.2)?,
        snapshot: SourceSnapshotDigest::try_from_slice(&row.3)
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
        git_state: GitStateDigest::try_from_slice(&row.4)
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
        searched_count: nonnegative(row.5)?,
        skipped_count: nonnegative(row.6)?,
        unresolved_count: nonnegative(row.7)?,
        truncated_count: nonnegative(row.8)?,
    })
}

pub(super) fn require_current_write_source(
    connection: &Connection,
    source: MemoryProjectionSource,
    control: WriteControl<'_>,
) -> Result<(), SqliteStoreError> {
    check_control(control)?;
    let is_current = connection
        .query_row(
            "SELECT EXISTS(
                SELECT 1
                FROM workspaces
                WHERE workspace_id = ?1
                  AND repository_identity = ?2
                  AND source_epoch = ?3
            )",
            params![
                source.workspace_id,
                source.repository.as_bytes().as_slice(),
                fixed_integer(source.source_epoch)?,
            ],
            |row| row.get::<_, bool>(0),
        )
        .map_err(|_| control_database_error(control))?;
    if !is_current {
        return Err(SqliteStoreError::StaleSourceEpoch);
    }
    Ok(())
}

fn unique_approval_git_source(
    connection: &Connection,
    workspace_id: i64,
    record_id: MemoryRecordId,
    revision: CanonicalMemoryDigest,
    control: WriteControl<'_>,
) -> Result<Option<MemoryCommitId>, SqliteStoreError> {
    let mut statement = connection
        .prepare(
            "SELECT source_kind, source_format, source_revision
             FROM memory_current_trust
             WHERE workspace_id = ?1
               AND record_id = ?2
               AND revision_digest = ?3
             GROUP BY source_kind, source_format, source_revision
             ORDER BY source_kind, source_format, source_revision
             LIMIT 2",
        )
        .map_err(|_| control_database_error(control))?;
    let rows = statement
        .query_map(
            params![
                workspace_id,
                record_id.as_bytes().as_slice(),
                revision.as_bytes().as_slice()
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                ))
            },
        )
        .map_err(|_| control_database_error(control))?;
    let distinct = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| control_database_error(control))?;
    if distinct.is_empty() {
        return Err(SqliteStoreError::IntegrityCheckFailed);
    }
    if distinct.len() != 1 || distinct[0].0 != "git" {
        return Ok(None);
    }
    decode_commit(&distinct[0].1, &distinct[0].2).map(Some)
}

fn load_rust_candidates_inner(
    connection: &Connection,
    source: MemoryProjectionSource,
    evidence: &RustSymbolMemoryEvidence,
    control: WriteControl<'_>,
) -> Result<LoadedRustCandidateSet, SqliteStoreError> {
    let subject_name_elided = load_subject_fingerprint(connection, evidence, control)?
        .map(|fingerprint| fingerprint.name_elided());
    let kind = analysis_symbol_kind(evidence.symbol_kind());
    let container = symbol_container(evidence.name().as_str(), evidence.qualified_name().as_str())?;
    let candidate_count_before_limit = candidate_count(
        connection,
        source,
        evidence,
        kind,
        container,
        subject_name_elided,
        control,
    )?;
    if candidate_count_before_limit
        > u64::try_from(MAX_RUST_CORRESPONDENCE_CANDIDATES)
            .map_err(|_| SqliteStoreError::CountNotRepresentable)?
    {
        return Ok(LoadedRustCandidateSet {
            subject_name_elided,
            candidates: Vec::new(),
            candidate_count_before_limit,
        });
    }
    let candidates = read_candidates(
        connection,
        source,
        evidence,
        kind,
        container,
        subject_name_elided,
        candidate_count_before_limit,
        control,
    )?;
    Ok(LoadedRustCandidateSet {
        subject_name_elided,
        candidates,
        candidate_count_before_limit,
    })
}

#[allow(
    clippy::too_many_arguments,
    reason = "the complete candidate predicate keeps every semantic input explicit"
)]
fn candidate_count(
    connection: &Connection,
    source: MemoryProjectionSource,
    evidence: &RustSymbolMemoryEvidence,
    kind: RustSymbolKind,
    container: &str,
    name_elided: Option<CorrespondenceFingerprintDigest>,
    control: WriteControl<'_>,
) -> Result<u64, SqliteStoreError> {
    let value = connection
        .query_row(
            CANDIDATE_COUNT_SQL,
            candidate_params(source, evidence, kind, container, name_elided),
            |row| row.get::<_, i64>(0),
        )
        .map_err(|_| control_database_error(control))?;
    nonnegative(value)
}

#[allow(
    clippy::too_many_arguments,
    reason = "the complete candidate predicate keeps every semantic input explicit"
)]
fn read_candidates(
    connection: &Connection,
    source: MemoryProjectionSource,
    evidence: &RustSymbolMemoryEvidence,
    kind: RustSymbolKind,
    container: &str,
    name_elided: Option<CorrespondenceFingerprintDigest>,
    expected_count: u64,
    control: WriteControl<'_>,
) -> Result<Vec<RustCorrespondenceCandidate>, SqliteStoreError> {
    let mut statement = connection
        .prepare(CANDIDATE_ROWS_SQL)
        .map_err(|_| control_database_error(control))?;
    let mut rows = statement
        .query(candidate_params(
            source,
            evidence,
            kind,
            container,
            name_elided,
        ))
        .map_err(|_| control_database_error(control))?;
    let capacity =
        usize::try_from(expected_count).map_err(|_| SqliteStoreError::CountNotRepresentable)?;
    let mut candidates = Vec::with_capacity(capacity);
    while let Some(row) = rows.next().map_err(|_| control_database_error(control))? {
        check_control(control)?;
        if candidates.len() >= capacity {
            return Err(SqliteStoreError::IntegrityCheckFailed);
        }
        let path_bytes = row
            .get::<_, Vec<u8>>(0)
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        let path = RepositoryPath::try_from_bytes(&path_bytes, PERSISTED_PATH_LIMITS)
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        let artifact_bytes = row
            .get::<_, Vec<u8>>(1)
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        let artifact = AnalysisArtifactDigest::try_from_slice(&artifact_bytes)
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        let ordinal = nonnegative(
            row.get::<_, i64>(2)
                .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
        )?;
        let persisted_kind = row
            .get::<_, String>(3)
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        let persisted_kind = RustSymbolKind::from_stable_str(&persisted_kind)
            .ok_or(SqliteStoreError::IntegrityCheckFailed)?;
        let name = row
            .get::<_, String>(4)
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        let qualified_name = row
            .get::<_, String>(5)
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        let name_span = persisted_span(
            row.get(6)
                .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
            row.get(7)
                .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
        )?;
        let declaration_span = persisted_span(
            row.get(8)
                .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
            row.get(9)
                .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
        )?;
        let declaration_bytes = row
            .get::<_, Vec<u8>>(10)
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        let name_elided_bytes = row
            .get::<_, Vec<u8>>(11)
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        let fact = RustSymbolFact::try_new_with_correspondence(
            persisted_kind,
            name,
            qualified_name,
            name_span,
            declaration_span,
            RustOccurrenceFingerprint::new(
                DeclarationDigest::try_from_slice(&declaration_bytes)
                    .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
                CorrespondenceFingerprintDigest::try_from_slice(&name_elided_bytes)
                    .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
            ),
            RustAnalysisLimits::default(),
        )
        .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        let candidate = RustCorrespondenceCandidate::try_from_fact(
            path,
            artifact,
            ordinal,
            &fact,
            RustPathContinuity::None,
        )
        .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        candidates.push(candidate);
    }
    if candidates.len() != capacity {
        return Err(SqliteStoreError::IntegrityCheckFailed);
    }
    Ok(candidates)
}

fn load_subject_fingerprint(
    connection: &Connection,
    evidence: &RustSymbolMemoryEvidence,
    control: WriteControl<'_>,
) -> Result<Option<RustOccurrenceFingerprint>, SqliteStoreError> {
    let row = connection
        .query_row(
            "SELECT declaration_digest, name_elided_digest
             FROM artifact_fact_correspondence
             WHERE artifact_digest = ?1
               AND fact_ordinal = ?2
               AND profile_id = ?3
               AND profile_version = ?4",
            params![
                evidence.artifact().as_bytes().as_slice(),
                i64::try_from(evidence.fact_ordinal().get())
                    .map_err(|_| SqliteStoreError::CountNotRepresentable)?,
                RUST_CORRESPONDENCE_PROFILE_ID,
                i64::from(RUST_CORRESPONDENCE_PROFILE_VERSION)
            ],
            |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?)),
        )
        .optional()
        .map_err(|_| control_database_error(control))?;
    let Some((declaration, name_elided)) = row else {
        return Ok(None);
    };
    let fingerprint = RustOccurrenceFingerprint::new(
        DeclarationDigest::try_from_slice(&declaration)
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
        CorrespondenceFingerprintDigest::try_from_slice(&name_elided)
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
    );
    if fingerprint.declaration() != evidence.declaration_digest() {
        return Err(SqliteStoreError::IntegrityCheckFailed);
    }
    Ok(Some(fingerprint))
}

fn candidate_params<'a>(
    source: MemoryProjectionSource,
    evidence: &'a RustSymbolMemoryEvidence,
    kind: RustSymbolKind,
    container: &'a str,
    name_elided: Option<CorrespondenceFingerprintDigest>,
) -> [rusqlite::types::Value; 7] {
    [
        source.generation.get().into(),
        kind.as_str().to_owned().into(),
        evidence.path().as_bytes().to_vec().into(),
        evidence.declaration_digest().as_bytes().to_vec().into(),
        name_elided.map_or(rusqlite::types::Value::Null, |digest| {
            digest.as_bytes().to_vec().into()
        }),
        container.to_owned().into(),
        i64::from(RUST_CORRESPONDENCE_PROFILE_VERSION).into(),
    ]
}

fn symbol_container<'a>(name: &str, qualified_name: &'a str) -> Result<&'a str, SqliteStoreError> {
    if qualified_name == name {
        return Ok("");
    }
    qualified_name
        .strip_suffix(name)
        .and_then(|prefix| prefix.strip_suffix("::"))
        .filter(|container| !container.is_empty())
        .ok_or(SqliteStoreError::IntegrityCheckFailed)
}

macro_rules! candidate_base_sql {
    () => {
        "
        FROM generation_files AS file
        JOIN analysis_artifacts AS artifact
          ON artifact.artifact_digest = file.artifact_digest
        JOIN artifact_facts AS fact
          ON fact.artifact_digest = file.artifact_digest
        JOIN artifact_fact_correspondence AS correspondence
          ON correspondence.artifact_digest = fact.artifact_digest
         AND correspondence.fact_ordinal = fact.ordinal
         AND correspondence.profile_id = 'rust-name-elided'
         AND correspondence.profile_version = ?7
        WHERE file.generation_id = ?1
          AND artifact.lifecycle_state = 'complete'
          AND artifact.language = 'rust'
          AND fact.kind = ?2
          AND (
              file.repository_path = ?3
              OR correspondence.declaration_digest = ?4
              OR (?5 IS NOT NULL AND correspondence.name_elided_digest = ?5)
              OR (
                  (?6 = '' AND fact.qualified_name = fact.name)
                  OR (?6 != '' AND fact.qualified_name = ?6 || '::' || fact.name)
              )
          )"
    };
}

const CANDIDATE_COUNT_SQL: &str = concat!("SELECT count(*) ", candidate_base_sql!());
const CANDIDATE_ROWS_SQL: &str = concat!(
    "SELECT file.repository_path, file.artifact_digest, fact.ordinal,
            fact.kind, fact.name, fact.qualified_name,
            fact.name_start, fact.name_end,
            fact.declaration_start, fact.declaration_end,
            correspondence.declaration_digest,
            correspondence.name_elided_digest ",
    candidate_base_sql!(),
    " ORDER BY file.repository_path, file.artifact_digest, fact.ordinal"
);
