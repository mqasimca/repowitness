/// Validated identity and parentage of one immutable memory version.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemoryRecordHeader {
    record_id: MemoryRecordId,
    display_revision: MemoryDisplayRevision,
    parents: Vec<CanonicalMemoryDigest>,
}

impl MemoryRecordHeader {
    /// Validates, sorts, and owns the version parent set.
    pub fn try_new(
        record_id: MemoryRecordId,
        display_revision: MemoryDisplayRevision,
        mut parents: Vec<CanonicalMemoryDigest>,
    ) -> Result<Self, MemoryRecordError> {
        if parents.len() > MAX_MEMORY_PARENTS {
            return Err(MemoryRecordError::InvalidCollection(
                MemoryCollectionField::Parents,
            ));
        }
        parents.sort_unstable();
        if has_duplicates(&parents) {
            return Err(MemoryRecordError::InvalidCollection(
                MemoryCollectionField::Parents,
            ));
        }
        Ok(Self {
            record_id,
            display_revision,
            parents,
        })
    }

    /// Returns the logical record identity.
    #[must_use]
    pub const fn record_id(&self) -> MemoryRecordId {
        self.record_id
    }

    /// Returns the presentation-only revision number.
    #[must_use]
    pub const fn display_revision(&self) -> MemoryDisplayRevision {
        self.display_revision
    }

    /// Returns canonically sorted parent revision digests.
    #[must_use]
    pub fn parents(&self) -> &[CanonicalMemoryDigest] {
        &self.parents
    }
}

/// Validated human-authored claim content.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemoryClaim {
    kind: MemoryKind,
    title: MemoryTitle,
    body: MemoryBody,
}

impl MemoryClaim {
    /// Creates claim content from validated values.
    #[must_use]
    pub const fn new(kind: MemoryKind, title: MemoryTitle, body: MemoryBody) -> Self {
        Self { kind, title, body }
    }

    /// Returns the claim kind.
    #[must_use]
    pub const fn kind(&self) -> MemoryKind {
        self.kind
    }

    /// Returns the claim title.
    #[must_use]
    pub const fn title(&self) -> &MemoryTitle {
        &self.title
    }

    /// Returns the claim body.
    #[must_use]
    pub const fn body(&self) -> &MemoryBody {
        &self.body
    }
}

/// Complete validated semantic version-1 engineering-memory record.
#[derive(Clone, Eq, PartialEq)]
pub struct MemoryRecord {
    header: MemoryRecordHeader,
    claim: MemoryClaim,
    scope: MemoryScope,
    provenance: MemoryProvenance,
    assurance: MemoryAssurance,
    lifecycle: MemoryLifecycle,
    validity: MemoryValidity,
    evidence: Vec<MemoryEvidence>,
    relationships: Vec<MemoryRelationship>,
    tombstone: bool,
}

