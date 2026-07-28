fn canonical_bytes(record: &ValidatedMemoryRecord) -> Result<Vec<u8>, StrictMemoryError> {
    let record = &record.0;
    let semantic = CanonicalMemoryRecord {
        schema_version: record.schema_version,
        record_id: &record.record_id,
        parent_revision_digests: &record.parent_revision_digests,
        kind: record.kind,
        title: &record.title,
        body: &record.body,
        scope: &record.scope,
        provenance: &record.provenance,
        assurance: record.assurance,
        lifecycle: record.lifecycle,
        validity: &record.validity,
        evidence: &record.evidence,
        relationships: &record.relationships,
        tombstone: record.tombstone,
    };
    let bytes = serde_json_canonicalizer::to_vec(&semantic)
        .map_err(|_| StrictMemoryError::CanonicalizationFailed)?;
    if bytes.len() > MAX_CANONICAL_BYTES {
        return Err(StrictMemoryError::CanonicalizationFailed);
    }
    Ok(bytes)
}

fn canonical_digest(
    record: &ValidatedMemoryRecord,
) -> Result<CanonicalMemoryDigest, StrictMemoryError> {
    let canonical = canonical_bytes(record)?;
    digest_canonical_bytes(&canonical)
}

fn digest_canonical_bytes(canonical: &[u8]) -> Result<CanonicalMemoryDigest, StrictMemoryError> {
    let length =
        u64::try_from(canonical.len()).map_err(|_| StrictMemoryError::CanonicalizationFailed)?;
    let mut hasher = Sha256::new();
    hasher.update(b"RepoWitness\0memory-record\0");
    hasher.update(1_u32.to_be_bytes());
    hasher.update(length.to_be_bytes());
    hasher.update(canonical);
    Ok(CanonicalMemoryDigest::new(hasher.finalize().into()))
}

