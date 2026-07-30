struct PersistedGraphArtifactMetadata {
    source_content_digest: Vec<u8>,
    producer_manifest_digest: Vec<u8>,
    configuration_digest: Vec<u8>,
    analysis_schema_digest: Vec<u8>,
    canonicalization_version: i64,
    visited_nodes: i64,
    syntax_error_nodes: i64,
    payload_digest: Option<Vec<u8>>,
    site_profile_version: i64,
    site_count: i64,
    max_observed_depth: i64,
}

struct PersistedGraphSite {
    ordinal: i64,
    kind: String,
    evidence: String,
    occurrence_start: i64,
    occurrence_end: i64,
    target_start: i64,
    target_end: i64,
    raw_target: String,
    enclosing_kind: Option<String>,
    enclosing_name: Option<String>,
    enclosing_qualified_name: Option<String>,
    enclosing_name_start: Option<i64>,
    enclosing_name_end: Option<i64>,
    enclosing_declaration_start: Option<i64>,
    enclosing_declaration_end: Option<i64>,
}

impl WriterState {
    fn verify_graph_artifact(
        &self,
        artifact: &crate::sqlite::graph::PreparedRustGraphArtifact,
        control: WriteControl<'_>,
    ) -> Result<(), SqliteStoreError> {
        check_control(control)?;
        let key = artifact.key();
        let metadata: Option<PersistedGraphArtifactMetadata> = self
            .connection
            .query_row(
                "SELECT base.source_content_digest,
                        base.producer_manifest_digest,
                        base.configuration_digest, base.analysis_schema_digest,
                        base.canonicalization_version, base.visited_nodes,
                        base.syntax_error_nodes, base.payload_digest,
                        graph.site_profile_version, graph.site_count,
                        graph.max_observed_depth
                 FROM analysis_artifacts AS base
                 JOIN rust_graph_artifacts AS graph USING (artifact_digest)
                 WHERE base.artifact_digest = ?1
                   AND base.lifecycle_state = 'complete'
                   AND base.language = 'rust' AND base.fact_count = 0",
                [artifact.artifact_digest().as_bytes().as_slice()],
                |row| {
                    Ok(PersistedGraphArtifactMetadata {
                        source_content_digest: row.get(0)?,
                        producer_manifest_digest: row.get(1)?,
                        configuration_digest: row.get(2)?,
                        analysis_schema_digest: row.get(3)?,
                        canonicalization_version: row.get(4)?,
                        visited_nodes: row.get(5)?,
                        syntax_error_nodes: row.get(6)?,
                        payload_digest: row.get(7)?,
                        site_profile_version: row.get(8)?,
                        site_count: row.get(9)?,
                        max_observed_depth: row.get(10)?,
                    })
                },
            )
            .optional()
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        let Some(metadata) = metadata else {
            return Err(SqliteStoreError::IntegrityCheckFailed);
        };
        if metadata.source_content_digest != key.source_digest().as_bytes()
            || metadata.producer_manifest_digest != key.analyzer_identity().as_bytes()
            || metadata.configuration_digest != key.configuration_identity().as_bytes()
            || metadata.analysis_schema_digest != key.schema_identity().as_bytes()
            || metadata.canonicalization_version != i64::from(*key.canonicalization_version())
            || metadata.visited_nodes != i64::from(artifact.analysis().visited_nodes())
            || metadata.syntax_error_nodes != i64::from(artifact.analysis().syntax_error_nodes())
            || metadata.site_profile_version
                != i64::from(repowitness_analysis::RUST_GRAPH_SITE_PROFILE_VERSION)
            || metadata.site_count != fixed_usize(artifact.analysis().sites().len())?
            || metadata.max_observed_depth != i64::from(artifact.analysis().max_observed_depth())
        {
            return Err(SqliteStoreError::IntegrityCheckFailed);
        }
        self.verify_graph_sites(artifact, control)?;
        match metadata.payload_digest {
            Some(payload) if payload.as_slice() == artifact.payload_digest().as_slice() => {}
            Some(_) => return Err(SqliteStoreError::IntegrityCheckFailed),
            None => {
                check_control(control)?;
                let changed = self
                    .connection
                    .execute(
                        "UPDATE analysis_artifacts SET payload_digest = ?2
                         WHERE artifact_digest = ?1 AND lifecycle_state = 'complete'
                         AND payload_digest IS NULL",
                        params![
                            artifact.artifact_digest().as_bytes().as_slice(),
                            artifact.payload_digest().as_slice()
                        ],
                    )
                    .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
                if changed != 1 {
                    return Err(SqliteStoreError::IntegrityCheckFailed);
                }
            }
        }
        check_control(control)?;
        Ok(())
    }

