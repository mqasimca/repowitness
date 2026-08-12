/// Emits the deterministic human-facing YAML presentation for an accepted profile.
#[allow(
    clippy::too_many_lines,
    reason = "the fixed schema order is intentionally visible in one writer"
)]
pub fn generate_memory_yaml(
    record: &MemoryRecord,
    control: MemoryFormatControl<'_>,
) -> Result<Vec<u8>, MemoryFormatError> {
    check_control(control)?;
    let record = domain_into_dto(record)?;
    let mut output = MemoryYamlOutput::default();
    write_yaml_line(
        &mut output,
        format_args!("schema_version: {}", record.schema_version),
    )?;
    write_yaml_line(&mut output, format_args!("record_id: {}", record.record_id))?;
    write_yaml_line(
        &mut output,
        format_args!("display_revision: {}", record.display_revision),
    )?;
    if record.parent_revision_digests.is_empty() {
        write_yaml_line(&mut output, format_args!("parent_revision_digests: []"))?;
    } else {
        write_yaml_line(&mut output, format_args!("parent_revision_digests:"))?;
        for digest in &record.parent_revision_digests {
            check_control(control)?;
            write_yaml_line(&mut output, format_args!("  - {}", yaml_quoted(digest)))?;
        }
    }
    write_yaml_line(
        &mut output,
        format_args!("kind: {}", memory_kind_text(record.kind)),
    )?;
    write_yaml_line(
        &mut output,
        format_args!("title: {}", yaml_quoted(&record.title)),
    )?;
    write_yaml_line(
        &mut output,
        format_args!("body: {}", yaml_quoted(&record.body)),
    )?;
    write_yaml_line(&mut output, format_args!("scope:"))?;
    write_yaml_line(
        &mut output,
        format_args!("  repository_id: {}", record.scope.repository_id),
    )?;
    write_yaml_line(
        &mut output,
        format_args!("  subject_evidence: {}", record.scope.subject_evidence),
    )?;
    write_yaml_line(&mut output, format_args!("provenance:"))?;
    write_yaml_line(
        &mut output,
        format_args!(
            "  origin: {}",
            provenance_origin_text(record.provenance.origin)
        ),
    )?;
    write_yaml_line(
        &mut output,
        format_args!(
            "  actor_kind: {}",
            actor_kind_text(record.provenance.actor_kind)
        ),
    )?;
    write_yaml_line(
        &mut output,
        format_args!("  actor_id: {}", yaml_quoted(&record.provenance.actor_id)),
    )?;
    write_yaml_line(
        &mut output,
        format_args!("assurance: {}", assurance_text(record.assurance)),
    )?;
    write_yaml_line(
        &mut output,
        format_args!("lifecycle: {}", lifecycle_text(record.lifecycle)),
    )?;
    write_yaml_line(&mut output, format_args!("validity:"))?;
    match &record.validity {
        ValidityDto::Commits {
            introduced_by,
            invalidated_by,
        } => {
            write_yaml_line(&mut output, format_args!("  kind: commits"))?;
            write_yaml_line(&mut output, format_args!("  introduced_by:"))?;
            write_commit_ids(&mut output, introduced_by, control)?;
            if invalidated_by.is_empty() {
                write_yaml_line(&mut output, format_args!("  invalidated_by: []"))?;
            } else {
                write_yaml_line(&mut output, format_args!("  invalidated_by:"))?;
                write_commit_ids(&mut output, invalidated_by, control)?;
            }
        }
        ValidityDto::Worktree {
            source_snapshot_digest,
        } => {
            write_yaml_line(&mut output, format_args!("  kind: worktree"))?;
            write_yaml_line(
                &mut output,
                format_args!(
                    "  source_snapshot_digest: {}",
                    yaml_quoted(source_snapshot_digest)
                ),
            )?;
        }
    }
    write_yaml_line(&mut output, format_args!("evidence:"))?;
    for evidence in &record.evidence {
        check_control(control)?;
        write_yaml_line(
            &mut output,
            format_args!("  - kind: {}", evidence_kind_text(evidence.kind)),
        )?;
        write_yaml_line(
            &mut output,
            format_args!(
                "    source_snapshot_digest: {}",
                yaml_quoted(&evidence.source_snapshot_digest)
            ),
        )?;
        write_yaml_line(&mut output, format_args!("    path: {}", evidence.path))?;
        write_yaml_line(
            &mut output,
            format_args!(
                "    content_digest: {}",
                yaml_quoted(&evidence.content_digest)
            ),
        )?;
        write_yaml_line(
            &mut output,
            format_args!(
                "    artifact_digest: {}",
                yaml_quoted(&evidence.artifact_digest)
            ),
        )?;
        write_yaml_line(
            &mut output,
            format_args!("    fact_ordinal: {}", evidence.fact_ordinal),
        )?;
        write_yaml_line(
            &mut output,
            format_args!(
                "    symbol_kind: {}",
                symbol_kind_text(evidence.symbol_kind)
            ),
        )?;
        write_yaml_line(
            &mut output,
            format_args!("    name: {}", yaml_quoted(&evidence.name)),
        )?;
        write_yaml_line(
            &mut output,
            format_args!(
                "    qualified_name: {}",
                yaml_quoted(&evidence.qualified_name)
            ),
        )?;
        write_yaml_line(
            &mut output,
            format_args!("    name_start: {}", evidence.name_start),
        )?;
        write_yaml_line(
            &mut output,
            format_args!("    name_length: {}", evidence.name_length),
        )?;
        write_yaml_line(
            &mut output,
            format_args!("    declaration_start: {}", evidence.declaration_start),
        )?;
        write_yaml_line(
            &mut output,
            format_args!("    declaration_length: {}", evidence.declaration_length),
        )?;
        write_yaml_line(
            &mut output,
            format_args!(
                "    declaration_digest: {}",
                yaml_quoted(&evidence.declaration_digest)
            ),
        )?;
        write_yaml_line(
            &mut output,
            format_args!("    producer_id: {}", yaml_quoted(&evidence.producer_id)),
        )?;
        write_yaml_line(
            &mut output,
            format_args!(
                "    producer_version: {}",
                yaml_quoted(&evidence.producer_version)
            ),
        )?;
    }
    if record.relationships.is_empty() {
        write_yaml_line(&mut output, format_args!("relationships: []"))?;
    } else {
        write_yaml_line(&mut output, format_args!("relationships:"))?;
        for relationship in &record.relationships {
            check_control(control)?;
            write_yaml_line(
                &mut output,
                format_args!("  - kind: {}", relationship_kind_text(relationship.kind)),
            )?;
            write_yaml_line(
                &mut output,
                format_args!("    record_id: {}", relationship.record_id),
            )?;
            write_yaml_line(
                &mut output,
                format_args!(
                    "    revision_digest: {}",
                    yaml_quoted(&relationship.revision_digest)
                ),
            )?;
        }
    }
    write_yaml_line(&mut output, format_args!("tombstone: {}", record.tombstone))?;
    check_control(control)?;
    Ok(output.into_bytes())
}

