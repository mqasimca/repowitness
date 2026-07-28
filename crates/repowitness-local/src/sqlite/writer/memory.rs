fn insert_memory_children(
    transaction: &Transaction<'_>,
    workspace_id: i64,
    prepared: &PreparedMemoryImport,
    control: WriteControl<'_>,
) -> Result<(), SqliteStoreError> {
    insert_memory_parents(transaction, workspace_id, prepared, control)?;
    insert_memory_validity(transaction, workspace_id, prepared, control)?;
    insert_memory_evidence(transaction, workspace_id, prepared, control)?;
    insert_memory_relationships(transaction, workspace_id, prepared, control)
}

fn insert_memory_parents(
    transaction: &Transaction<'_>,
    workspace_id: i64,
    prepared: &PreparedMemoryImport,
    control: WriteControl<'_>,
) -> Result<(), SqliteStoreError> {
    let record = &prepared.record;
    let record_id = record.header().record_id();
    for (ordinal, parent) in record.header().parents().iter().enumerate() {
        check_control(control)?;
        transaction
            .execute(
                "INSERT INTO memory_version_parents(
                    workspace_id, record_id, revision_digest, ordinal,
                    parent_revision_digest
                 ) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    workspace_id,
                    record_id.as_bytes().as_slice(),
                    prepared.revision.as_bytes().as_slice(),
                    fixed_usize(ordinal)?,
                    parent.as_bytes().as_slice()
                ],
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    }
    Ok(())
}

fn insert_memory_validity(
    transaction: &Transaction<'_>,
    workspace_id: i64,
    prepared: &PreparedMemoryImport,
    control: WriteControl<'_>,
) -> Result<(), SqliteStoreError> {
    let record = &prepared.record;
    let record_id = record.header().record_id();
    if let MemoryValidity::Commits {
        introduced_by,
        invalidated_by,
    } = record.validity()
    {
        for (side, commits) in [
            ("introduced_by", introduced_by.as_slice()),
            ("invalidated_by", invalidated_by.as_slice()),
        ] {
            for (ordinal, commit) in commits.iter().enumerate() {
                check_control(control)?;
                transaction
                    .execute(
                        "INSERT INTO memory_validity_commits(
                            workspace_id, record_id, revision_digest, side,
                            ordinal, object_format, object_id
                         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                        params![
                            workspace_id,
                            record_id.as_bytes().as_slice(),
                            prepared.revision.as_bytes().as_slice(),
                            side,
                            fixed_usize(ordinal)?,
                            memory_object_format(commit.object_format()),
                            commit.as_bytes()
                        ],
                    )
                    .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
            }
        }
    }
    Ok(())
}

fn insert_memory_evidence(
    transaction: &Transaction<'_>,
    workspace_id: i64,
    prepared: &PreparedMemoryImport,
    control: WriteControl<'_>,
) -> Result<(), SqliteStoreError> {
    let record = &prepared.record;
    let record_id = record.header().record_id();
    for (ordinal, evidence) in record.evidence().iter().enumerate() {
        check_control(control)?;
        let MemoryEvidence::RustSymbol(evidence) = evidence;
        transaction
            .execute(
                "INSERT INTO memory_evidence(
                    workspace_id, record_id, revision_digest, ordinal,
                    evidence_kind, source_snapshot_digest, repository_path,
                    content_digest, artifact_digest, fact_ordinal, symbol_kind,
                    name, qualified_name, name_start, name_length,
                    declaration_start, declaration_length, declaration_digest,
                    producer_id, producer_version
                 ) VALUES (
                    ?1, ?2, ?3, ?4, 'rust_symbol', ?5, ?6, ?7, ?8, ?9,
                    ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19
                 )",
                params![
                    workspace_id,
                    record_id.as_bytes().as_slice(),
                    prepared.revision.as_bytes().as_slice(),
                    fixed_usize(ordinal)?,
                    evidence.source_snapshot().as_bytes().as_slice(),
                    evidence.path().as_bytes(),
                    evidence.content().as_bytes().as_slice(),
                    evidence.artifact().as_bytes().as_slice(),
                    fixed_integer(evidence.fact_ordinal().get())?,
                    rust_memory_symbol_kind(evidence.symbol_kind()),
                    evidence.name().as_str(),
                    evidence.qualified_name().as_str(),
                    fixed_integer(evidence.name_span().start().get())?,
                    fixed_integer(evidence.name_span().len().get())?,
                    fixed_integer(evidence.declaration_span().start().get())?,
                    fixed_integer(evidence.declaration_span().len().get())?,
                    evidence.declaration_digest().as_bytes().as_slice(),
                    evidence.producer().id().as_str(),
                    evidence.producer().version().as_str()
                ],
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    }
    Ok(())
}

