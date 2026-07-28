/// Phase 0 engineering-memory claim kind.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum MemoryKind {
    /// A reviewed engineering decision.
    Decision,
    /// A reviewed failed approach or failure mode.
    Failure,
}

/// Repository-authored provenance origin supported by schema version 1.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum MemoryProvenanceOrigin {
    /// Authored by a human.
    Human,
}

/// Repository-authored actor strength supported by schema version 1.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum MemoryActorKind {
    /// A local label that is not an authenticated organization principal.
    LocalAsserted,
}

/// Repository-authored assurance claim supported by schema version 1.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum MemoryAssurance {
    /// The file claims local approval; trusted audit state must verify it.
    LocallyApproved,
}

/// Repository-authored lifecycle of one exact memory version.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum MemoryLifecycle {
    /// Authored as active, subject to effective-state checks.
    Active,
    /// Requires manual review before use.
    NeedsReview,
    /// Evidence no longer supports the claim.
    Stale,
    /// Contradicted by another reviewed version.
    Contradicted,
    /// Replaced by another reviewed version.
    Superseded,
    /// Isolated by policy or validation.
    Quarantined,
    /// Explicit deletion marker that preserves history.
    Tombstoned,
}

/// Relationship between immutable memory versions.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum MemoryRelationshipKind {
    /// The target version contradicts this version.
    Contradicts,
    /// The target version supersedes this version.
    Supersedes,
}

/// Trusted manual decision over one exact source-correspondence target.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum MemoryCorrespondenceReviewOperation {
    /// Accept a proposed target occurrence.
    Approved,
    /// Reject one exact target without selecting another.
    Rejected,
    /// Establish an explicit target that need not have been proposed automatically.
    ManualLink,
}

/// Supported Rust declaration category for Phase 0 memory evidence.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum RustMemorySymbolKind {
    /// Free function.
    Function,
    /// Associated or receiver method.
    Method,
    /// Struct declaration.
    Struct,
    /// Enum declaration.
    Enum,
    /// Union declaration.
    Union,
    /// Trait declaration.
    Trait,
    /// Module declaration.
    Module,
    /// Type alias.
    TypeAlias,
    /// Constant declaration.
    Constant,
    /// Static declaration.
    Static,
    /// Macro declaration.
    Macro,
}

/// Validated repository and subject-evidence scope.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MemoryScope {
    repository: RepositoryIdentityDigest,
    subject_evidence: MemoryEvidenceIndex,
}

impl MemoryScope {
    /// Creates a typed scope; record construction validates the selected index.
    #[must_use]
    pub const fn new(
        repository: RepositoryIdentityDigest,
        subject_evidence: MemoryEvidenceIndex,
    ) -> Self {
        Self {
            repository,
            subject_evidence,
        }
    }

    /// Returns the scoped repository.
    #[must_use]
    pub const fn repository(self) -> RepositoryIdentityDigest {
        self.repository
    }

    /// Returns the selected evidence index.
    #[must_use]
    pub const fn subject_evidence(self) -> MemoryEvidenceIndex {
        self.subject_evidence
    }
}

/// Validated repository-authored provenance fields.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemoryProvenance {
    origin: MemoryProvenanceOrigin,
    actor_kind: MemoryActorKind,
    actor_id: MemoryActorId,
}

impl MemoryProvenance {
    /// Creates repository-authored provenance from validated values.
    #[must_use]
    pub const fn new(
        origin: MemoryProvenanceOrigin,
        actor_kind: MemoryActorKind,
        actor_id: MemoryActorId,
    ) -> Self {
        Self {
            origin,
            actor_kind,
            actor_id,
        }
    }

    /// Returns the authored origin.
    #[must_use]
    pub const fn origin(&self) -> MemoryProvenanceOrigin {
        self.origin
    }

    /// Returns the authored actor kind.
    #[must_use]
    pub const fn actor_kind(&self) -> MemoryActorKind {
        self.actor_kind
    }

    /// Returns the locally asserted actor label.
    #[must_use]
    pub const fn actor_id(&self) -> &MemoryActorId {
        &self.actor_id
    }
}

/// Canonical Git object identity admitted by Phase 0 memory validity.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum MemoryCommitId {
    /// Raw 20-byte SHA-1 object identity.
    Sha1([u8; 20]),
    /// Raw 32-byte SHA-256 object identity.
    Sha256([u8; 32]),
}

impl fmt::Debug for MemoryCommitId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MemoryCommitId")
            .field(
                "object_format",
                &match self {
                    Self::Sha1(_) => "sha1",
                    Self::Sha256(_) => "sha256",
                },
            )
            .field("identity_bytes", &self.as_bytes().len())
            .finish_non_exhaustive()
    }
}

