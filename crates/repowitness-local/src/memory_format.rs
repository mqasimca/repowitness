use std::{
    error::Error,
    fmt::{self, Write as _},
    io, str,
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

use granit_parser::{Event, Parser, Scanner, StrInput, Token, TokenType};
use repowitness_application::{
    MemoryRecordIdTextV1, RepositoryIdentityTextV1, RepositoryPathTextByteLimit,
    RepositoryPathTextV1,
};
use repowitness_domain::{
    AnalysisArtifactDigest, ByteOffset, ByteSpan, CanonicalMemoryDigest, DeclarationDigest,
    MAX_MEMORY_INTEROPERABLE_INTEGER, MAX_MEMORY_SOURCE_BYTES,
    MEMORY_RECORD_CURRENT_SCHEMA_VERSION, MEMORY_RECORD_PROFILE_V2_SCHEMA_VERSION,
    MEMORY_RECORD_SCHEMA_VERSION, MemoryActorId, MemoryActorKind, MemoryAssurance, MemoryBody,
    MemoryClaim, MemoryCommitId, MemoryDisplayRevision, MemoryEvidence, MemoryEvidenceIndex,
    MemoryFactOrdinal, MemoryKind, MemoryLifecycle, MemoryObjectFormat, MemoryProducerId,
    MemoryProducerVersion, MemoryProvenance, MemoryProvenanceOrigin, MemoryQualifiedName,
    MemoryRecord, MemoryRecordError, MemoryRecordHeader, MemoryRelationship,
    MemoryRelationshipKind, MemoryScope, MemorySymbolName, MemoryTitle, MemoryValidity,
    ProducerIdentity, RepositoryPathLimits, RustMemorySymbolKind, RustSymbolMemoryEvidence,
    SourceContentDigest, SourceSnapshotDigest,
};
use serde::{Deserialize, Serialize};
use serde_saphyr::{DuplicateKeyPolicy, MergeKeyPolicy};
use sha2::{Digest, Sha256};

/// Maximum admitted UTF-8 YAML bytes for one accepted memory profile.
pub const MAX_MEMORY_YAML_BYTES: usize = 64 * 1024;
/// Maximum aggregate decoded YAML scalar bytes for one memory record.
pub const MAX_MEMORY_SCALAR_BYTES: usize = 48 * 1024;
/// Maximum emitted RFC 8785 canonical JSON bytes for one memory record.
pub const MAX_CANONICAL_MEMORY_BYTES: usize = 256 * 1024;

const MAX_MEMORY_COMMENT_BYTES: usize = 4 * 1024;
const MAX_MEMORY_YAML_EVENTS: usize = 4_096;
const MAX_MEMORY_YAML_NODES: usize = 2_048;
const MAX_MEMORY_YAML_DEPTH: usize = 8;
const PATH_TEXT_LIMIT: RepositoryPathTextByteLimit =
    RepositoryPathTextByteLimit::new(MAX_MEMORY_YAML_BYTES as u64);
const PATH_LIMITS: RepositoryPathLimits = RepositoryPathLimits::new(32_764, 32_764);

/// Cooperative cancellation and absolute deadline for memory-format work.
#[derive(Clone, Copy)]
pub struct MemoryFormatControl<'a> {
    cancelled: &'a AtomicBool,
    deadline: Instant,
}

impl<'a> MemoryFormatControl<'a> {
    /// Creates explicit operation control for parsing, canonicalization, or generation.
    #[must_use]
    pub const fn new(cancelled: &'a AtomicBool, deadline: Instant) -> Self {
        Self {
            cancelled,
            deadline,
        }
    }

    /// Returns the shared cancellation flag.
    #[must_use]
    pub const fn cancelled(self) -> &'a AtomicBool {
        self.cancelled
    }

    /// Returns the absolute operation deadline.
    #[must_use]
    pub const fn deadline(self) -> Instant {
        self.deadline
    }
}

impl fmt::Debug for MemoryFormatControl<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MemoryFormatControl")
            .field("cancelled", &self.cancelled.load(Ordering::Relaxed))
            .field("deadline_configured", &true)
            .finish()
    }
}

/// Stable, content-redacted failures from the hostile memory-file boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryFormatError {
    /// Input exceeded the fixed 64 KiB YAML bound.
    InputTooLarge,
    /// YAML encoding, syntax, feature, or parser-budget validation failed.
    InvalidYaml,
    /// The decoded semantic object violated an accepted memory-profile contract.
    InvalidRecord(MemoryRecordError),
    /// Persisted canonical JSON was malformed, non-canonical, or misidentified.
    InvalidCanonicalRecord,
    /// RFC 8785 serialization or its output bound failed.
    CanonicalizationFailed,
    /// Deterministic YAML generation or its output bound failed.
    GenerationFailed,
    /// The caller cancelled the operation.
    Cancelled,
    /// The absolute operation deadline elapsed.
    DeadlineExceeded,
}