#[allow(clippy::too_many_lines)]
fn generated_yaml(record: &ValidatedMemoryRecord) -> Result<Vec<u8>, StrictMemoryError> {
    let record = &record.0;
    let mut output = String::new();
    writeln!(output, "schema_version: {}", record.schema_version)
        .map_err(|_| StrictMemoryError::GenerationFailed)?;
    writeln!(output, "record_id: {}", record.record_id)
        .map_err(|_| StrictMemoryError::GenerationFailed)?;
    writeln!(output, "display_revision: {}", record.display_revision)
        .map_err(|_| StrictMemoryError::GenerationFailed)?;
    if record.parent_revision_digests.is_empty() {
        writeln!(output, "parent_revision_digests: []")
            .map_err(|_| StrictMemoryError::GenerationFailed)?;
    } else {
        writeln!(output, "parent_revision_digests:")
            .map_err(|_| StrictMemoryError::GenerationFailed)?;
        for digest in &record.parent_revision_digests {
            writeln!(output, "  - {}", yaml_quoted(digest)?)
                .map_err(|_| StrictMemoryError::GenerationFailed)?;
        }
    }
    writeln!(output, "kind: {}", memory_kind_text(record.kind))
        .map_err(|_| StrictMemoryError::GenerationFailed)?;
    writeln!(output, "title: {}", yaml_quoted(&record.title)?)
        .map_err(|_| StrictMemoryError::GenerationFailed)?;
    writeln!(output, "body: {}", yaml_quoted(&record.body)?)
        .map_err(|_| StrictMemoryError::GenerationFailed)?;
    writeln!(output, "scope:").map_err(|_| StrictMemoryError::GenerationFailed)?;
    writeln!(output, "  repository_id: {}", record.scope.repository_id)
        .map_err(|_| StrictMemoryError::GenerationFailed)?;
    writeln!(
        output,
        "  subject_evidence: {}",
        record.scope.subject_evidence
    )
    .map_err(|_| StrictMemoryError::GenerationFailed)?;
    writeln!(output, "provenance:").map_err(|_| StrictMemoryError::GenerationFailed)?;
    writeln!(
        output,
        "  origin: {}",
        provenance_origin_text(record.provenance.origin)
    )
    .map_err(|_| StrictMemoryError::GenerationFailed)?;
    writeln!(
        output,
        "  actor_kind: {}",
        actor_kind_text(record.provenance.actor_kind)
    )
    .map_err(|_| StrictMemoryError::GenerationFailed)?;
    writeln!(
        output,
        "  actor_id: {}",
        yaml_quoted(&record.provenance.actor_id)?
    )
    .map_err(|_| StrictMemoryError::GenerationFailed)?;
    writeln!(output, "assurance: {}", assurance_text(record.assurance))
        .map_err(|_| StrictMemoryError::GenerationFailed)?;
    writeln!(output, "lifecycle: {}", lifecycle_text(record.lifecycle))
        .map_err(|_| StrictMemoryError::GenerationFailed)?;
    writeln!(output, "validity:").map_err(|_| StrictMemoryError::GenerationFailed)?;
    match &record.validity {
        ValidityDto::Commits {
            introduced_by,
            invalidated_by,
        } => {
            writeln!(output, "  kind: commits").map_err(|_| StrictMemoryError::GenerationFailed)?;
            writeln!(output, "  introduced_by:")
                .map_err(|_| StrictMemoryError::GenerationFailed)?;
            write_commit_ids(&mut output, introduced_by)?;
            if invalidated_by.is_empty() {
                writeln!(output, "  invalidated_by: []")
                    .map_err(|_| StrictMemoryError::GenerationFailed)?;
            } else {
                writeln!(output, "  invalidated_by:")
                    .map_err(|_| StrictMemoryError::GenerationFailed)?;
                write_commit_ids(&mut output, invalidated_by)?;
            }
        }
        ValidityDto::Worktree {
            source_snapshot_digest,
        } => {
            writeln!(output, "  kind: worktree")
                .map_err(|_| StrictMemoryError::GenerationFailed)?;
            writeln!(
                output,
                "  source_snapshot_digest: {}",
                yaml_quoted(source_snapshot_digest)?
            )
            .map_err(|_| StrictMemoryError::GenerationFailed)?;
        }
    }
    writeln!(output, "evidence:").map_err(|_| StrictMemoryError::GenerationFailed)?;
    for evidence in &record.evidence {
        writeln!(output, "  - kind: {}", evidence_kind_text(evidence.kind))
            .map_err(|_| StrictMemoryError::GenerationFailed)?;
        writeln!(
            output,
            "    source_snapshot_digest: {}",
            yaml_quoted(&evidence.source_snapshot_digest)?
        )
        .map_err(|_| StrictMemoryError::GenerationFailed)?;
        writeln!(output, "    path: {}", evidence.path)
            .map_err(|_| StrictMemoryError::GenerationFailed)?;
        writeln!(
            output,
            "    content_digest: {}",
            yaml_quoted(&evidence.content_digest)?
        )
        .map_err(|_| StrictMemoryError::GenerationFailed)?;
        writeln!(
            output,
            "    artifact_digest: {}",
            yaml_quoted(&evidence.artifact_digest)?
        )
        .map_err(|_| StrictMemoryError::GenerationFailed)?;
        writeln!(output, "    fact_ordinal: {}", evidence.fact_ordinal)
            .map_err(|_| StrictMemoryError::GenerationFailed)?;
        writeln!(
            output,
            "    symbol_kind: {}",
            symbol_kind_text(evidence.symbol_kind)
        )
        .map_err(|_| StrictMemoryError::GenerationFailed)?;
        writeln!(output, "    name: {}", yaml_quoted(&evidence.name)?)
            .map_err(|_| StrictMemoryError::GenerationFailed)?;
        writeln!(
            output,
            "    qualified_name: {}",
            yaml_quoted(&evidence.qualified_name)?
        )
        .map_err(|_| StrictMemoryError::GenerationFailed)?;
        writeln!(output, "    name_start: {}", evidence.name_start)
            .map_err(|_| StrictMemoryError::GenerationFailed)?;
        writeln!(output, "    name_length: {}", evidence.name_length)
            .map_err(|_| StrictMemoryError::GenerationFailed)?;
        writeln!(
            output,
            "    declaration_start: {}",
            evidence.declaration_start
        )
        .map_err(|_| StrictMemoryError::GenerationFailed)?;
        writeln!(
            output,
            "    declaration_length: {}",
            evidence.declaration_length
        )
        .map_err(|_| StrictMemoryError::GenerationFailed)?;
        writeln!(
            output,
            "    declaration_digest: {}",
            yaml_quoted(&evidence.declaration_digest)?
        )
        .map_err(|_| StrictMemoryError::GenerationFailed)?;
        writeln!(
            output,
            "    producer_id: {}",
            yaml_quoted(&evidence.producer_id)?
        )
        .map_err(|_| StrictMemoryError::GenerationFailed)?;
        writeln!(
            output,
            "    producer_version: {}",
            yaml_quoted(&evidence.producer_version)?
        )
        .map_err(|_| StrictMemoryError::GenerationFailed)?;
    }
    if record.relationships.is_empty() {
        writeln!(output, "relationships: []").map_err(|_| StrictMemoryError::GenerationFailed)?;
    } else {
        writeln!(output, "relationships:").map_err(|_| StrictMemoryError::GenerationFailed)?;
        for relationship in &record.relationships {
            writeln!(
                output,
                "  - kind: {}",
                relationship_kind_text(relationship.kind)
            )
            .map_err(|_| StrictMemoryError::GenerationFailed)?;
            writeln!(output, "    record_id: {}", relationship.record_id)
                .map_err(|_| StrictMemoryError::GenerationFailed)?;
            writeln!(
                output,
                "    revision_digest: {}",
                yaml_quoted(&relationship.revision_digest)?
            )
            .map_err(|_| StrictMemoryError::GenerationFailed)?;
        }
    }
    writeln!(output, "tombstone: {}", record.tombstone)
        .map_err(|_| StrictMemoryError::GenerationFailed)?;

    let output = output.into_bytes();
    if output.len() > MAX_INPUT_BYTES {
        return Err(StrictMemoryError::GenerationFailed);
    }
    Ok(output)
}

