use std::{error::Error, fmt};

use crate::{
    AnalysisArtifactDigest, ByteSpan, CanonicalMemoryDigest, DeclarationDigest, ProducerIdentity,
    RepositoryIdentityDigest, RepositoryPath, SourceContentDigest, SourceSnapshotDigest,
};

/// Version of the first accepted Phase 0 engineering-memory semantic shape.
pub const MEMORY_RECORD_SCHEMA_VERSION: u32 = 1;
/// Largest integer admitted into RFC 8785 canonical memory JSON.
pub const MAX_MEMORY_INTEROPERABLE_INTEGER: u64 = 9_007_199_254_740_991;
/// Maximum source byte endpoint admitted by Phase 0 memory evidence.
pub const MAX_MEMORY_SOURCE_BYTES: u64 = 8 * 1024 * 1024;
/// Maximum number of parent revision digests on one memory version.
pub const MAX_MEMORY_PARENTS: usize = 8;
/// Maximum number of evidence entries on one memory version.
pub const MAX_MEMORY_EVIDENCE: usize = 16;
/// Maximum number of relationship entries on one memory version.
pub const MAX_MEMORY_RELATIONSHIPS: usize = 16;
/// Maximum number of introduction or invalidation commits.
pub const MAX_MEMORY_COMMITS: usize = 16;

const MEMORY_RECORD_ID_BYTES: usize = 16;
const MAX_TITLE_BYTES: usize = 256;
const MAX_BODY_BYTES: usize = 16 * 1024;
const MAX_ACTOR_BYTES: usize = 128;
const MAX_PRODUCER_BYTES: usize = 128;
const MAX_NAME_BYTES: usize = 256;
const MAX_QUALIFIED_NAME_BYTES: usize = 1_024;

/// Stable, content-redacted validation failures for Phase 0 memory values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryRecordError {
    /// A semantic schema version was not version 1.
    InvalidSchemaVersion,
    /// A record identifier did not contain exactly 128 bits.
    InvalidRecordId,
    /// A display revision was zero.
    InvalidDisplayRevision,
    /// A text field violated its field-specific encoding or byte bound.
    InvalidText(MemoryTextField),
    /// A numeric field exceeded its version-1 bound.
    InvalidInteger(MemoryIntegerField),
    /// A collection violated its count, uniqueness, or ordering contract.
    InvalidCollection(MemoryCollectionField),
    /// A commit validity expression was empty, duplicated, or contradictory.
    InvalidValidity,
    /// Source evidence violated an identity, span, or containment invariant.
    InvalidEvidence,
    /// A tombstone flag and authored lifecycle did not agree.
    InvalidTombstone,
}

impl fmt::Display for MemoryRecordError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidSchemaVersion => "memory schema version is invalid",
            Self::InvalidRecordId => "memory record identifier is invalid",
            Self::InvalidDisplayRevision => "memory display revision is invalid",
            Self::InvalidText(_) => "memory text field is invalid",
            Self::InvalidInteger(_) => "memory integer field is invalid",
            Self::InvalidCollection(_) => "memory collection is invalid",
            Self::InvalidValidity => "memory validity is invalid",
            Self::InvalidEvidence => "memory evidence is invalid",
            Self::InvalidTombstone => "memory tombstone state is invalid",
        })
    }
}

impl Error for MemoryRecordError {}

/// Text-field categories exposed by redacted validation diagnostics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryTextField {
    /// Human-facing claim title.
    Title,
    /// Human-facing claim body.
    Body,
    /// Locally asserted actor label.
    ActorId,
    /// Trusted local audit actor label supplied outside repository-authored data.
    AuditActorId,
    /// Source symbol name.
    SymbolName,
    /// Qualified source symbol name.
    QualifiedName,
    /// Evidence producer identifier.
    ProducerId,
    /// Evidence producer version.
    ProducerVersion,
}

/// Integer-field categories exposed by redacted validation diagnostics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryIntegerField {
    /// Evidence index selected by record scope.
    EvidenceIndex,
    /// Stable source fact ordinal.
    FactOrdinal,
    /// Nonnegative system-recorded Unix timestamp in milliseconds.
    RecordedAtUnixMillis,
}

/// Collection categories exposed by redacted validation diagnostics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryCollectionField {
    /// Parent revision digests.
    Parents,
    /// Evidence entries.
    Evidence,
    /// Cross-record relationships.
    Relationships,
    /// Introduction or invalidation commits.
    Commits,
}

/// Opaque 128-bit identity of one logical memory record.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct MemoryRecordId([u8; MEMORY_RECORD_ID_BYTES]);

