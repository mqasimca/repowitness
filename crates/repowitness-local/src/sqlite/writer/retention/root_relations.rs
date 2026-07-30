const RETENTION_RELATION_ROOT_DOMAIN: &[u8] =
    b"RepoWitness\0phase1-generation-retention-relation-root\0";

struct RetentionRootQuery {
    domain: &'static [u8],
    sql: &'static str,
}

const ENFORCED_RETENTION_ROOT_QUERIES: &[RetentionRootQuery] = &[
    RetentionRootQuery {
        domain: b"graph-source-generation",
        sql: "SELECT generation_id, ordinal, source_slot_id, source_generation_id
              FROM generation_graph_sources
              WHERE source_generation_id != generation_id
              ORDER BY generation_id, ordinal",
    },
    RetentionRootQuery {
        domain: b"memory-projection-generation",
        sql: "SELECT workspace_id, index_generation_id
              FROM memory_projection_generations
              ORDER BY workspace_id, index_generation_id",
    },
    RetentionRootQuery {
        domain: b"memory-projection-snapshot",
        sql: "SELECT workspace_id, snapshot_digest
              FROM memory_projection_generations
              ORDER BY workspace_id, snapshot_digest",
    },
    RetentionRootQuery {
        domain: b"memory-version-snapshot",
        sql: "SELECT workspace_id, record_id, revision_digest,
                     validity_source_snapshot
              FROM memory_versions
              WHERE validity_source_snapshot IS NOT NULL
              ORDER BY workspace_id, record_id, revision_digest",
    },
    RetentionRootQuery {
        domain: b"memory-evidence-snapshot",
        sql: "SELECT workspace_id, record_id, revision_digest, ordinal,
                     source_snapshot_digest
              FROM memory_evidence
              ORDER BY workspace_id, record_id, revision_digest, ordinal",
    },
    RetentionRootQuery {
        domain: b"memory-evidence-artifact",
        sql: "SELECT workspace_id, record_id, revision_digest, ordinal,
                     artifact_digest
              FROM memory_evidence
              ORDER BY workspace_id, record_id, revision_digest, ordinal",
    },
    RetentionRootQuery {
        domain: b"memory-audit-snapshot",
        sql: "SELECT workspace_id, record_id, revision_digest, source_revision
              FROM memory_audit
              WHERE source_format = 'source_snapshot'
              ORDER BY workspace_id, record_id, revision_digest,
                       source_revision",
    },
    RetentionRootQuery {
        domain: b"memory-correspondence-source-snapshot",
        sql: "SELECT workspace_id, record_id, revision_digest,
                     evidence_ordinal, source_snapshot_digest,
                     target_snapshot_digest, source_artifact_digest,
                     target_artifact_digest, source_fact_ordinal,
                     target_fact_ordinal
              FROM memory_correspondence_audit
              ORDER BY workspace_id, record_id, revision_digest,
                       evidence_ordinal, source_snapshot_digest,
                       target_snapshot_digest, source_artifact_digest,
                       target_artifact_digest, source_fact_ordinal,
                       target_fact_ordinal",
    },
    RetentionRootQuery {
        domain: b"memory-correspondence-target-snapshot",
        sql: "SELECT workspace_id, record_id, revision_digest,
                     evidence_ordinal, source_snapshot_digest,
                     target_snapshot_digest, source_artifact_digest,
                     target_artifact_digest, source_fact_ordinal,
                     target_fact_ordinal
              FROM memory_correspondence_audit
              ORDER BY workspace_id, record_id, revision_digest,
                       evidence_ordinal, source_snapshot_digest,
                       target_snapshot_digest, source_artifact_digest,
                       target_artifact_digest, source_fact_ordinal,
                       target_fact_ordinal",
    },
    RetentionRootQuery {
        domain: b"memory-correspondence-source-artifact",
        sql: "SELECT workspace_id, record_id, revision_digest,
                     evidence_ordinal, source_snapshot_digest,
                     target_snapshot_digest, source_artifact_digest,
                     target_artifact_digest, source_fact_ordinal,
                     target_fact_ordinal
              FROM memory_correspondence_audit
              ORDER BY workspace_id, record_id, revision_digest,
                       evidence_ordinal, source_snapshot_digest,
                       target_snapshot_digest, source_artifact_digest,
                       target_artifact_digest, source_fact_ordinal,
                       target_fact_ordinal",
    },
    RetentionRootQuery {
        domain: b"memory-correspondence-target-artifact",
        sql: "SELECT workspace_id, record_id, revision_digest,
                     evidence_ordinal, source_snapshot_digest,
                     target_snapshot_digest, source_artifact_digest,
                     target_artifact_digest, source_fact_ordinal,
                     target_fact_ordinal
              FROM memory_correspondence_audit
              ORDER BY workspace_id, record_id, revision_digest,
                       evidence_ordinal, source_snapshot_digest,
                       target_snapshot_digest, source_artifact_digest,
                       target_artifact_digest, source_fact_ordinal,
                       target_fact_ordinal",
    },
    RetentionRootQuery {
        domain: b"memory-projection-evidence-snapshot",
        sql: "SELECT workspace_id, record_id, revision_digest,
                     evidence_ordinal, target_snapshot_digest
              FROM memory_projection_evidence
              WHERE target_snapshot_digest IS NOT NULL
              ORDER BY workspace_id, record_id, revision_digest,
                       evidence_ordinal, target_snapshot_digest",
    },
    RetentionRootQuery {
        domain: b"memory-projection-evidence-artifact",
        sql: "SELECT workspace_id, record_id, revision_digest,
                     evidence_ordinal, target_artifact_digest
              FROM memory_projection_evidence
              WHERE target_artifact_digest IS NOT NULL
              ORDER BY workspace_id, record_id, revision_digest,
                       evidence_ordinal, target_artifact_digest",
    },
    RetentionRootQuery {
        domain: b"memory-projection-candidate-snapshot",
        sql: "SELECT workspace_id, record_id, revision_digest,
                     evidence_ordinal, ordinal, target_snapshot_digest
              FROM memory_projection_candidates
              ORDER BY workspace_id, record_id, revision_digest,
                       evidence_ordinal, ordinal, target_snapshot_digest",
    },
    RetentionRootQuery {
        domain: b"memory-projection-candidate-artifact",
        sql: "SELECT workspace_id, record_id, revision_digest,
                     evidence_ordinal, ordinal, target_artifact_digest
              FROM memory_projection_candidates
              ORDER BY workspace_id, record_id, revision_digest,
                       evidence_ordinal, ordinal, target_artifact_digest",
    },
];

