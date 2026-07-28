fn parse_strict_memory(input: &[u8]) -> Result<ValidatedMemoryRecord, StrictMemoryError> {
    if input.len() > MAX_INPUT_BYTES {
        return Err(StrictMemoryError::InputTooLarge);
    }
    if input.contains(&b'\r') {
        return Err(StrictMemoryError::InvalidYaml);
    }
    let text = str::from_utf8(input).map_err(|_| StrictMemoryError::InvalidYaml)?;
    reject_yaml_extensions(text)?;

    let options = serde_saphyr::options! {
        budget: serde_saphyr::budget! {
            max_reader_input_bytes: Some(MAX_INPUT_BYTES),
            max_events: 4_096,
            max_aliases: 0,
            max_anchors: 0,
            max_depth: 8,
            max_inclusion_depth: 0,
            max_documents: 1,
            max_nodes: 2_048,
            max_total_scalar_bytes: MAX_TOTAL_SCALAR_BYTES,
            max_total_comment_bytes: 4 * 1_024,
            max_merge_keys: 0,
        },
        duplicate_keys: DuplicateKeyPolicy::Error,
        merge_keys: MergeKeyPolicy::Error,
        alias_limits: serde_saphyr::alias_limits! {
            max_total_replayed_events: 0,
            max_replay_stack_depth: 0,
            max_alias_expansions_per_anchor: 0,
        },
        legacy_octal_numbers: false,
        strict_booleans: true,
        no_schema: true,
        with_snippet: false,
        crop_radius: 0,
    };
    let dto: MemoryRecordDto = serde_saphyr::from_slice_with_options(input, options)
        .map_err(|_| StrictMemoryError::InvalidYaml)?;
    validate_memory_record(dto)
}

fn reject_yaml_extensions(input: &str) -> Result<(), StrictMemoryError> {
    let mut scanner = Scanner::new(StrInput::new(input));
    for Token(_, token) in scanner.by_ref() {
        if matches!(
            token,
            TokenType::VersionDirective(..)
                | TokenType::TagDirective(..)
                | TokenType::ReservedDirective(..)
                | TokenType::Anchor(..)
                | TokenType::Alias(..)
                | TokenType::Tag(..)
        ) {
            return Err(StrictMemoryError::InvalidYaml);
        }
    }
    if scanner.get_error().is_some() {
        return Err(StrictMemoryError::InvalidYaml);
    }

    let mut preflight = YamlPreflight::default();
    for parsed in Parser::new_from_str(input) {
        let (event, _) = parsed.map_err(|_| StrictMemoryError::InvalidYaml)?;
        preflight.observe(event)?;
    }
    if preflight.depth != 0 || preflight.documents != 1 {
        return Err(StrictMemoryError::InvalidYaml);
    }
    Ok(())
}

fn increment_bounded(value: &mut usize, limit: usize) -> Result<(), StrictMemoryError> {
    *value = value.checked_add(1).ok_or(StrictMemoryError::InvalidYaml)?;
    if *value > limit {
        return Err(StrictMemoryError::InvalidYaml);
    }
    Ok(())
}

fn validate_memory_record(
    mut dto: MemoryRecordDto,
) -> Result<ValidatedMemoryRecord, StrictMemoryError> {
    validate_record_header(&dto)?;
    sort_unique_strings(&mut dto.parent_revision_digests, MAX_PARENT_DIGESTS)?;
    if dto
        .parent_revision_digests
        .iter()
        .any(|digest| !valid_lower_hex(digest, 64))
    {
        return Err(StrictMemoryError::InvalidRecord);
    }

    validate_scope_and_text(&dto)?;
    validate_validity(&mut dto.validity)?;
    validate_evidence(&dto.evidence)?;
    validate_relationships(&mut dto.relationships)?;
    validate_tombstone(dto.lifecycle, dto.tombstone, &dto.parent_revision_digests)?;
    Ok(ValidatedMemoryRecord(dto))
}