fn insert_memory_relationships(
    transaction: &Transaction<'_>,
    workspace_id: i64,
    prepared: &PreparedMemoryImport,
    control: WriteControl<'_>,
) -> Result<(), SqliteStoreError> {
    let record = &prepared.record;
    let record_id = record.header().record_id();
    for (ordinal, relationship) in record.relationships().iter().enumerate() {
        check_control(control)?;
        transaction
            .execute(
                "INSERT INTO memory_relationships(
                    workspace_id, record_id, revision_digest, ordinal,
                    relationship_kind, target_record_id, target_revision_digest
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    workspace_id,
                    record_id.as_bytes().as_slice(),
                    prepared.revision.as_bytes().as_slice(),
                    fixed_usize(ordinal)?,
                    memory_relationship_kind(relationship.kind()),
                    relationship.record_id().as_bytes().as_slice(),
                    relationship.revision_digest().as_bytes().as_slice()
                ],
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    }
    Ok(())
}

fn insert_memory_version(
    transaction: &Transaction<'_>,
    workspace_id: i64,
    prepared: &PreparedMemoryImport,
) -> Result<(), SqliteStoreError> {
    let record = &prepared.record;
    let (validity_kind, validity_source_snapshot) = memory_validity(record.validity());
    let inserted = transaction
        .execute(
            "INSERT INTO memory_versions(
                workspace_id, record_id, revision_digest, schema_version,
                canonical_json, kind, title, body, subject_evidence,
                provenance_origin, authored_actor_kind, authored_actor_id,
                authored_assurance, authored_lifecycle, validity_kind,
                validity_source_snapshot, tombstone
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
                ?13, ?14, ?15, ?16, ?17
             )",
            params![
                workspace_id,
                record.header().record_id().as_bytes().as_slice(),
                prepared.revision.as_bytes().as_slice(),
                i64::from(record.schema_version()),
                prepared.canonical_json.as_slice(),
                memory_kind(record.claim().kind()),
                record.claim().title().as_str(),
                record.claim().body().as_str(),
                fixed_integer(record.scope().subject_evidence().get())?,
                memory_provenance_origin(record.provenance().origin()),
                memory_actor_kind(record.provenance().actor_kind()),
                record.provenance().actor_id().as_str(),
                memory_assurance(record.assurance()),
                memory_lifecycle(record.lifecycle()),
                validity_kind,
                validity_source_snapshot,
                i64::from(record.tombstone())
            ],
        )
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    if inserted != 1 {
        return Err(SqliteStoreError::IntegrityCheckFailed);
    }
    Ok(())
}

fn verify_memory_version(
    transaction: &Transaction<'_>,
    workspace_id: i64,
    prepared: &PreparedMemoryImport,
    control: WriteControl<'_>,
) -> Result<(), SqliteStoreError> {
    check_control(control)?;
    let record = &prepared.record;
    let record_id = record.header().record_id();
    let (validity_kind, validity_source_snapshot) = memory_validity(record.validity());
    let version_matches: i64 = transaction
        .query_row(
            "SELECT EXISTS(
                SELECT 1 FROM memory_versions
                WHERE workspace_id = ?1
                  AND record_id = ?2
                  AND revision_digest = ?3
                  AND schema_version = ?4
                  AND canonical_json = ?5
                  AND kind = ?6
                  AND title = ?7
                  AND body = ?8
                  AND subject_evidence = ?9
                  AND provenance_origin = ?10
                  AND authored_actor_kind = ?11
                  AND authored_actor_id = ?12
                  AND authored_assurance = ?13
                  AND authored_lifecycle = ?14
                  AND validity_kind = ?15
                  AND validity_source_snapshot IS ?16
                  AND tombstone = ?17
             )",
            params![
                workspace_id,
                record_id.as_bytes().as_slice(),
                prepared.revision.as_bytes().as_slice(),
                i64::from(record.schema_version()),
                prepared.canonical_json.as_slice(),
                memory_kind(record.claim().kind()),
                record.claim().title().as_str(),
                record.claim().body().as_str(),
                fixed_integer(record.scope().subject_evidence().get())?,
                memory_provenance_origin(record.provenance().origin()),
                memory_actor_kind(record.provenance().actor_kind()),
                record.provenance().actor_id().as_str(),
                memory_assurance(record.assurance()),
                memory_lifecycle(record.lifecycle()),
                validity_kind,
                validity_source_snapshot,
                i64::from(record.tombstone())
            ],
            |row| row.get(0),
        )
        .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
    require_match(version_matches)?;

    verify_memory_child_count(
        transaction,
        "memory_version_parents",
        workspace_id,
        record_id.as_bytes(),
        prepared.revision.as_bytes(),
        record.header().parents().len(),
    )?;
    for (ordinal, parent) in record.header().parents().iter().enumerate() {
        check_control(control)?;
        let matches: i64 = transaction
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM memory_version_parents
                    WHERE workspace_id = ?1 AND record_id = ?2
                      AND revision_digest = ?3 AND ordinal = ?4
                      AND parent_revision_digest = ?5
                 )",
                params![
                    workspace_id,
                    record_id.as_bytes().as_slice(),
                    prepared.revision.as_bytes().as_slice(),
                    fixed_usize(ordinal)?,
                    parent.as_bytes().as_slice()
                ],
                |row| row.get(0),
            )
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        require_match(matches)?;
    }

    verify_memory_validity(transaction, workspace_id, prepared, control)?;
    verify_memory_evidence(transaction, workspace_id, prepared, control)?;
    verify_memory_relationships(transaction, workspace_id, prepared, control)
}