fn hash_enforced_retention_root_relations(
    transaction: &Transaction<'_>,
    policy: &GenerationRetentionPolicy,
    hasher: &mut Sha256,
    budget: &mut RetentionWorkBudget,
    root_count: &mut u64,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), SqliteStoreError> {
    hash_retention_root_query(
        transaction,
        b"retained-generation-floor",
        "WITH ranked AS (
             SELECT slot.source_slot_id, generation.source_epoch,
                    generation.generation_id,
                    row_number() OVER (
                        PARTITION BY slot.source_slot_id
                        ORDER BY generation.source_epoch DESC,
                                 generation.generation_id DESC
                    ) AS retained_rank
             FROM workspace_source_slots AS slot
             JOIN index_generations AS generation
               ON generation.workspace_id = slot.generation_workspace_id
              AND generation.lifecycle_state = 'retained'
         )
         SELECT source_slot_id, source_epoch, generation_id, retained_rank
         FROM ranked
         WHERE retained_rank <= ?1
         ORDER BY source_slot_id, source_epoch DESC, generation_id DESC",
        params![i64::from(
            policy.retained_generations_per_source_slot()
        )],
        hasher,
        budget,
        root_count,
        cancelled,
        deadline,
    )?;
    for query in ENFORCED_RETENTION_ROOT_QUERIES {
        hash_retention_root_query(
            transaction,
            query.domain,
            query.sql,
            params![],
            hasher,
            budget,
            root_count,
            cancelled,
            deadline,
        )?;
    }
    Ok(())
}

#[allow(
    clippy::too_many_arguments,
    reason = "the root domain, SQL, parameters, digest, budget, cancellation, and deadline are explicit trust-boundary inputs"
)]
fn hash_retention_root_query(
    transaction: &Transaction<'_>,
    domain: &[u8],
    sql: &str,
    parameters: impl rusqlite::Params,
    hasher: &mut Sha256,
    budget: &mut RetentionWorkBudget,
    root_count: &mut u64,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), SqliteStoreError> {
    check_retention_control(cancelled, deadline)?;
    let mut statement = transaction
        .prepare(sql)
        .map_err(|error| retention_database_error(error, cancelled, deadline))?;
    let column_count = statement.column_count();
    let mut rows = statement
        .query(parameters)
        .map_err(|error| retention_database_error(error, cancelled, deadline))?;
    while let Some(row) = rows
        .next()
        .map_err(|error| retention_database_error(error, cancelled, deadline))?
    {
        check_retention_control(cancelled, deadline)?;
        record_retention_root(budget, root_count)?;
        hasher.update(RETENTION_RELATION_ROOT_DOMAIN);
        hash_retention_root_bytes(hasher, domain)?;
        hasher.update(
            u64::try_from(column_count)
                .map_err(|_| SqliteStoreError::CountNotRepresentable)?
                .to_be_bytes(),
        );
        for column in 0..column_count {
            let value = row
                .get_ref(column)
                .map_err(|error| retention_database_error(error, cancelled, deadline))?;
            hash_retention_root_value(hasher, value)?;
        }
    }
    Ok(())
}

fn hash_retention_root_value(
    hasher: &mut Sha256,
    value: rusqlite::types::ValueRef<'_>,
) -> Result<(), SqliteStoreError> {
    match value {
        rusqlite::types::ValueRef::Integer(value) => {
            hasher.update([1]);
            hasher.update(value.to_be_bytes());
        }
        rusqlite::types::ValueRef::Blob(value) => {
            hasher.update([2]);
            hash_retention_root_bytes(hasher, value)?;
        }
        rusqlite::types::ValueRef::Null
        | rusqlite::types::ValueRef::Real(_)
        | rusqlite::types::ValueRef::Text(_) => {
            return Err(SqliteStoreError::IntegrityCheckFailed);
        }
    }
    Ok(())
}

fn hash_retention_root_bytes(
    hasher: &mut Sha256,
    bytes: &[u8],
) -> Result<(), SqliteStoreError> {
    hasher.update(
        u64::try_from(bytes.len())
            .map_err(|_| SqliteStoreError::CountNotRepresentable)?
            .to_be_bytes(),
    );
    hasher.update(bytes);
    Ok(())
}