fn write_commit_ids(output: &mut String, commits: &[CommitIdDto]) -> Result<(), StrictMemoryError> {
    for commit in commits {
        writeln!(
            output,
            "    - object_format: {}",
            object_format_text(commit.object_format)
        )
        .map_err(|_| StrictMemoryError::GenerationFailed)?;
        writeln!(
            output,
            "      object_id: {}",
            yaml_quoted(&commit.object_id)?
        )
        .map_err(|_| StrictMemoryError::GenerationFailed)?;
    }
    Ok(())
}

fn yaml_quoted(value: &str) -> Result<String, StrictMemoryError> {
    const HEX: &[u8; 16] = b"0123456789abcdef";

    let mut quoted = String::with_capacity(value.len().saturating_add(2));
    quoted.push('"');
    for character in value.chars() {
        match character {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            '\u{8}' => quoted.push_str("\\b"),
            '\u{c}' => quoted.push_str("\\f"),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            '\u{2028}' => quoted.push_str("\\u2028"),
            '\u{2029}' => quoted.push_str("\\u2029"),
            control if control <= '\u{1f}' || ('\u{7f}'..='\u{9f}').contains(&control) => {
                let value = control as usize;
                quoted.push_str("\\u00");
                quoted.push(char::from(HEX[value >> 4]));
                quoted.push(char::from(HEX[value & 0x0f]));
            }
            ordinary => quoted.push(ordinary),
        }
    }
    quoted.push('"');
    Ok(quoted)
}

fn memory_kind_text(kind: MemoryKind) -> &'static str {
    match kind {
        MemoryKind::Decision => "decision",
        MemoryKind::Failure => "failure",
    }
}

fn provenance_origin_text(origin: ProvenanceOrigin) -> &'static str {
    match origin {
        ProvenanceOrigin::Human => "human",
    }
}

fn actor_kind_text(kind: ActorKind) -> &'static str {
    match kind {
        ActorKind::LocalAsserted => "local_asserted",
    }
}

fn assurance_text(assurance: Assurance) -> &'static str {
    match assurance {
        Assurance::LocallyApproved => "locally_approved",
    }
}

fn lifecycle_text(lifecycle: Lifecycle) -> &'static str {
    match lifecycle {
        Lifecycle::Active => "active",
        Lifecycle::NeedsReview => "needs_review",
        Lifecycle::Stale => "stale",
        Lifecycle::Contradicted => "contradicted",
        Lifecycle::Superseded => "superseded",
        Lifecycle::Quarantined => "quarantined",
        Lifecycle::Tombstoned => "tombstoned",
    }
}

fn object_format_text(format: ObjectFormat) -> &'static str {
    match format {
        ObjectFormat::Sha1 => "sha1",
        ObjectFormat::Sha256 => "sha256",
    }
}

fn evidence_kind_text(kind: EvidenceKind) -> &'static str {
    match kind {
        EvidenceKind::RustSymbol => "rust_symbol",
    }
}

fn symbol_kind_text(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::Function => "function",
        SymbolKind::Method => "method",
        SymbolKind::Struct => "struct",
        SymbolKind::Enum => "enum",
        SymbolKind::Union => "union",
        SymbolKind::Trait => "trait",
        SymbolKind::Module => "module",
        SymbolKind::TypeAlias => "type_alias",
        SymbolKind::Constant => "constant",
        SymbolKind::Static => "static",
        SymbolKind::Macro => "macro",
    }
}

fn relationship_kind_text(kind: RelationshipKind) -> &'static str {
    match kind {
        RelationshipKind::Contradicts => "contradicts",
        RelationshipKind::Supersedes => "supersedes",
    }
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}