fn verify_memory_validity(
    transaction: &Transaction<'_>,
    workspace_id: i64,
    prepared: &PreparedMemoryImport,
    control: WriteControl<'_>,
) -> Result<(), SqliteStoreError> {
    let record = &prepared.record;
    let record_id = record.header().record_id();
    let (introduced_by, invalidated_by) = match record.validity() {
        MemoryValidity::Commits {
            introduced_by,
            invalidated_by,
        } => (introduced_by.as_slice(), invalidated_by.as_slice()),
        MemoryValidity::Worktree { .. } => (&[][..], &[][..]),
    };
    let expected_count = introduced_by
        .len()
        .checked_add(invalidated_by.len())
        .ok_or(SqliteStoreError::CountNotRepresentable)?;
    verify_memory_child_count(
        transaction,
        "memory_validity_commits",
        workspace_id,
        record_id.as_bytes(),
        prepared.revision.as_bytes(),
        expected_count,
    )?;
    for (side, commits) in [
        ("introduced_by", introduced_by),
        ("invalidated_by", invalidated_by),
    ] {
        for (ordinal, commit) in commits.iter().enumerate() {
            check_control(control)?;
            let matches: i64 = transaction
                .query_row(
                    "SELECT EXISTS(
                        SELECT 1 FROM memory_validity_commits
                        WHERE workspace_id = ?1 AND record_id = ?2
                          AND revision_digest = ?3 AND side = ?4
                          AND ordinal = ?5 AND object_format = ?6
                          AND object_id = ?7
                     )",
                    params![
                        workspace_id,
                        record_id.as_bytes().as_slice(),
                        prepared.revision.as_bytes().as_slice(),
                        side,
                        fixed_usize(ordinal)?,
                        memory_object_format(commit.object_format()),
                        commit.as_bytes()
                    ],
                    |row| row.get(0),
                )
                .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
            require_match(matches)?;
        }
    }
    Ok(())
}

fn verify_memory_evidence(
    transaction: &Transaction<'_>,
    workspace_id: i64,
    prepared: &PreparedMemoryImport,
    control: WriteControl<'_>,
) -> Result<(), SqliteStoreError> {
    let record = &prepared.record;
    let record_id = record.header().record_id();
    verify_memory_child_count(
        transaction,
        "memory_evidence",
        workspace_id,
        record_id.as_bytes(),
        prepared.revision.as_bytes(),
        record.evidence().len(),
    )?;
    for (ordinal, evidence) in record.evidence().iter().enumerate() {
        check_control(control)?;
        let MemoryEvidence::RustSymbol(evidence) = evidence;
        let matches: i64 = transaction
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM memory_evidence
                    WHERE workspace_id = ?1 AND record_id = ?2
                      AND revision_digest = ?3 AND ordinal = ?4
                      AND evidence_kind = 'rust_symbol'
                      AND source_snapshot_digest = ?5
                      AND repository_path = ?6
                      AND content_digest = ?7
                      AND artifact_digest = ?8
                      AND fact_ordinal = ?9
                      AND symbol_kind = ?10
                      AND name = ?11
                      AND qualified_name = ?12
                      AND name_start = ?13
                      AND name_length = ?14
                      AND declaration_start = ?15
                      AND declaration_length = ?16
                      AND declaration_digest = ?17
                      AND producer_id = ?18
                      AND producer_version = ?19
                 )",
                params![
                    workspace_id,
                    record_id.as_bytes().as_slice(),
                    prepared.revision.as_bytes().as_slice(),
                    fixed_usize(ordinal)?,
                    evidence.source_snapshot().as_bytes().as_slice(),
                    evidence.path().as_bytes(),
                    evidence.content().as_bytes().as_slice(),
                    evidence.artifact().as_bytes().as_slice(),
                    fixed_integer(evidence.fact_ordinal().get())?,
                    rust_memory_symbol_kind(evidence.symbol_kind()),
                    evidence.name().as_str(),
                    evidence.qualified_name().as_str(),
                    fixed_integer(evidence.name_span().start().get())?,
                    fixed_integer(evidence.name_span().len().get())?,
                    fixed_integer(evidence.declaration_span().start().get())?,
                    fixed_integer(evidence.declaration_span().len().get())?,
                    evidence.declaration_digest().as_bytes().as_slice(),
                    evidence.producer().id().as_str(),
                    evidence.producer().version().as_str()
                ],
                |row| row.get(0),
            )
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        require_match(matches)?;
    }
    Ok(())
}