impl MemoryCommitId {
    /// Returns the stable object-format label.
    #[must_use]
    pub const fn object_format(self) -> MemoryObjectFormat {
        match self {
            Self::Sha1(_) => MemoryObjectFormat::Sha1,
            Self::Sha256(_) => MemoryObjectFormat::Sha256,
        }
    }

    /// Returns the exact decoded object-ID bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Self::Sha1(bytes) => bytes,
            Self::Sha256(bytes) => bytes,
        }
    }
}

/// Git object format carried by a memory validity commit.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum MemoryObjectFormat {
    /// SHA-1 object format.
    Sha1,
    /// SHA-256 object format.
    Sha256,
}

/// Exact source receipt attached to a trusted memory observation.
#[derive(Clone, Copy, Eq, PartialEq)]
pub enum MemoryObservationSource {
    /// One exact Git commit object.
    Git(MemoryCommitId),
    /// One exact dirty-worktree source snapshot.
    Worktree(SourceSnapshotDigest),
}

impl fmt::Debug for MemoryObservationSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (kind, format, bytes) = match self {
            Self::Git(commit) => (
                "git",
                match commit.object_format() {
                    MemoryObjectFormat::Sha1 => "sha1",
                    MemoryObjectFormat::Sha256 => "sha256",
                },
                commit.as_bytes().len(),
            ),
            Self::Worktree(_) => ("worktree", "source_snapshot", 32),
        };
        formatter
            .debug_struct("MemoryObservationSource")
            .field("kind", &kind)
            .field("format", &format)
            .field("identity_bytes", &bytes)
            .finish_non_exhaustive()
    }
}

/// Nonnegative system-recorded Unix timestamp representable by SQLite.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct MemoryRecordedAtUnixMillis(u64);

impl MemoryRecordedAtUnixMillis {
    /// Validates the signed 64-bit SQLite integer boundary.
    pub const fn try_new(value: u64) -> Result<Self, MemoryRecordError> {
        if value > i64::MAX as u64 {
            return Err(MemoryRecordError::InvalidInteger(
                MemoryIntegerField::RecordedAtUnixMillis,
            ));
        }
        Ok(Self(value))
    }

    /// Returns the exact nonnegative Unix timestamp in milliseconds.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Project-valid scope for one memory record version.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MemoryValidity {
    /// Git-DAG validity with one or more introduction commits.
    Commits {
        /// Canonically sorted unique introduction commits.
        introduced_by: Vec<MemoryCommitId>,
        /// Canonically sorted unique invalidation commits.
        invalidated_by: Vec<MemoryCommitId>,
    },
    /// Exact dirty-worktree snapshot validity.
    Worktree {
        /// Exact canonical source-snapshot identity.
        source_snapshot: SourceSnapshotDigest,
    },
}

impl MemoryValidity {
    /// Validates, sorts, and owns Git introduction and invalidation sets.
    pub fn try_commits(
        mut introduced_by: Vec<MemoryCommitId>,
        mut invalidated_by: Vec<MemoryCommitId>,
    ) -> Result<Self, MemoryRecordError> {
        if introduced_by.is_empty()
            || introduced_by.len() > MAX_MEMORY_COMMITS
            || invalidated_by.len() > MAX_MEMORY_COMMITS
        {
            return Err(MemoryRecordError::InvalidCollection(
                MemoryCollectionField::Commits,
            ));
        }
        introduced_by.sort_unstable();
        invalidated_by.sort_unstable();
        if has_duplicates(&introduced_by)
            || has_duplicates(&invalidated_by)
            || introduced_by
                .iter()
                .any(|commit| invalidated_by.binary_search(commit).is_ok())
        {
            return Err(MemoryRecordError::InvalidValidity);
        }
        Ok(Self::Commits {
            introduced_by,
            invalidated_by,
        })
    }

    /// Creates exact dirty-worktree validity.
    #[must_use]
    pub const fn worktree(source_snapshot: SourceSnapshotDigest) -> Self {
        Self::Worktree { source_snapshot }
    }
}

/// Exact syntax evidence binding a memory claim to one Rust occurrence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RustSymbolMemoryEvidence {
    source_snapshot: SourceSnapshotDigest,
    path: RepositoryPath,
    content: SourceContentDigest,
    artifact: AnalysisArtifactDigest,
    fact_ordinal: MemoryFactOrdinal,
    symbol_kind: RustMemorySymbolKind,
    name: MemorySymbolName,
    qualified_name: MemoryQualifiedName,
    name_span: ByteSpan,
    declaration_span: ByteSpan,
    declaration_digest: DeclarationDigest,
    producer: ProducerIdentity<MemoryProducerId, MemoryProducerVersion>,
}