impl fmt::Display for MemoryFormatError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InputTooLarge => "memory YAML exceeds its byte limit",
            Self::InvalidYaml => "memory YAML is invalid",
            Self::InvalidRecord(_) => "memory record is invalid",
            Self::InvalidCanonicalRecord => "persisted canonical memory record is invalid",
            Self::CanonicalizationFailed => "memory canonicalization failed",
            Self::GenerationFailed => "memory YAML generation failed",
            Self::Cancelled => "memory format operation was cancelled",
            Self::DeadlineExceeded => "memory format operation exceeded its deadline",
        })
    }
}

impl Error for MemoryFormatError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidRecord(error) => Some(error),
            _ => None,
        }
    }
}

impl From<MemoryRecordError> for MemoryFormatError {
    fn from(error: MemoryRecordError) -> Self {
        Self::InvalidRecord(error)
    }
}

/// One fully parsed accepted-profile record with its exact canonical identity material.
#[derive(Clone, Eq, PartialEq)]
pub struct ParsedMemoryRecord {
    record: MemoryRecord,
    canonical_json: Box<[u8]>,
    digest: CanonicalMemoryDigest,
}

impl ParsedMemoryRecord {
    /// Returns the validated semantic record.
    #[must_use]
    pub const fn record(&self) -> &MemoryRecord {
        &self.record
    }

    /// Returns the exact RFC 8785 canonical JSON bytes.
    #[must_use]
    pub const fn canonical_json(&self) -> &[u8] {
        &self.canonical_json
    }

    /// Returns the domain-separated canonical semantic digest.
    #[must_use]
    pub const fn digest(&self) -> CanonicalMemoryDigest {
        self.digest
    }

    /// Consumes the parsed result and returns the validated record.
    #[must_use]
    pub fn into_record(self) -> MemoryRecord {
        self.record
    }
}

impl fmt::Debug for ParsedMemoryRecord {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ParsedMemoryRecord")
            .field("record", &self.record)
            .field("canonical_bytes", &self.canonical_json.len())
            .field("digest", &self.digest)
            .finish()
    }
}

#[derive(Default)]
struct CanonicalMemoryOutput {
    bytes: Vec<u8>,
}

