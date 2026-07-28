//! Pre-acceptance golden and hostile-input evidence for proposed ADR-0014.
//!
//! This is deliberately an integration-test-only format implementation. It
//! must not become a production import path while ADR-0014 remains proposed.

use std::{
    fmt::{self, Write as _},
    str,
};

use granit_parser::{Event, Parser, Scanner, StrInput, Token, TokenType};
use repowitness_application::{
    RepositoryIdentityTextV1, RepositoryPathTextByteLimit, RepositoryPathTextV1,
};
use repowitness_domain::{CanonicalMemoryDigest, RepositoryPathLimits};
use serde::{Deserialize, Serialize};
use serde_saphyr::{DuplicateKeyPolicy, MergeKeyPolicy};
use sha2::{Digest, Sha256};

const MAX_INPUT_BYTES: usize = 64 * 1024;
const MAX_PARENT_DIGESTS: usize = 8;
const MAX_EVIDENCE: usize = 16;
const MAX_RELATIONSHIPS: usize = 16;
const MAX_COMMITS: usize = 16;
const MAX_TITLE_BYTES: usize = 256;
const MAX_BODY_BYTES: usize = 16 * 1024;
const MAX_ACTOR_BYTES: usize = 128;
const MAX_PRODUCER_BYTES: usize = 128;
const MAX_NAME_BYTES: usize = 256;
const MAX_QUALIFIED_NAME_BYTES: usize = 1_024;
const MAX_TOTAL_SCALAR_BYTES: usize = 48 * 1024;
const MAX_CANONICAL_BYTES: usize = 256 * 1024;
const MAX_SOURCE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_INTEROPERABLE_INTEGER: u64 = 9_007_199_254_740_991;
const RECORD_ID_PREFIX: &str = "mem_";
const RECORD_ID_PAYLOAD_BYTES: usize = 26;
const CROCKFORD_BASE32: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const PATH_TEXT_LIMIT: RepositoryPathTextByteLimit =
    RepositoryPathTextByteLimit::new(MAX_INPUT_BYTES as u64);
const PATH_LIMITS: RepositoryPathLimits = RepositoryPathLimits::new(32_764, 32_764);

const COMMIT_YAML: &[u8] = include_bytes!("fixtures/memory-v1/commit.yaml");
const COMMIT_YAML_SHA256: &str = "916d2366754e37a20ac49416172a88815d5bd47aa5477c5eaac41062e7c90c1f";
const COMMIT_CANONICAL: &str = include_str!("fixtures/memory-v1/commit.canonical.json");
const COMMIT_DIGEST: &str = include_str!("fixtures/memory-v1/commit.digest");
const WORKTREE_YAML: &[u8] = include_bytes!("fixtures/memory-v1/worktree-relationship.yaml");
const WORKTREE_YAML_SHA256: &str =
    "762a1220300cc182a129c20864dd15c3bdbc4a59b997ecb3c963f970a7b8e083";
const WORKTREE_CANONICAL: &str =
    include_str!("fixtures/memory-v1/worktree-relationship.canonical.json");
const WORKTREE_DIGEST: &str = include_str!("fixtures/memory-v1/worktree-relationship.digest");

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum MemoryKind {
    Decision,
    Failure,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum ProvenanceOrigin {
    Human,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum ActorKind {
    LocalAsserted,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Assurance {
    LocallyApproved,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Lifecycle {
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
enum ObjectFormat {
    Sha1,
    Sha256,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum EvidenceKind {
    RustSymbol,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum SymbolKind {
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
enum RelationshipKind {
    Contradicts,
    Supersedes,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MemoryRecordDto {
    schema_version: u32,
    record_id: String,
    display_revision: u32,
    parent_revision_digests: Vec<String>,
    kind: MemoryKind,
    title: String,
    body: String,
    scope: ScopeDto,
    provenance: ProvenanceDto,
    assurance: Assurance,
    lifecycle: Lifecycle,
    validity: ValidityDto,
    evidence: Vec<RustSymbolEvidenceDto>,
    relationships: Vec<RelationshipDto>,
    tombstone: bool,
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
    origin: ProvenanceOrigin,
    actor_kind: ActorKind,
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
    object_format: ObjectFormat,
    object_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RustSymbolEvidenceDto {
    kind: EvidenceKind,
    source_snapshot_digest: String,
    path: String,
    content_digest: String,
    artifact_digest: String,
    fact_ordinal: u64,
    symbol_kind: SymbolKind,
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
    kind: RelationshipKind,
    record_id: String,
    revision_digest: String,
}

#[derive(Clone, Debug)]
struct ValidatedMemoryRecord(MemoryRecordDto);

#[derive(Serialize)]
struct CanonicalMemoryRecord<'a> {
    schema_version: u32,
    record_id: &'a str,
    parent_revision_digests: &'a [String],
    kind: MemoryKind,
    title: &'a str,
    body: &'a str,
    scope: &'a ScopeDto,
    provenance: &'a ProvenanceDto,
    assurance: Assurance,
    lifecycle: Lifecycle,
    validity: &'a ValidityDto,
    evidence: &'a [RustSymbolEvidenceDto],
    relationships: &'a [RelationshipDto],
    tombstone: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StrictMemoryError {
    InputTooLarge,
    InvalidYaml,
    InvalidRecord,
    CanonicalizationFailed,
    GenerationFailed,
}

impl fmt::Display for StrictMemoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InputTooLarge => "memory YAML exceeds its byte limit",
            Self::InvalidYaml => "memory YAML is invalid",
            Self::InvalidRecord => "memory record is invalid",
            Self::CanonicalizationFailed => "memory canonicalization failed",
            Self::GenerationFailed => "memory YAML generation failed",
        })
    }
}

#[derive(Default)]
struct YamlPreflight {
    events: usize,
    nodes: usize,
    depth: usize,
    documents: usize,
}

impl YamlPreflight {
    fn observe(&mut self, event: Event<'_>) -> Result<(), StrictMemoryError> {
        increment_bounded(&mut self.events, 4_096)?;
        match event {
            Event::DocumentStart(_, version) => {
                if version.is_some() {
                    return Err(StrictMemoryError::InvalidYaml);
                }
                increment_bounded(&mut self.documents, 1)
            }
            Event::Alias(_) => Err(StrictMemoryError::InvalidYaml),
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
    ) -> Result<(), StrictMemoryError> {
        if anchor != 0 || has_tag {
            return Err(StrictMemoryError::InvalidYaml);
        }
        increment_bounded(&mut self.nodes, 2_048)?;
        if opens_collection {
            increment_bounded(&mut self.depth, 8)?;
        }
        Ok(())
    }

    fn close_collection(&mut self) -> Result<(), StrictMemoryError> {
        self.depth = self
            .depth
            .checked_sub(1)
            .ok_or(StrictMemoryError::InvalidYaml)?;
        Ok(())
    }
}

include!("phase0_memory_record_v1_spike/parsing.rs");
include!("phase0_memory_record_v1_spike/serialization.rs");
include!("phase0_memory_record_v1_spike/assertions.rs");
include!("phase0_memory_record_v1_spike/golden_tests.rs");
include!("phase0_memory_record_v1_spike/semantic_tests.rs");
include!("phase0_memory_record_v1_spike/validation_tests.rs");