impl RustSymbolMemoryEvidence {
    /// Validates all exact occurrence, span, and producer invariants.
    #[allow(
        clippy::too_many_arguments,
        reason = "every evidence identity input is semantic"
    )]
    pub fn try_new(
        source_snapshot: SourceSnapshotDigest,
        path: RepositoryPath,
        content: SourceContentDigest,
        artifact: AnalysisArtifactDigest,
        fact_ordinal: MemoryFactOrdinal,
        symbol_kind: RustMemorySymbolKind,
        name: MemorySymbolName,
        qualified_name: MemoryQualifiedName,
        name_span: ByteSpan,
        declaration_span: ByteSpan,
        declaration_digest: DeclarationDigest,
        producer: ProducerIdentity<MemoryProducerId, MemoryProducerVersion>,
    ) -> Result<Self, MemoryRecordError> {
        if name_span.is_empty()
            || declaration_span.is_empty()
            || name_span.end().get() > MAX_MEMORY_SOURCE_BYTES
            || declaration_span.end().get() > MAX_MEMORY_SOURCE_BYTES
            || name_span.start().get() < declaration_span.start().get()
            || name_span.end().get() > declaration_span.end().get()
            || name_span.len().get() != u64::try_from(name.as_str().len()).unwrap_or(u64::MAX)
        {
            return Err(MemoryRecordError::InvalidEvidence);
        }
        Ok(Self {
            source_snapshot,
            path,
            content,
            artifact,
            fact_ordinal,
            symbol_kind,
            name,
            qualified_name,
            name_span,
            declaration_span,
            declaration_digest,
            producer,
        })
    }

    /// Returns the exact source snapshot.
    #[must_use]
    pub const fn source_snapshot(&self) -> SourceSnapshotDigest {
        self.source_snapshot
    }

    /// Returns the repository-relative source path.
    #[must_use]
    pub const fn path(&self) -> &RepositoryPath {
        &self.path
    }

    /// Returns the exact file-content digest.
    #[must_use]
    pub const fn content(&self) -> SourceContentDigest {
        self.content
    }

    /// Returns the exact analysis-artifact digest.
    #[must_use]
    pub const fn artifact(&self) -> AnalysisArtifactDigest {
        self.artifact
    }

    /// Returns the source fact ordinal.
    #[must_use]
    pub const fn fact_ordinal(&self) -> MemoryFactOrdinal {
        self.fact_ordinal
    }

    /// Returns the categorical Rust symbol kind.
    #[must_use]
    pub const fn symbol_kind(&self) -> RustMemorySymbolKind {
        self.symbol_kind
    }

    /// Returns the exact symbol name.
    #[must_use]
    pub const fn name(&self) -> &MemorySymbolName {
        &self.name
    }

    /// Returns the exact qualified symbol name.
    #[must_use]
    pub const fn qualified_name(&self) -> &MemoryQualifiedName {
        &self.qualified_name
    }

    /// Returns the exact identifier byte span.
    #[must_use]
    pub const fn name_span(&self) -> ByteSpan {
        self.name_span
    }

    /// Returns the exact declaration byte span.
    #[must_use]
    pub const fn declaration_span(&self) -> ByteSpan {
        self.declaration_span
    }

    /// Returns the digest of the exact declaration bytes.
    #[must_use]
    pub const fn declaration_digest(&self) -> DeclarationDigest {
        self.declaration_digest
    }

    /// Returns the exact evidence producer identity.
    #[must_use]
    pub const fn producer(&self) -> &ProducerIdentity<MemoryProducerId, MemoryProducerVersion> {
        &self.producer
    }
}

/// Evidence classes admitted by Phase 0 memory schema version 1.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MemoryEvidence {
    /// Exact Rust syntax occurrence evidence.
    RustSymbol(RustSymbolMemoryEvidence),
}

/// Attributed relation to another immutable memory version.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct MemoryRelationship {
    kind: MemoryRelationshipKind,
    record_id: MemoryRecordId,
    revision_digest: CanonicalMemoryDigest,
}

impl MemoryRelationship {
    /// Creates a fully attributed relationship.
    #[must_use]
    pub const fn new(
        kind: MemoryRelationshipKind,
        record_id: MemoryRecordId,
        revision_digest: CanonicalMemoryDigest,
    ) -> Self {
        Self {
            kind,
            record_id,
            revision_digest,
        }
    }

    /// Returns the relationship kind.
    #[must_use]
    pub const fn kind(&self) -> MemoryRelationshipKind {
        self.kind
    }

    /// Returns the target logical record.
    #[must_use]
    pub const fn record_id(&self) -> MemoryRecordId {
        self.record_id
    }

    /// Returns the exact target revision.
    #[must_use]
    pub const fn revision_digest(&self) -> CanonicalMemoryDigest {
        self.revision_digest
    }
}