impl CanonicalMemoryOutput {
    fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

impl io::Write for CanonicalMemoryOutput {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let remaining = MAX_CANONICAL_MEMORY_BYTES.saturating_sub(self.bytes.len());
        if bytes.len() > remaining {
            return Err(io::Error::other("canonical memory output limit exceeded"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Default)]
struct MemoryYamlOutput {
    text: String,
}

impl MemoryYamlOutput {
    fn into_bytes(self) -> Vec<u8> {
        self.text.into_bytes()
    }
}

impl fmt::Write for MemoryYamlOutput {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        let remaining = MAX_MEMORY_YAML_BYTES.saturating_sub(self.text.len());
        if value.len() > remaining {
            return Err(fmt::Error);
        }
        self.text.push_str(value);
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum MemoryKindDto {
    Decision,
    Failure,
    Fact,
    Procedure,
    Episode,
    Preference,
    Policy,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum ProvenanceOriginDto {
    Human,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum ActorKindDto {
    LocalAsserted,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum AssuranceDto {
    LocallyApproved,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum LifecycleDto {
    Active,
    NeedsReview,
    Stale,
    Contradicted,
    Superseded,
    Quarantined,
    Tombstoned,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
enum ObjectFormatDto {
    Sha1,
    Sha256,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum EvidenceKindDto {
    RustSymbol,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum SymbolKindDto {
    Function,
    Method,
    Struct,
    Enum,
    Union,
    Trait,
    Module,
    TypeAlias,
    Constant,
    Static,
    Macro,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
enum RelationshipKindDto {
    Contradicts,
    Supersedes,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MemoryRecordDto {
    #[serde(default = "default_memory_schema_version")]
    schema_version: u32,
    record_id: String,
    display_revision: u32,
    parent_revision_digests: Vec<String>,
    kind: MemoryKindDto,
    title: String,
    body: String,
    scope: ScopeDto,
    provenance: ProvenanceDto,
    assurance: AssuranceDto,
    lifecycle: LifecycleDto,
    validity: ValidityDto,
    evidence: Vec<RustSymbolEvidenceDto>,
    relationships: Vec<RelationshipDto>,
    tombstone: bool,
}

const fn default_memory_schema_version() -> u32 {
    MEMORY_RECORD_CURRENT_SCHEMA_VERSION
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ScopeDto {
    repository_id: String,
    subject_evidence: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProvenanceDto {
    origin: ProvenanceOriginDto,
    actor_kind: ActorKindDto,
    actor_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum ValidityDto {
    Commits {
        introduced_by: Vec<CommitIdDto>,
        invalidated_by: Vec<CommitIdDto>,
    },
    Worktree {
        source_snapshot_digest: String,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
struct CommitIdDto {
    object_format: ObjectFormatDto,
    object_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RustSymbolEvidenceDto {
    kind: EvidenceKindDto,
    source_snapshot_digest: String,
    path: String,
    content_digest: String,
    artifact_digest: String,
    fact_ordinal: u64,
    symbol_kind: SymbolKindDto,
    name: String,
    qualified_name: String,
    name_start: u64,
    name_length: u64,
    declaration_start: u64,
    declaration_length: u64,
    declaration_digest: String,
    producer_id: String,
    producer_version: String,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
struct RelationshipDto {
    kind: RelationshipKindDto,
    record_id: String,
    revision_digest: String,
}

#[derive(Serialize)]
struct CanonicalMemoryRecordDto<'a> {
    schema_version: u32,
    record_id: &'a str,
    parent_revision_digests: &'a [String],
    kind: MemoryKindDto,
    title: &'a str,
    body: &'a str,
    scope: &'a ScopeDto,
    provenance: &'a ProvenanceDto,
    assurance: AssuranceDto,
    lifecycle: LifecycleDto,
    validity: &'a ValidityDto,
    evidence: &'a [RustSymbolEvidenceDto],
    relationships: &'a [RelationshipDto],
    tombstone: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PersistedCanonicalMemoryRecordDto {
    schema_version: u32,
    record_id: String,
    parent_revision_digests: Vec<String>,
    kind: MemoryKindDto,
    title: String,
    body: String,
    scope: ScopeDto,
    provenance: ProvenanceDto,
    assurance: AssuranceDto,
    lifecycle: LifecycleDto,
    validity: ValidityDto,
    evidence: Vec<RustSymbolEvidenceDto>,
    relationships: Vec<RelationshipDto>,
    tombstone: bool,
}

#[derive(Default)]
struct YamlPreflight {
    events: usize,
    nodes: usize,
    depth: usize,
    documents: usize,
}

impl YamlPreflight {
    fn observe(
        &mut self,
        event: Event<'_>,
        control: MemoryFormatControl<'_>,
    ) -> Result<(), MemoryFormatError> {
        check_control(control)?;
        increment_bounded(&mut self.events, MAX_MEMORY_YAML_EVENTS)?;
        match event {
            Event::DocumentStart(_, version) => {
                if version.is_some() {
                    return Err(MemoryFormatError::InvalidYaml);
                }
                increment_bounded(&mut self.documents, 1)
            }
            Event::Alias(_) => Err(MemoryFormatError::InvalidYaml),
            Event::Scalar(_, _, anchor, tag) => self.observe_node(anchor, tag.is_some(), false),
            Event::SequenceStart(_, anchor, tag) | Event::MappingStart(_, anchor, tag) => {
                self.observe_node(anchor, tag.is_some(), true)
            }
            Event::SequenceEnd | Event::MappingEnd => self.close_collection(),
            Event::Nothing
            | Event::StreamStart
            | Event::StreamEnd
            | Event::DocumentEnd
            | Event::Comment(..) => Ok(()),
        }
    }

    fn observe_node(
        &mut self,
        anchor: usize,
        has_tag: bool,
        opens_collection: bool,
    ) -> Result<(), MemoryFormatError> {
        if anchor != 0 || has_tag {
            return Err(MemoryFormatError::InvalidYaml);
        }
        increment_bounded(&mut self.nodes, MAX_MEMORY_YAML_NODES)?;
        if opens_collection {
            increment_bounded(&mut self.depth, MAX_MEMORY_YAML_DEPTH)?;
        }
        Ok(())
    }

    fn close_collection(&mut self) -> Result<(), MemoryFormatError> {
        self.depth = self
            .depth
            .checked_sub(1)
            .ok_or(MemoryFormatError::InvalidYaml)?;
        Ok(())
    }
}

include!("memory_format/canonical.rs");
include!("memory_format/yaml.rs");

#[cfg(test)]
mod tests;