fn write_commit_ids(
    output: &mut MemoryYamlOutput,
    commits: &[CommitIdDto],
    control: MemoryFormatControl<'_>,
) -> Result<(), MemoryFormatError> {
    for commit in commits {
        check_control(control)?;
        write_yaml_line(
            output,
            format_args!(
                "    - object_format: {}",
                object_format_text(commit.object_format)
            ),
        )?;
        write_yaml_line(
            output,
            format_args!("      object_id: {}", yaml_quoted(&commit.object_id)),
        )?;
    }
    Ok(())
}

fn write_yaml_line(
    output: &mut MemoryYamlOutput,
    arguments: fmt::Arguments<'_>,
) -> Result<(), MemoryFormatError> {
    writeln!(output, "{arguments}").map_err(|_| MemoryFormatError::GenerationFailed)
}

fn yaml_quoted(value: &str) -> String {
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
    quoted
}

fn memory_kind_text(kind: MemoryKindDto) -> &'static str {
    match kind {
        MemoryKindDto::Decision => "decision",
        MemoryKindDto::Failure => "failure",
        MemoryKindDto::Fact => "fact",
        MemoryKindDto::Procedure => "procedure",
        MemoryKindDto::Episode => "episode",
        MemoryKindDto::Preference => "preference",
        MemoryKindDto::Policy => "policy",
    }
}

fn provenance_origin_text(origin: ProvenanceOriginDto) -> &'static str {
    match origin {
        ProvenanceOriginDto::Human => "human",
    }
}

fn actor_kind_text(kind: ActorKindDto) -> &'static str {
    match kind {
        ActorKindDto::LocalAsserted => "local_asserted",
    }
}

fn assurance_text(assurance: AssuranceDto) -> &'static str {
    match assurance {
        AssuranceDto::LocallyApproved => "locally_approved",
    }
}

fn lifecycle_text(lifecycle: LifecycleDto) -> &'static str {
    match lifecycle {
        LifecycleDto::Active => "active",
        LifecycleDto::NeedsReview => "needs_review",
        LifecycleDto::Stale => "stale",
        LifecycleDto::Contradicted => "contradicted",
        LifecycleDto::Superseded => "superseded",
        LifecycleDto::Quarantined => "quarantined",
        LifecycleDto::Tombstoned => "tombstoned",
    }
}

fn object_format_text(format: ObjectFormatDto) -> &'static str {
    match format {
        ObjectFormatDto::Sha1 => "sha1",
        ObjectFormatDto::Sha256 => "sha256",
    }
}

fn evidence_kind_text(kind: EvidenceKindDto) -> &'static str {
    match kind {
        EvidenceKindDto::RustSymbol => "rust_symbol",
    }
}

fn symbol_kind_text(kind: SymbolKindDto) -> &'static str {
    match kind {
        SymbolKindDto::Function => "function",
        SymbolKindDto::Method => "method",
        SymbolKindDto::Struct => "struct",
        SymbolKindDto::Enum => "enum",
        SymbolKindDto::Union => "union",
        SymbolKindDto::Trait => "trait",
        SymbolKindDto::Module => "module",
        SymbolKindDto::TypeAlias => "type_alias",
        SymbolKindDto::Constant => "constant",
        SymbolKindDto::Static => "static",
        SymbolKindDto::Macro => "macro",
    }
}

fn relationship_kind_text(kind: RelationshipKindDto) -> &'static str {
    match kind {
        RelationshipKindDto::Contradicts => "contradicts",
        RelationshipKindDto::Supersedes => "supersedes",
    }
}