fn verify_memory_relationships(
    transaction: &Transaction<'_>,
    workspace_id: i64,
    prepared: &PreparedMemoryImport,
    control: WriteControl<'_>,
) -> Result<(), SqliteStoreError> {
    let record = &prepared.record;
    let record_id = record.header().record_id();
    verify_memory_child_count(
        transaction,
        "memory_relationships",
        workspace_id,
        record_id.as_bytes(),
        prepared.revision.as_bytes(),
        record.relationships().len(),
    )?;
    for (ordinal, relationship) in record.relationships().iter().enumerate() {
        check_control(control)?;
        let matches: i64 = transaction
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM memory_relationships
                    WHERE workspace_id = ?1 AND record_id = ?2
                      AND revision_digest = ?3 AND ordinal = ?4
                      AND relationship_kind = ?5
                      AND target_record_id = ?6
                      AND target_revision_digest = ?7
                 )",
                params![
                    workspace_id,
                    record_id.as_bytes().as_slice(),
                    prepared.revision.as_bytes().as_slice(),
                    fixed_usize(ordinal)?,
                    memory_relationship_kind(relationship.kind()),
                    relationship.record_id().as_bytes().as_slice(),
                    relationship.revision_digest().as_bytes().as_slice()
                ],
                |row| row.get(0),
            )
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        require_match(matches)?;
    }
    Ok(())
}

fn verify_memory_child_count(
    transaction: &Transaction<'_>,
    table: &'static str,
    workspace_id: i64,
    record_id: &[u8; 16],
    revision: &[u8; 32],
    expected: usize,
) -> Result<(), SqliteStoreError> {
    let sql = format!(
        "SELECT count(*) FROM {table}
         WHERE workspace_id = ?1 AND record_id = ?2 AND revision_digest = ?3"
    );
    let actual: i64 = transaction
        .query_row(
            &sql,
            params![workspace_id, record_id.as_slice(), revision.as_slice()],
            |row| row.get(0),
        )
        .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
    if actual != fixed_usize(expected)? {
        return Err(SqliteStoreError::IntegrityCheckFailed);
    }
    Ok(())
}

fn insert_memory_audit(
    transaction: &Transaction<'_>,
    workspace_id: i64,
    prepared: &PreparedMemoryImport,
    operation: &'static str,
) -> Result<bool, SqliteStoreError> {
    let (source_kind, source_format, source_revision) = memory_observation_source(&prepared.source);
    let record = &prepared.record;
    let sql = match operation {
        "observed" => {
            "INSERT INTO memory_audit(
                workspace_id, record_id, revision_digest, operation,
                trusted_actor_kind, trusted_actor_id, recorded_at_unix_ms,
                source_kind, source_format, source_revision,
                display_revision, presentation_digest
             ) VALUES (
                ?1, ?2, ?3, 'observed', 'local_asserted', ?4, ?5,
                ?6, ?7, ?8, ?9, ?10
             )
             ON CONFLICT(
                workspace_id, record_id, revision_digest, source_kind,
                source_format, source_revision, presentation_digest,
                trusted_actor_kind, trusted_actor_id
             ) WHERE operation = 'observed' DO NOTHING"
        }
        "locally_approved" => {
            "INSERT INTO memory_audit(
                workspace_id, record_id, revision_digest, operation,
                trusted_actor_kind, trusted_actor_id, recorded_at_unix_ms,
                source_kind, source_format, source_revision,
                display_revision, presentation_digest
             ) VALUES (
                ?1, ?2, ?3, 'locally_approved', 'local_asserted', ?4, ?5,
                ?6, ?7, ?8, ?9, ?10
             )
             ON CONFLICT(
                workspace_id, record_id, revision_digest,
                trusted_actor_kind, trusted_actor_id
             ) WHERE operation = 'locally_approved' DO NOTHING"
        }
        _ => return Err(SqliteStoreError::InvalidMemoryImport),
    };
    let inserted = transaction
        .execute(
            sql,
            params![
                workspace_id,
                record.header().record_id().as_bytes().as_slice(),
                prepared.revision.as_bytes().as_slice(),
                prepared.audit_actor.as_str(),
                fixed_integer(prepared.recorded_at.get())?,
                source_kind,
                source_format,
                source_revision,
                i64::from(record.header().display_revision().get()),
                prepared.presentation.as_bytes().as_slice()
            ],
        )
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    Ok(inserted == 1)
}