    fn verify_graph_sites(
        &self,
        artifact: &crate::sqlite::graph::PreparedRustGraphArtifact,
        control: WriteControl<'_>,
    ) -> Result<(), SqliteStoreError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT ordinal, site_kind, extraction_evidence,
                        occurrence_start, occurrence_end, target_start,
                        target_end, raw_target, enclosing_kind, enclosing_name,
                        enclosing_qualified_name, enclosing_name_start,
                        enclosing_name_end, enclosing_declaration_start,
                        enclosing_declaration_end
                 FROM rust_graph_sites WHERE artifact_digest = ?1
                 ORDER BY ordinal",
            )
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        let mut rows = statement
            .query([artifact.artifact_digest().as_bytes().as_slice()])
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        for site in artifact.analysis().sites() {
            check_control(control)?;
            let row = rows
                .next()
                .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?
                .ok_or(SqliteStoreError::IntegrityCheckFailed)?;
            if !persisted_graph_site_matches(row, site)? {
                return Err(SqliteStoreError::IntegrityCheckFailed);
            }
        }
        if rows
            .next()
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?
            .is_some()
        {
            return Err(SqliteStoreError::IntegrityCheckFailed);
        }
        Ok(())
    }

    fn delete_staging_graph_artifact(
        &mut self,
        artifact: repowitness_domain::AnalysisArtifactDigest,
    ) -> Result<(), SqliteStoreError> {
        let transaction = self.transaction()?;
        transaction
            .execute(
                "DELETE FROM rust_graph_sites WHERE artifact_digest = ?1",
                [artifact.as_bytes().as_slice()],
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        transaction
            .execute(
                "DELETE FROM rust_graph_artifacts WHERE artifact_digest = ?1",
                [artifact.as_bytes().as_slice()],
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        transaction
            .execute(
                "DELETE FROM analysis_artifacts
                 WHERE artifact_digest = ?1 AND lifecycle_state = 'staging'",
                [artifact.as_bytes().as_slice()],
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        commit_mutation(transaction)
    }
}

fn persisted_graph_site_matches(
    row: &rusqlite::Row<'_>,
    site: &repowitness_analysis::RustGraphSite,
) -> Result<bool, SqliteStoreError> {
    let enclosing = site.enclosing_definition();
    let expected = (
        i64::from(site.ordinal().get()),
        site.kind().as_str(),
        site.evidence().as_str(),
        fixed_integer(site.occurrence_span().start().get())?,
        fixed_integer(site.occurrence_span().end().get())?,
        fixed_integer(site.target_span().start().get())?,
        fixed_integer(site.target_span().end().get())?,
        site.raw_target(),
        enclosing.map(|value| value.kind().as_str()),
        enclosing.map(|value| value.name()),
        enclosing.map(|value| value.qualified_name()),
        enclosing
            .map(|value| fixed_integer(value.name_span().start().get()))
            .transpose()?,
        enclosing
            .map(|value| fixed_integer(value.name_span().end().get()))
            .transpose()?,
        enclosing
            .map(|value| fixed_integer(value.declaration_span().start().get()))
            .transpose()?,
        enclosing
            .map(|value| fixed_integer(value.declaration_span().end().get()))
            .transpose()?,
    );
    let actual = PersistedGraphSite {
        ordinal: row
            .get(0)
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
        kind: row
            .get(1)
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
        evidence: row
            .get(2)
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
        occurrence_start: row
            .get(3)
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
        occurrence_end: row
            .get(4)
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
        target_start: row
            .get(5)
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
        target_end: row
            .get(6)
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
        raw_target: row
            .get(7)
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
        enclosing_kind: row
            .get(8)
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
        enclosing_name: row
            .get(9)
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
        enclosing_qualified_name: row
            .get(10)
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
        enclosing_name_start: row
            .get(11)
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
        enclosing_name_end: row
            .get(12)
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
        enclosing_declaration_start: row
            .get(13)
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
        enclosing_declaration_end: row
            .get(14)
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
    };
    Ok(actual.ordinal == expected.0
        && actual.kind == expected.1
        && actual.evidence == expected.2
        && actual.occurrence_start == expected.3
        && actual.occurrence_end == expected.4
        && actual.target_start == expected.5
        && actual.target_end == expected.6
        && actual.raw_target == expected.7
        && actual.enclosing_kind.as_deref() == expected.8
        && actual.enclosing_name.as_deref() == expected.9
        && actual.enclosing_qualified_name.as_deref() == expected.10
        && actual.enclosing_name_start == expected.11
        && actual.enclosing_name_end == expected.12
        && actual.enclosing_declaration_start == expected.13
        && actual.enclosing_declaration_end == expected.14)
}