impl MemoryRecordId {
    /// Creates a record identity from exactly 128 opaque bits.
    #[must_use]
    pub const fn new(bytes: [u8; MEMORY_RECORD_ID_BYTES]) -> Self {
        Self(bytes)
    }

    /// Returns the exact opaque identity bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; MEMORY_RECORD_ID_BYTES] {
        &self.0
    }

    /// Consumes the identity and returns its exact bytes.
    #[must_use]
    pub const fn into_bytes(self) -> [u8; MEMORY_RECORD_ID_BYTES] {
        self.0
    }
}

impl fmt::Debug for MemoryRecordId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MemoryRecordId")
            .field("bits", &128_u16)
            .finish_non_exhaustive()
    }
}

/// Nonzero human-facing revision number that is excluded from semantic identity.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct MemoryDisplayRevision(u32);

impl MemoryDisplayRevision {
    /// Validates a nonzero display revision.
    pub const fn try_new(value: u32) -> Result<Self, MemoryRecordError> {
        if value == 0 {
            return Err(MemoryRecordError::InvalidDisplayRevision);
        }
        Ok(Self(value))
    }

    /// Returns the revision number.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// Zero-based semantic evidence index stored in record scope.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct MemoryEvidenceIndex(u64);

impl MemoryEvidenceIndex {
    /// Validates the RFC 8785 interoperable integer bound.
    pub const fn try_new(value: u64) -> Result<Self, MemoryRecordError> {
        if value > MAX_MEMORY_INTEROPERABLE_INTEGER {
            return Err(MemoryRecordError::InvalidInteger(
                MemoryIntegerField::EvidenceIndex,
            ));
        }
        Ok(Self(value))
    }

    /// Returns the zero-based index.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Stable ordinal of one fact within a canonical analysis artifact.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct MemoryFactOrdinal(u64);

impl MemoryFactOrdinal {
    /// Validates the RFC 8785 interoperable integer bound.
    pub const fn try_new(value: u64) -> Result<Self, MemoryRecordError> {
        if value > MAX_MEMORY_INTEROPERABLE_INTEGER {
            return Err(MemoryRecordError::InvalidInteger(
                MemoryIntegerField::FactOrdinal,
            ));
        }
        Ok(Self(value))
    }

    /// Returns the stable fact ordinal.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

macro_rules! define_memory_text {
    ($name:ident, $field:expr, $documentation:literal, $validator:ident) => {
        #[doc = $documentation]
        #[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(Box<str>);

        impl $name {
            /// Validates and owns one field value.
            pub fn try_new(value: String) -> Result<Self, MemoryRecordError> {
                if !$validator(&value) {
                    return Err(MemoryRecordError::InvalidText($field));
                }
                Ok(Self(value.into_boxed_str()))
            }

            /// Returns the validated field text.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// Consumes the value and returns its owned text.
            #[must_use]
            pub fn into_string(self) -> String {
                self.0.into()
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter
                    .debug_struct(stringify!($name))
                    .field("utf8_bytes", &self.0.len())
                    .finish_non_exhaustive()
            }
        }
    };
}

define_memory_text!(
    MemoryTitle,
    MemoryTextField::Title,
    "Validated human-facing title of a memory claim.",
    valid_title
);
define_memory_text!(
    MemoryBody,
    MemoryTextField::Body,
    "Validated human-facing body of a memory claim.",
    valid_body
);
define_memory_text!(
    MemoryActorId,
    MemoryTextField::ActorId,
    "Validated locally asserted actor label.",
    valid_actor
);
define_memory_text!(
    MemoryAuditActorId,
    MemoryTextField::AuditActorId,
    "Validated trusted local audit actor label.",
    valid_actor
);
define_memory_text!(
    MemorySymbolName,
    MemoryTextField::SymbolName,
    "Validated exact UTF-8 source symbol name.",
    valid_symbol_name
);
define_memory_text!(
    MemoryQualifiedName,
    MemoryTextField::QualifiedName,
    "Validated exact UTF-8 qualified source symbol name.",
    valid_qualified_name
);
define_memory_text!(
    MemoryProducerId,
    MemoryTextField::ProducerId,
    "Validated printable-ASCII evidence producer identifier.",
    valid_producer
);
define_memory_text!(
    MemoryProducerVersion,
    MemoryTextField::ProducerVersion,
    "Validated printable-ASCII evidence producer version.",
    valid_producer
);

include!("memory/model.rs");
include!("memory/record.rs");

#[cfg(test)]
mod tests;
