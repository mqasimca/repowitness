//! Pure, local-only Phase 3 personal-memory identities and immutable records.

use std::fmt;

use crate::{MemoryLifecycle, RepositoryIdentityDigest, TaskText};

/// Opaque local profile identity. It is never a repository, account, or actor name.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PersonalMemoryProfileId([u8; 16]);

impl PersonalMemoryProfileId {
    /// Constructs an opaque profile identity from trusted local bytes.
    #[must_use]
    pub const fn new(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    /// Returns exact storage bytes.
    #[must_use]
    pub const fn as_bytes(self) -> [u8; 16] {
        self.0
    }
}

impl fmt::Debug for PersonalMemoryProfileId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PersonalMemoryProfileId(<redacted>)")
    }
}

/// Opaque identity shared by all immutable revisions of one personal record.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PersonalMemoryId([u8; 16]);

impl PersonalMemoryId {
    /// Constructs an opaque local record identity from trusted local bytes.
    #[must_use]
    pub const fn new(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    /// Returns exact storage bytes.
    #[must_use]
    pub const fn as_bytes(self) -> [u8; 16] {
        self.0
    }
}

impl fmt::Debug for PersonalMemoryId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PersonalMemoryId(<redacted>)")
    }
}

/// Opaque immutable identity for one personal-memory revision.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PersonalMemoryRevision([u8; 32]);

impl PersonalMemoryRevision {
    /// Constructs a revision identity computed by the trusted local boundary.
    #[must_use]
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Returns exact storage bytes.
    #[must_use]
    pub const fn as_bytes(self) -> [u8; 32] {
        self.0
    }
}

impl fmt::Debug for PersonalMemoryRevision {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PersonalMemoryRevision(<redacted>)")
    }
}

/// Additional Phase 3 memory kinds. Version-1 team records retain their original enum.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum PersonalMemoryKind {
    /// A non-source-derivable bounded fact.
    Fact,
    /// A decision and its rationale.
    Decision,
    /// A reusable procedure, subject to independent verification evidence.
    Procedure,
    /// A bounded historical event or incident.
    Episode,
    /// A local preference that cannot be promoted implicitly.
    Preference,
    /// A local policy or guardrail.
    Policy,
    /// A failed approach and its constrained applicability.
    Failure,
}

/// Immutable local-only memory revision. The text fields use the same bounded,
/// redacted value type as durable work state; secret scanning occurs at I/O admission.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PersonalMemoryRecord {
    profile: PersonalMemoryProfileId,
    repository: RepositoryIdentityDigest,
    record_id: PersonalMemoryId,
    revision: PersonalMemoryRevision,
    kind: PersonalMemoryKind,
    title: TaskText,
    body: TaskText,
    lifecycle: MemoryLifecycle,
    recorded_at_unix_ms: u64,
}

impl PersonalMemoryRecord {
    /// Creates one immutable personal-memory revision.
    #[allow(
        clippy::too_many_arguments,
        reason = "each personal-memory scope and revision identity stays explicit"
    )]
    #[must_use]
    pub const fn new(
        profile: PersonalMemoryProfileId,
        repository: RepositoryIdentityDigest,
        record_id: PersonalMemoryId,
        revision: PersonalMemoryRevision,
        kind: PersonalMemoryKind,
        title: TaskText,
        body: TaskText,
        lifecycle: MemoryLifecycle,
        recorded_at_unix_ms: u64,
    ) -> Self {
        Self {
            profile,
            repository,
            record_id,
            revision,
            kind,
            title,
            body,
            lifecycle,
            recorded_at_unix_ms,
        }
    }

    /// Returns the explicit local profile scope.
    #[must_use]
    pub const fn profile(&self) -> PersonalMemoryProfileId {
        self.profile
    }
    /// Returns the explicit repository scope.
    #[must_use]
    pub const fn repository(&self) -> RepositoryIdentityDigest {
        self.repository
    }
    /// Returns the local record identity.
    #[must_use]
    pub const fn record_id(&self) -> PersonalMemoryId {
        self.record_id
    }
    /// Returns the immutable revision identity.
    #[must_use]
    pub const fn revision(&self) -> PersonalMemoryRevision {
        self.revision
    }
    /// Returns the versioned memory kind.
    #[must_use]
    pub const fn kind(&self) -> PersonalMemoryKind {
        self.kind
    }
    /// Returns bounded title text.
    #[must_use]
    pub const fn title(&self) -> &TaskText {
        &self.title
    }
    /// Returns bounded body text.
    #[must_use]
    pub const fn body(&self) -> &TaskText {
        &self.body
    }
    /// Returns the lifecycle asserted by this immutable revision.
    #[must_use]
    pub const fn lifecycle(&self) -> MemoryLifecycle {
        self.lifecycle
    }
    /// Returns the trusted local recorded timestamp.
    #[must_use]
    pub const fn recorded_at_unix_ms(&self) -> u64 {
        self.recorded_at_unix_ms
    }
}