fn validate_record_header(dto: &MemoryRecordDto) -> Result<(), StrictMemoryError> {
    if dto.schema_version != 1
        || decode_record_id(&dto.record_id).is_none()
        || dto.display_revision == 0
        || dto.parent_revision_digests.len() > MAX_PARENT_DIGESTS
        || dto.evidence.is_empty()
        || dto.evidence.len() > MAX_EVIDENCE
        || dto.relationships.len() > MAX_RELATIONSHIPS
    {
        return Err(StrictMemoryError::InvalidRecord);
    }
    Ok(())
}

fn validate_scope_and_text(dto: &MemoryRecordDto) -> Result<(), StrictMemoryError> {
    if !valid_title(&dto.title)
        || !valid_body(&dto.body)
        || RepositoryIdentityTextV1::decode(&dto.scope.repository_id).is_err()
        || dto.scope.subject_evidence > MAX_INTEROPERABLE_INTEGER
        || usize::try_from(dto.scope.subject_evidence)
            .ok()
            .is_none_or(|index| index >= dto.evidence.len())
        || !valid_printable_ascii(&dto.provenance.actor_id, MAX_ACTOR_BYTES)
    {
        return Err(StrictMemoryError::InvalidRecord);
    }
    Ok(())
}

fn validate_validity(validity: &mut ValidityDto) -> Result<(), StrictMemoryError> {
    match validity {
        ValidityDto::Commits {
            introduced_by,
            invalidated_by,
        } => {
            if introduced_by.is_empty()
                || introduced_by.len() > MAX_COMMITS
                || invalidated_by.len() > MAX_COMMITS
                || introduced_by.iter().any(|commit| !valid_commit(commit))
                || invalidated_by.iter().any(|commit| !valid_commit(commit))
            {
                return Err(StrictMemoryError::InvalidRecord);
            }
            introduced_by.sort_unstable();
            invalidated_by.sort_unstable();
            if has_duplicates(introduced_by)
                || has_duplicates(invalidated_by)
                || introduced_by
                    .iter()
                    .any(|commit| invalidated_by.binary_search(commit).is_ok())
            {
                return Err(StrictMemoryError::InvalidRecord);
            }
        }
        ValidityDto::Worktree {
            source_snapshot_digest,
        } => {
            if !valid_lower_hex(source_snapshot_digest, 64) {
                return Err(StrictMemoryError::InvalidRecord);
            }
        }
    }
    Ok(())
}

fn validate_evidence(evidence: &[RustSymbolEvidenceDto]) -> Result<(), StrictMemoryError> {
    for item in evidence {
        if !valid_lower_hex(&item.source_snapshot_digest, 64)
            || RepositoryPathTextV1::decode(&item.path, PATH_TEXT_LIMIT, PATH_LIMITS).is_err()
            || !valid_lower_hex(&item.content_digest, 64)
            || !valid_lower_hex(&item.artifact_digest, 64)
            || !valid_lower_hex(&item.declaration_digest, 64)
            || item.fact_ordinal > MAX_INTEROPERABLE_INTEGER
            || !valid_source_name(&item.name, MAX_NAME_BYTES)
            || !valid_source_name(&item.qualified_name, MAX_QUALIFIED_NAME_BYTES)
            || !valid_printable_ascii(&item.producer_id, MAX_PRODUCER_BYTES)
            || !valid_printable_ascii(&item.producer_version, MAX_PRODUCER_BYTES)
            || !valid_evidence_spans(item)
        {
            return Err(StrictMemoryError::InvalidRecord);
        }
    }
    Ok(())
}

fn valid_evidence_spans(item: &RustSymbolEvidenceDto) -> bool {
    if item.name_length != u64::try_from(item.name.len()).unwrap_or(u64::MAX) {
        return false;
    }
    let Some(name_end) = bounded_span_end(item.name_start, item.name_length) else {
        return false;
    };
    let Some(declaration_end) = bounded_span_end(item.declaration_start, item.declaration_length)
    else {
        return false;
    };
    item.name_start >= item.declaration_start && name_end <= declaration_end
}

fn bounded_span_end(start: u64, length: u64) -> Option<u64> {
    if start > MAX_INTEROPERABLE_INTEGER || length == 0 || length > MAX_INTEROPERABLE_INTEGER {
        return None;
    }
    let end = start.checked_add(length)?;
    (end <= MAX_SOURCE_BYTES).then_some(end)
}