fn require_match(matches: i64) -> Result<(), SqliteStoreError> {
    if matches == 1 {
        Ok(())
    } else {
        Err(SqliteStoreError::IntegrityCheckFailed)
    }
}

const fn memory_kind(kind: MemoryKind) -> &'static str {
    match kind {
        MemoryKind::Decision => "decision",
        MemoryKind::Failure => "failure",
    }
}

const fn memory_provenance_origin(origin: MemoryProvenanceOrigin) -> &'static str {
    match origin {
        MemoryProvenanceOrigin::Human => "human",
    }
}

const fn memory_actor_kind(kind: MemoryActorKind) -> &'static str {
    match kind {
        MemoryActorKind::LocalAsserted => "local_asserted",
    }
}

const fn memory_assurance(assurance: MemoryAssurance) -> &'static str {
    match assurance {
        MemoryAssurance::LocallyApproved => "locally_approved",
    }
}

const fn memory_lifecycle(lifecycle: MemoryLifecycle) -> &'static str {
    match lifecycle {
        MemoryLifecycle::Active => "active",
        MemoryLifecycle::NeedsReview => "needs_review",
        MemoryLifecycle::Stale => "stale",
        MemoryLifecycle::Contradicted => "contradicted",
        MemoryLifecycle::Superseded => "superseded",
        MemoryLifecycle::Quarantined => "quarantined",
        MemoryLifecycle::Tombstoned => "tombstoned",
    }
}

fn memory_validity(validity: &MemoryValidity) -> (&'static str, Option<&[u8]>) {
    match validity {
        MemoryValidity::Commits { .. } => ("commits", None),
        MemoryValidity::Worktree { source_snapshot } => {
            ("worktree", Some(source_snapshot.as_bytes().as_slice()))
        }
    }
}

const fn memory_object_format(format: MemoryObjectFormat) -> &'static str {
    match format {
        MemoryObjectFormat::Sha1 => "sha1",
        MemoryObjectFormat::Sha256 => "sha256",
    }
}

const fn rust_memory_symbol_kind(kind: RustMemorySymbolKind) -> &'static str {
    match kind {
        RustMemorySymbolKind::Function => "function",
        RustMemorySymbolKind::Method => "method",
        RustMemorySymbolKind::Struct => "struct",
        RustMemorySymbolKind::Enum => "enum",
        RustMemorySymbolKind::Union => "union",
        RustMemorySymbolKind::Trait => "trait",
        RustMemorySymbolKind::Module => "module",
        RustMemorySymbolKind::TypeAlias => "type_alias",
        RustMemorySymbolKind::Constant => "constant",
        RustMemorySymbolKind::Static => "static",
        RustMemorySymbolKind::Macro => "macro",
    }
}

const fn memory_relationship_kind(kind: MemoryRelationshipKind) -> &'static str {
    match kind {
        MemoryRelationshipKind::Contradicts => "contradicts",
        MemoryRelationshipKind::Supersedes => "supersedes",
    }
}

fn memory_observation_source(
    source: &MemoryObservationSource,
) -> (&'static str, &'static str, &[u8]) {
    match source {
        MemoryObservationSource::Git(commit) => (
            "git",
            memory_object_format(commit.object_format()),
            commit.as_bytes(),
        ),
        MemoryObservationSource::Worktree(snapshot) => (
            "worktree",
            "source_snapshot",
            snapshot.as_bytes().as_slice(),
        ),
    }
}