impl MemoryRecord {
    /// Validates aggregate bounds, subject selection, relationship uniqueness,
    /// and tombstone state before constructing one semantic record.
    #[allow(
        clippy::too_many_arguments,
        reason = "version-1 fields are a fixed semantic contract"
    )]
    pub fn try_new(
        header: MemoryRecordHeader,
        claim: MemoryClaim,
        scope: MemoryScope,
        provenance: MemoryProvenance,
        assurance: MemoryAssurance,
        lifecycle: MemoryLifecycle,
        validity: MemoryValidity,
        evidence: Vec<MemoryEvidence>,
        mut relationships: Vec<MemoryRelationship>,
        tombstone: bool,
    ) -> Result<Self, MemoryRecordError> {
        if evidence.is_empty() || evidence.len() > MAX_MEMORY_EVIDENCE {
            return Err(MemoryRecordError::InvalidCollection(
                MemoryCollectionField::Evidence,
            ));
        }
        if usize::try_from(scope.subject_evidence().get())
            .ok()
            .is_none_or(|index| index >= evidence.len())
        {
            return Err(MemoryRecordError::InvalidInteger(
                MemoryIntegerField::EvidenceIndex,
            ));
        }
        if relationships.len() > MAX_MEMORY_RELATIONSHIPS {
            return Err(MemoryRecordError::InvalidCollection(
                MemoryCollectionField::Relationships,
            ));
        }
        relationships.sort_unstable();
        if has_duplicates(&relationships) {
            return Err(MemoryRecordError::InvalidCollection(
                MemoryCollectionField::Relationships,
            ));
        }
        let valid_tombstone = if tombstone {
            lifecycle == MemoryLifecycle::Tombstoned && !header.parents().is_empty()
        } else {
            lifecycle != MemoryLifecycle::Tombstoned
        };
        if !valid_tombstone {
            return Err(MemoryRecordError::InvalidTombstone);
        }
        Ok(Self {
            header,
            claim,
            scope,
            provenance,
            assurance,
            lifecycle,
            validity,
            evidence,
            relationships,
            tombstone,
        })
    }

    /// Returns the fixed semantic schema version.
    #[must_use]
    pub const fn schema_version(&self) -> u32 {
        MEMORY_RECORD_SCHEMA_VERSION
    }

    /// Returns version identity and parentage.
    #[must_use]
    pub const fn header(&self) -> &MemoryRecordHeader {
        &self.header
    }

    /// Returns validated human-authored claim content.
    #[must_use]
    pub const fn claim(&self) -> &MemoryClaim {
        &self.claim
    }

    /// Returns repository and subject-evidence scope.
    #[must_use]
    pub const fn scope(&self) -> MemoryScope {
        self.scope
    }

    /// Returns repository-authored provenance.
    #[must_use]
    pub const fn provenance(&self) -> &MemoryProvenance {
        &self.provenance
    }

    /// Returns repository-authored assurance.
    #[must_use]
    pub const fn assurance(&self) -> MemoryAssurance {
        self.assurance
    }

    /// Returns repository-authored lifecycle.
    #[must_use]
    pub const fn lifecycle(&self) -> MemoryLifecycle {
        self.lifecycle
    }

    /// Returns project-valid scope.
    #[must_use]
    pub const fn validity(&self) -> &MemoryValidity {
        &self.validity
    }

    /// Returns semantic evidence in authored order.
    #[must_use]
    pub fn evidence(&self) -> &[MemoryEvidence] {
        &self.evidence
    }

    /// Returns canonically sorted cross-record relationships.
    #[must_use]
    pub fn relationships(&self) -> &[MemoryRelationship] {
        &self.relationships
    }

    /// Reports whether this version is an explicit tombstone.
    #[must_use]
    pub const fn tombstone(&self) -> bool {
        self.tombstone
    }
}

impl fmt::Debug for MemoryRecord {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MemoryRecord")
            .field("schema_version", &MEMORY_RECORD_SCHEMA_VERSION)
            .field("record_id", &self.header.record_id)
            .field("display_revision", &self.header.display_revision)
            .field("parent_count", &self.header.parents.len())
            .field("kind", &self.claim.kind)
            .field("lifecycle", &self.lifecycle)
            .field("evidence_count", &self.evidence.len())
            .field("relationship_count", &self.relationships.len())
            .field("tombstone", &self.tombstone)
            .finish_non_exhaustive()
    }
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

fn valid_actor(value: &str) -> bool {
    valid_printable_ascii(value, MAX_ACTOR_BYTES)
}

fn valid_symbol_name(value: &str) -> bool {
    valid_source_name(value, MAX_NAME_BYTES)
}

fn valid_qualified_name(value: &str) -> bool {
    valid_source_name(value, MAX_QUALIFIED_NAME_BYTES)
}

fn valid_producer(value: &str) -> bool {
    valid_printable_ascii(value, MAX_PRODUCER_BYTES)
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

fn has_duplicates<T: PartialEq>(values: &[T]) -> bool {
    values.windows(2).any(|pair| pair[0] == pair[1])
}