fn validate_relationships(relationships: &mut [RelationshipDto]) -> Result<(), StrictMemoryError> {
    if relationships.len() > MAX_RELATIONSHIPS
        || relationships.iter().any(|relationship| {
            decode_record_id(&relationship.record_id).is_none()
                || !valid_lower_hex(&relationship.revision_digest, 64)
        })
    {
        return Err(StrictMemoryError::InvalidRecord);
    }
    relationships.sort_unstable();
    if has_duplicates(relationships) {
        return Err(StrictMemoryError::InvalidRecord);
    }
    Ok(())
}

fn validate_tombstone(
    lifecycle: Lifecycle,
    tombstone: bool,
    parents: &[String],
) -> Result<(), StrictMemoryError> {
    let valid = if tombstone {
        lifecycle == Lifecycle::Tombstoned && !parents.is_empty()
    } else {
        lifecycle != Lifecycle::Tombstoned
    };
    valid.then_some(()).ok_or(StrictMemoryError::InvalidRecord)
}

fn sort_unique_strings(values: &mut [String], limit: usize) -> Result<(), StrictMemoryError> {
    if values.len() > limit {
        return Err(StrictMemoryError::InvalidRecord);
    }
    values.sort_unstable();
    if has_duplicates(values) {
        return Err(StrictMemoryError::InvalidRecord);
    }
    Ok(())
}

fn has_duplicates<T: PartialEq>(values: &[T]) -> bool {
    values.windows(2).any(|pair| pair[0] == pair[1])
}

fn valid_commit(commit: &CommitIdDto) -> bool {
    let expected = match commit.object_format {
        ObjectFormat::Sha1 => 40,
        ObjectFormat::Sha256 => 64,
    };
    valid_lower_hex(&commit.object_id, expected)
}

fn valid_title(value: &str) -> bool {
    (1..=MAX_TITLE_BYTES).contains(&value.len())
        && !value.chars().any(|character| {
            matches!(
                character,
                '\0' | '\n' | '\r' | '\u{85}' | '\u{2028}' | '\u{2029}'
            )
        })
}

fn valid_body(value: &str) -> bool {
    (1..=MAX_BODY_BYTES).contains(&value.len())
        && !value
            .as_bytes()
            .iter()
            .any(|byte| matches!(byte, 0 | b'\r'))
}

fn valid_source_name(value: &str, limit: usize) -> bool {
    (1..=limit).contains(&value.len())
        && !value
            .as_bytes()
            .iter()
            .any(|byte| matches!(byte, 0 | b'\n' | b'\r'))
}

fn valid_printable_ascii(value: &str, limit: usize) -> bool {
    (1..=limit).contains(&value.len())
        && value
            .as_bytes()
            .iter()
            .all(|byte| matches!(byte, b' '..=b'~'))
}

fn valid_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .as_bytes()
            .iter()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn encode_record_id(bytes: [u8; 16]) -> String {
    let mut value = u128::from_be_bytes(bytes);
    let mut encoded = [b'0'; RECORD_ID_PAYLOAD_BYTES];
    for output in encoded.iter_mut().rev() {
        *output = CROCKFORD_BASE32[(value & 31) as usize];
        value >>= 5;
    }
    let payload = str::from_utf8(&encoded).expect("the fixed alphabet is UTF-8");
    format!("{RECORD_ID_PREFIX}{payload}")
}

fn decode_record_id(value: &str) -> Option<[u8; 16]> {
    let payload = value.strip_prefix(RECORD_ID_PREFIX)?.as_bytes();
    if payload.len() != RECORD_ID_PAYLOAD_BYTES {
        return None;
    }
    let mut decoded = 0_u128;
    for (index, byte) in payload.iter().enumerate() {
        let digit = CROCKFORD_BASE32
            .iter()
            .position(|candidate| candidate == byte)?;
        if index == 0 && digit > 7 {
            return None;
        }
        decoded = decoded.checked_mul(32)?.checked_add(digit as u128)?;
    }
    Some(decoded.to_be_bytes())
}
