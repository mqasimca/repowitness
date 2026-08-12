/// Strictly parses, validates, canonicalizes, and identifies one memory record.
pub fn parse_memory_record(
    input: &[u8],
    control: MemoryFormatControl<'_>,
) -> Result<ParsedMemoryRecord, MemoryFormatError> {
    check_control(control)?;
    if input.len() > MAX_MEMORY_YAML_BYTES {
        return Err(MemoryFormatError::InputTooLarge);
    }
    if input.contains(&b'\r') {
        return Err(MemoryFormatError::InvalidYaml);
    }
    let text = str::from_utf8(input).map_err(|_| MemoryFormatError::InvalidYaml)?;
    reject_yaml_extensions(text, control)?;
    check_control(control)?;

    let options = serde_saphyr::options! {
        budget: serde_saphyr::budget! {
            max_reader_input_bytes: Some(MAX_MEMORY_YAML_BYTES),
            max_events: MAX_MEMORY_YAML_EVENTS,
            max_aliases: 0,
            max_anchors: 0,
            max_depth: MAX_MEMORY_YAML_DEPTH,
            max_inclusion_depth: 0,
            max_documents: 1,
            max_nodes: MAX_MEMORY_YAML_NODES,
            max_total_scalar_bytes: MAX_MEMORY_SCALAR_BYTES,
            max_total_comment_bytes: MAX_MEMORY_COMMENT_BYTES,
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
        .map_err(|_| MemoryFormatError::InvalidYaml)?;
    check_control(control)?;
    let record = dto_into_domain(dto)?;
    let canonical_json = canonical_memory_json(&record, control)?;
    let digest = digest_canonical_bytes_for_schema(&canonical_json, record.schema_version())?;
    Ok(ParsedMemoryRecord {
        record,
        canonical_json: canonical_json.into_boxed_slice(),
        digest,
    })
}

/// Reconstructs one immutable database journal record and verifies its exact
/// canonical bytes and expected semantic identity.
pub(crate) fn parse_persisted_canonical_memory_record(
    input: &[u8],
    display_revision: MemoryDisplayRevision,
    expected_digest: CanonicalMemoryDigest,
    control: MemoryFormatControl<'_>,
) -> Result<ParsedMemoryRecord, MemoryFormatError> {
    check_control(control)?;
    if input.is_empty() || input.len() > MAX_CANONICAL_MEMORY_BYTES {
        return Err(MemoryFormatError::InvalidCanonicalRecord);
    }
    let persisted: PersistedCanonicalMemoryRecordDto =
        serde_json::from_slice(input).map_err(|_| MemoryFormatError::InvalidCanonicalRecord)?;
    check_control(control)?;
    let record = dto_into_domain(MemoryRecordDto {
        schema_version: persisted.schema_version,
        record_id: persisted.record_id,
        display_revision: display_revision.get(),
        parent_revision_digests: persisted.parent_revision_digests,
        kind: persisted.kind,
        title: persisted.title,
        body: persisted.body,
        scope: persisted.scope,
        provenance: persisted.provenance,
        assurance: persisted.assurance,
        lifecycle: persisted.lifecycle,
        validity: persisted.validity,
        evidence: persisted.evidence,
        relationships: persisted.relationships,
        tombstone: persisted.tombstone,
    })?;
    let canonical_json = canonical_memory_json(&record, control)?;
    if canonical_json != input {
        return Err(MemoryFormatError::InvalidCanonicalRecord);
    }
    let digest = digest_canonical_bytes_for_schema(&canonical_json, record.schema_version())?;
    if digest != expected_digest {
        return Err(MemoryFormatError::InvalidCanonicalRecord);
    }
    check_control(control)?;
    Ok(ParsedMemoryRecord {
        record,
        canonical_json: canonical_json.into_boxed_slice(),
        digest,
    })
}

/// Serializes one validated record with the exact RFC 8785 semantic profile.
pub fn canonical_memory_json(
    record: &MemoryRecord,
    control: MemoryFormatControl<'_>,
) -> Result<Vec<u8>, MemoryFormatError> {
    check_control(control)?;
    let dto = domain_into_dto(record)?;
    let semantic = CanonicalMemoryRecordDto {
        schema_version: dto.schema_version,
        record_id: &dto.record_id,
        parent_revision_digests: &dto.parent_revision_digests,
        kind: dto.kind,
        title: &dto.title,
        body: &dto.body,
        scope: &dto.scope,
        provenance: &dto.provenance,
        assurance: dto.assurance,
        lifecycle: dto.lifecycle,
        validity: &dto.validity,
        evidence: &dto.evidence,
        relationships: &dto.relationships,
        tombstone: dto.tombstone,
    };
    let mut output = CanonicalMemoryOutput::default();
    serde_json_canonicalizer::to_writer(&semantic, &mut output)
        .map_err(|_| MemoryFormatError::CanonicalizationFailed)?;
    check_control(control)?;
    Ok(output.into_bytes())
}

/// Computes the versioned, domain-separated digest of canonical memory semantics.
pub fn canonical_memory_digest(
    record: &MemoryRecord,
    control: MemoryFormatControl<'_>,
) -> Result<CanonicalMemoryDigest, MemoryFormatError> {
    let canonical = canonical_memory_json(record, control)?;
    check_control(control)?;
    digest_canonical_bytes_for_schema(&canonical, record.schema_version())
}

fn reject_yaml_extensions(
    input: &str,
    control: MemoryFormatControl<'_>,
) -> Result<(), MemoryFormatError> {
    let mut scanner = Scanner::new(StrInput::new(input));
    for Token(_, token) in scanner.by_ref() {
        check_control(control)?;
        if matches!(
            token,
            TokenType::VersionDirective(..)
                | TokenType::TagDirective(..)
                | TokenType::ReservedDirective(..)
                | TokenType::Anchor(..)
                | TokenType::Alias(..)
                | TokenType::Tag(..)
        ) {
            return Err(MemoryFormatError::InvalidYaml);
        }
    }
    if scanner.get_error().is_some() {
        return Err(MemoryFormatError::InvalidYaml);
    }

    let mut preflight = YamlPreflight::default();
    for parsed in Parser::new_from_str(input) {
        let (event, _) = parsed.map_err(|_| MemoryFormatError::InvalidYaml)?;
        preflight.observe(event, control)?;
    }
    if preflight.depth != 0 || preflight.documents != 1 {
        return Err(MemoryFormatError::InvalidYaml);
    }
    Ok(())
}

fn increment_bounded(value: &mut usize, limit: usize) -> Result<(), MemoryFormatError> {
    *value = value.checked_add(1).ok_or(MemoryFormatError::InvalidYaml)?;
    if *value > limit {
        return Err(MemoryFormatError::InvalidYaml);
    }
    Ok(())
}

fn dto_into_domain(dto: MemoryRecordDto) -> Result<MemoryRecord, MemoryFormatError> {
    if dto.schema_version != MEMORY_RECORD_SCHEMA_VERSION
        && dto.schema_version != MEMORY_RECORD_PROFILE_V2_SCHEMA_VERSION
    {
        return Err(MemoryFormatError::InvalidRecord(
            MemoryRecordError::InvalidSchemaVersion,
        ));
    }
    let record_id = MemoryRecordIdTextV1::decode(&dto.record_id)
        .map_err(|_| MemoryFormatError::InvalidRecord(MemoryRecordError::InvalidRecordId))?;
    let display_revision = MemoryDisplayRevision::try_new(dto.display_revision)?;
    let parents = dto
        .parent_revision_digests
        .iter()
        .map(|digest| {
            decode_lower_hex::<32>(digest)
                .map(CanonicalMemoryDigest::new)
                .ok_or(MemoryFormatError::InvalidYaml)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let header = MemoryRecordHeader::try_new(record_id, display_revision, parents)?;
    let claim = MemoryClaim::new(
        memory_kind(dto.kind),
        MemoryTitle::try_new(dto.title)?,
        MemoryBody::try_new(dto.body)?,
    );
    let repository = RepositoryIdentityTextV1::decode(&dto.scope.repository_id)
        .map_err(|_| MemoryFormatError::InvalidYaml)?;
    let scope = MemoryScope::new(
        repository,
        MemoryEvidenceIndex::try_new(dto.scope.subject_evidence)?,
    );
    let provenance = MemoryProvenance::new(
        provenance_origin(dto.provenance.origin),
        actor_kind(dto.provenance.actor_kind),
        MemoryActorId::try_new(dto.provenance.actor_id)?,
    );
    let validity = validity_into_domain(dto.validity)?;
    let evidence = dto
        .evidence
        .into_iter()
        .map(evidence_into_domain)
        .collect::<Result<Vec<_>, _>>()?;
    let relationships = dto
        .relationships
        .into_iter()
        .map(relationship_into_domain)
        .collect::<Result<Vec<_>, _>>()?;
    MemoryRecord::try_new_profile(
        dto.schema_version,
        header,
        claim,
        scope,
        provenance,
        assurance(dto.assurance),
        lifecycle(dto.lifecycle),
        validity,
        evidence,
        relationships,
        dto.tombstone,
    )
    .map_err(Into::into)
}

fn validity_into_domain(validity: ValidityDto) -> Result<MemoryValidity, MemoryFormatError> {
    match validity {
        ValidityDto::Commits {
            introduced_by,
            invalidated_by,
        } => MemoryValidity::try_commits(
            introduced_by
                .into_iter()
                .map(commit_into_domain)
                .collect::<Result<Vec<_>, _>>()?,
            invalidated_by
                .into_iter()
                .map(commit_into_domain)
                .collect::<Result<Vec<_>, _>>()?,
        )
        .map_err(Into::into),
        ValidityDto::Worktree {
            source_snapshot_digest,
        } => Ok(MemoryValidity::worktree(SourceSnapshotDigest::new(
            decode_lower_hex::<32>(&source_snapshot_digest)
                .ok_or(MemoryFormatError::InvalidYaml)?,
        ))),
    }
}

fn commit_into_domain(commit: CommitIdDto) -> Result<MemoryCommitId, MemoryFormatError> {
    match commit.object_format {
        ObjectFormatDto::Sha1 => decode_lower_hex::<20>(&commit.object_id)
            .map(MemoryCommitId::Sha1)
            .ok_or(MemoryFormatError::InvalidYaml),
        ObjectFormatDto::Sha256 => decode_lower_hex::<32>(&commit.object_id)
            .map(MemoryCommitId::Sha256)
            .ok_or(MemoryFormatError::InvalidYaml),
    }
}

fn evidence_into_domain(
    evidence: RustSymbolEvidenceDto,
) -> Result<MemoryEvidence, MemoryFormatError> {
    let _ = evidence.kind;
    let source_snapshot = SourceSnapshotDigest::new(
        decode_lower_hex::<32>(&evidence.source_snapshot_digest)
            .ok_or(MemoryFormatError::InvalidYaml)?,
    );
    let path = RepositoryPathTextV1::decode(&evidence.path, PATH_TEXT_LIMIT, PATH_LIMITS)
        .map_err(|_| MemoryFormatError::InvalidYaml)?;
    let content = SourceContentDigest::new(
        decode_lower_hex::<32>(&evidence.content_digest).ok_or(MemoryFormatError::InvalidYaml)?,
    );
    let artifact = AnalysisArtifactDigest::new(
        decode_lower_hex::<32>(&evidence.artifact_digest).ok_or(MemoryFormatError::InvalidYaml)?,
    );
    let fact_ordinal = MemoryFactOrdinal::try_new(evidence.fact_ordinal)?;
    let name = MemorySymbolName::try_new(evidence.name)?;
    let qualified_name = MemoryQualifiedName::try_new(evidence.qualified_name)?;
    let name_span = memory_span(evidence.name_start, evidence.name_length)?;
    let declaration_span = memory_span(evidence.declaration_start, evidence.declaration_length)?;
    let declaration_digest = DeclarationDigest::new(
        decode_lower_hex::<32>(&evidence.declaration_digest)
            .ok_or(MemoryFormatError::InvalidYaml)?,
    );
    let producer = ProducerIdentity::new(
        MemoryProducerId::try_new(evidence.producer_id)?,
        MemoryProducerVersion::try_new(evidence.producer_version)?,
    );
    Ok(MemoryEvidence::RustSymbol(
        RustSymbolMemoryEvidence::try_new(
            source_snapshot,
            path,
            content,
            artifact,
            fact_ordinal,
            symbol_kind(evidence.symbol_kind),
            name,
            qualified_name,
            name_span,
            declaration_span,
            declaration_digest,
            producer,
        )?,
    ))
}

fn relationship_into_domain(
    relationship: RelationshipDto,
) -> Result<MemoryRelationship, MemoryFormatError> {
    let record_id = MemoryRecordIdTextV1::decode(&relationship.record_id)
        .map_err(|_| MemoryFormatError::InvalidRecord(MemoryRecordError::InvalidRecordId))?;
    let revision_digest = CanonicalMemoryDigest::new(
        decode_lower_hex::<32>(&relationship.revision_digest)
            .ok_or(MemoryFormatError::InvalidYaml)?,
    );
    Ok(MemoryRelationship::new(
        relationship_kind(relationship.kind),
        record_id,
        revision_digest,
    ))
}

fn memory_span(start: u64, length: u64) -> Result<ByteSpan, MemoryFormatError> {
    if start > MAX_MEMORY_INTEROPERABLE_INTEGER
        || length == 0
        || length > MAX_MEMORY_INTEROPERABLE_INTEGER
    {
        return Err(MemoryRecordError::InvalidEvidence.into());
    }
    let end = start
        .checked_add(length)
        .filter(|end| *end <= MAX_MEMORY_SOURCE_BYTES)
        .ok_or(MemoryFormatError::InvalidRecord(
            MemoryRecordError::InvalidEvidence,
        ))?;
    ByteSpan::try_new(ByteOffset::new(start), ByteOffset::new(end))
        .map_err(|_| MemoryRecordError::InvalidEvidence.into())
}

fn domain_into_dto(record: &MemoryRecord) -> Result<MemoryRecordDto, MemoryFormatError> {
    check_record_schema(record)?;
    let header = record.header();
    let claim = record.claim();
    let scope = record.scope();
    let provenance = record.provenance();
    Ok(MemoryRecordDto {
        schema_version: record.schema_version(),
        record_id: MemoryRecordIdTextV1::encode(header.record_id()).into_string(),
        display_revision: header.display_revision().get(),
        parent_revision_digests: header
            .parents()
            .iter()
            .map(|digest| encode_lower_hex(digest.as_bytes()))
            .collect(),
        kind: memory_kind_dto(claim.kind()),
        title: claim.title().as_str().to_owned(),
        body: claim.body().as_str().to_owned(),
        scope: ScopeDto {
            repository_id: RepositoryIdentityTextV1::encode(scope.repository()).into_string(),
            subject_evidence: scope.subject_evidence().get(),
        },
        provenance: ProvenanceDto {
            origin: provenance_origin_dto(provenance.origin()),
            actor_kind: actor_kind_dto(provenance.actor_kind()),
            actor_id: provenance.actor_id().as_str().to_owned(),
        },
        assurance: assurance_dto(record.assurance()),
        lifecycle: lifecycle_dto(record.lifecycle()),
        validity: validity_into_dto(record.validity()),
        evidence: record
            .evidence()
            .iter()
            .map(evidence_into_dto)
            .collect::<Result<Vec<_>, _>>()?,
        relationships: record
            .relationships()
            .iter()
            .map(relationship_into_dto)
            .collect(),
        tombstone: record.tombstone(),
    })
}

fn validity_into_dto(validity: &MemoryValidity) -> ValidityDto {
    match validity {
        MemoryValidity::Commits {
            introduced_by,
            invalidated_by,
        } => ValidityDto::Commits {
            introduced_by: introduced_by.iter().map(commit_into_dto).collect(),
            invalidated_by: invalidated_by.iter().map(commit_into_dto).collect(),
        },
        MemoryValidity::Worktree { source_snapshot } => ValidityDto::Worktree {
            source_snapshot_digest: encode_lower_hex(source_snapshot.as_bytes()),
        },
    }
}

fn commit_into_dto(commit: &MemoryCommitId) -> CommitIdDto {
    CommitIdDto {
        object_format: match commit.object_format() {
            MemoryObjectFormat::Sha1 => ObjectFormatDto::Sha1,
            MemoryObjectFormat::Sha256 => ObjectFormatDto::Sha256,
        },
        object_id: encode_lower_hex(commit.as_bytes()),
    }
}

fn evidence_into_dto(
    evidence: &MemoryEvidence,
) -> Result<RustSymbolEvidenceDto, MemoryFormatError> {
    let MemoryEvidence::RustSymbol(evidence) = evidence;
    let name_span = evidence.name_span();
    let declaration_span = evidence.declaration_span();
    Ok(RustSymbolEvidenceDto {
        kind: EvidenceKindDto::RustSymbol,
        source_snapshot_digest: encode_lower_hex(evidence.source_snapshot().as_bytes()),
        path: RepositoryPathTextV1::encode(evidence.path(), PATH_TEXT_LIMIT)
            .map_err(|_| MemoryFormatError::GenerationFailed)?
            .into_string(),
        content_digest: encode_lower_hex(evidence.content().as_bytes()),
        artifact_digest: encode_lower_hex(evidence.artifact().as_bytes()),
        fact_ordinal: evidence.fact_ordinal().get(),
        symbol_kind: symbol_kind_dto(evidence.symbol_kind()),
        name: evidence.name().as_str().to_owned(),
        qualified_name: evidence.qualified_name().as_str().to_owned(),
        name_start: name_span.start().get(),
        name_length: name_span.len().get(),
        declaration_start: declaration_span.start().get(),
        declaration_length: declaration_span.len().get(),
        declaration_digest: encode_lower_hex(evidence.declaration_digest().as_bytes()),
        producer_id: evidence.producer().id().as_str().to_owned(),
        producer_version: evidence.producer().version().as_str().to_owned(),
    })
}

fn relationship_into_dto(relationship: &MemoryRelationship) -> RelationshipDto {
    RelationshipDto {
        kind: relationship_kind_dto(relationship.kind()),
        record_id: MemoryRecordIdTextV1::encode(relationship.record_id()).into_string(),
        revision_digest: encode_lower_hex(relationship.revision_digest().as_bytes()),
    }
}

fn check_record_schema(record: &MemoryRecord) -> Result<(), MemoryFormatError> {
    if record.schema_version() != MEMORY_RECORD_SCHEMA_VERSION
        && record.schema_version() != MEMORY_RECORD_PROFILE_V2_SCHEMA_VERSION
    {
        return Err(MemoryFormatError::GenerationFailed);
    }
    Ok(())
}

pub(crate) fn digest_canonical_bytes_for_schema(
    canonical: &[u8],
    schema_version: u32,
) -> Result<CanonicalMemoryDigest, MemoryFormatError> {
    let length =
        u64::try_from(canonical.len()).map_err(|_| MemoryFormatError::CanonicalizationFailed)?;
    let mut hasher = Sha256::new();
    hasher.update(b"RepoWitness\0memory-record\0");
    hasher.update(schema_version.to_be_bytes());
    hasher.update(length.to_be_bytes());
    hasher.update(canonical);
    Ok(CanonicalMemoryDigest::new(hasher.finalize().into()))
}

fn decode_lower_hex<const N: usize>(value: &str) -> Option<[u8; N]> {
    if value.len() != N.checked_mul(2)? {
        return None;
    }
    let mut decoded = [0_u8; N];
    for (output, pair) in decoded.iter_mut().zip(value.as_bytes().chunks_exact(2)) {
        *output = decode_lower_hex_nibble(pair[0])?
            .checked_mul(16)?
            .checked_add(decode_lower_hex_nibble(pair[1])?)?;
    }
    Some(decoded)
}

fn decode_lower_hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

fn encode_lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

fn check_control(control: MemoryFormatControl<'_>) -> Result<(), MemoryFormatError> {
    if control.cancelled.load(Ordering::Relaxed) {
        return Err(MemoryFormatError::Cancelled);
    }
    if Instant::now() >= control.deadline {
        return Err(MemoryFormatError::DeadlineExceeded);
    }
    Ok(())
}

fn memory_kind(value: MemoryKindDto) -> MemoryKind {
    match value {
        MemoryKindDto::Decision => MemoryKind::Decision,
        MemoryKindDto::Failure => MemoryKind::Failure,
        MemoryKindDto::Fact => MemoryKind::Fact,
        MemoryKindDto::Procedure => MemoryKind::Procedure,
        MemoryKindDto::Episode => MemoryKind::Episode,
        MemoryKindDto::Preference => MemoryKind::Preference,
        MemoryKindDto::Policy => MemoryKind::Policy,
    }
}

fn memory_kind_dto(value: MemoryKind) -> MemoryKindDto {
    match value {
        MemoryKind::Decision => MemoryKindDto::Decision,
        MemoryKind::Failure => MemoryKindDto::Failure,
        MemoryKind::Fact => MemoryKindDto::Fact,
        MemoryKind::Procedure => MemoryKindDto::Procedure,
        MemoryKind::Episode => MemoryKindDto::Episode,
        MemoryKind::Preference => MemoryKindDto::Preference,
        MemoryKind::Policy => MemoryKindDto::Policy,
    }
}

fn provenance_origin(value: ProvenanceOriginDto) -> MemoryProvenanceOrigin {
    match value {
        ProvenanceOriginDto::Human => MemoryProvenanceOrigin::Human,
    }
}

fn provenance_origin_dto(value: MemoryProvenanceOrigin) -> ProvenanceOriginDto {
    match value {
        MemoryProvenanceOrigin::Human => ProvenanceOriginDto::Human,
    }
}

fn actor_kind(value: ActorKindDto) -> MemoryActorKind {
    match value {
        ActorKindDto::LocalAsserted => MemoryActorKind::LocalAsserted,
    }
}

fn actor_kind_dto(value: MemoryActorKind) -> ActorKindDto {
    match value {
        MemoryActorKind::LocalAsserted => ActorKindDto::LocalAsserted,
    }
}

fn assurance(value: AssuranceDto) -> MemoryAssurance {
    match value {
        AssuranceDto::LocallyApproved => MemoryAssurance::LocallyApproved,
    }
}

fn assurance_dto(value: MemoryAssurance) -> AssuranceDto {
    match value {
        MemoryAssurance::LocallyApproved => AssuranceDto::LocallyApproved,
    }
}

fn lifecycle(value: LifecycleDto) -> MemoryLifecycle {
    match value {
        LifecycleDto::Active => MemoryLifecycle::Active,
        LifecycleDto::NeedsReview => MemoryLifecycle::NeedsReview,
        LifecycleDto::Stale => MemoryLifecycle::Stale,
        LifecycleDto::Contradicted => MemoryLifecycle::Contradicted,
        LifecycleDto::Superseded => MemoryLifecycle::Superseded,
        LifecycleDto::Quarantined => MemoryLifecycle::Quarantined,
        LifecycleDto::Tombstoned => MemoryLifecycle::Tombstoned,
    }
}

fn lifecycle_dto(value: MemoryLifecycle) -> LifecycleDto {
    match value {
        MemoryLifecycle::Active => LifecycleDto::Active,
        MemoryLifecycle::NeedsReview => LifecycleDto::NeedsReview,
        MemoryLifecycle::Stale => LifecycleDto::Stale,
        MemoryLifecycle::Contradicted => LifecycleDto::Contradicted,
        MemoryLifecycle::Superseded => LifecycleDto::Superseded,
        MemoryLifecycle::Quarantined => LifecycleDto::Quarantined,
        MemoryLifecycle::Tombstoned => LifecycleDto::Tombstoned,
    }
}

fn symbol_kind(value: SymbolKindDto) -> RustMemorySymbolKind {
    match value {
        SymbolKindDto::Function => RustMemorySymbolKind::Function,
        SymbolKindDto::Method => RustMemorySymbolKind::Method,
        SymbolKindDto::Struct => RustMemorySymbolKind::Struct,
        SymbolKindDto::Enum => RustMemorySymbolKind::Enum,
        SymbolKindDto::Union => RustMemorySymbolKind::Union,
        SymbolKindDto::Trait => RustMemorySymbolKind::Trait,
        SymbolKindDto::Module => RustMemorySymbolKind::Module,
        SymbolKindDto::TypeAlias => RustMemorySymbolKind::TypeAlias,
        SymbolKindDto::Constant => RustMemorySymbolKind::Constant,
        SymbolKindDto::Static => RustMemorySymbolKind::Static,
        SymbolKindDto::Macro => RustMemorySymbolKind::Macro,
    }
}

fn symbol_kind_dto(value: RustMemorySymbolKind) -> SymbolKindDto {
    match value {
        RustMemorySymbolKind::Function => SymbolKindDto::Function,
        RustMemorySymbolKind::Method => SymbolKindDto::Method,
        RustMemorySymbolKind::Struct => SymbolKindDto::Struct,
        RustMemorySymbolKind::Enum => SymbolKindDto::Enum,
        RustMemorySymbolKind::Union => SymbolKindDto::Union,
        RustMemorySymbolKind::Trait => SymbolKindDto::Trait,
        RustMemorySymbolKind::Module => SymbolKindDto::Module,
        RustMemorySymbolKind::TypeAlias => SymbolKindDto::TypeAlias,
        RustMemorySymbolKind::Constant => SymbolKindDto::Constant,
        RustMemorySymbolKind::Static => SymbolKindDto::Static,
        RustMemorySymbolKind::Macro => SymbolKindDto::Macro,
    }
}

fn relationship_kind(value: RelationshipKindDto) -> MemoryRelationshipKind {
    match value {
        RelationshipKindDto::Contradicts => MemoryRelationshipKind::Contradicts,
        RelationshipKindDto::Supersedes => MemoryRelationshipKind::Supersedes,
    }
}

fn relationship_kind_dto(value: MemoryRelationshipKind) -> RelationshipKindDto {
    match value {
        MemoryRelationshipKind::Contradicts => RelationshipKindDto::Contradicts,
        MemoryRelationshipKind::Supersedes => RelationshipKindDto::Supersedes,
    }
}
