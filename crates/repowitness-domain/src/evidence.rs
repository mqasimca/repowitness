//! Structured identity and provenance for inspectable evidence.

use core::fmt;

/// A zero-based byte offset into one exact source blob.
///
/// Offsets use a fixed-width representation so persisted and wire formats
/// never inherit the platform-dependent width of `usize`.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ByteOffset(u64);

impl ByteOffset {
    /// The first byte position.
    pub const ZERO: Self = Self(0);

    /// Creates an offset from its fixed-width representation.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the fixed-width representation.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// A fixed-width number of bytes.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ByteLength(u64);

impl ByteLength {
    /// No bytes.
    pub const ZERO: Self = Self(0);

    /// Creates a length from its fixed-width representation.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the fixed-width representation.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Failure to construct a valid half-open byte span.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ByteSpanError {
    /// The exclusive end precedes the inclusive start.
    EndBeforeStart {
        /// The requested inclusive start.
        start: ByteOffset,
        /// The requested exclusive end.
        end: ByteOffset,
    },
}

impl fmt::Display for ByteSpanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EndBeforeStart { start, end } => write!(
                formatter,
                "byte span end {} precedes start {}",
                end.get(),
                start.get()
            ),
        }
    }
}

impl std::error::Error for ByteSpanError {}

/// A validated half-open byte span `[start, end)` in one exact source blob.
///
/// Empty spans are valid and identify a point or insertion boundary. Content
/// owners must separately validate that both offsets are within the referenced
/// blob.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ByteSpan {
    start: ByteOffset,
    end: ByteOffset,
}

impl ByteSpan {
    /// Creates a half-open span after validating its endpoint order.
    ///
    /// # Errors
    ///
    /// Returns [`ByteSpanError::EndBeforeStart`] when `end` precedes `start`.
    pub const fn try_new(start: ByteOffset, end: ByteOffset) -> Result<Self, ByteSpanError> {
        if end.get() < start.get() {
            return Err(ByteSpanError::EndBeforeStart { start, end });
        }

        Ok(Self { start, end })
    }

    /// Returns the inclusive start offset.
    #[must_use]
    pub const fn start(self) -> ByteOffset {
        self.start
    }

    /// Returns the exclusive end offset.
    #[must_use]
    pub const fn end(self) -> ByteOffset {
        self.end
    }

    /// Returns the number of bytes covered by the span.
    #[must_use]
    pub const fn len(self) -> ByteLength {
        ByteLength::new(self.end.get() - self.start.get())
    }

    /// Returns whether the span identifies a point without covering a byte.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.start.get() == self.end.get()
    }
}

/// The most specific stable location available for one evidence record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EvidenceLocation<O> {
    /// The evidence applies to the referenced file as a whole.
    WholeFile,
    /// The evidence applies to an exact half-open byte span.
    ByteSpan(ByteSpan),
    /// The evidence applies to a validated symbol occurrence.
    SymbolOccurrence(O),
}

/// Durable source identity for one inspectable piece of evidence.
///
/// `R` is a validated repository identity, `S` a concrete revision or
/// worktree snapshot, `F` a normalized repository-relative path, `D` the
/// digest of the exact file content, and `O` a validated symbol occurrence
/// identity. Their concrete encodings remain independent of this structure.
/// Line and column positions are intentionally excluded because they are
/// display metadata rather than durable identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EvidenceIdentity<R, S, F, D, O> {
    repository: R,
    snapshot: S,
    path: F,
    content_digest: D,
    location: EvidenceLocation<O>,
}

impl<R, S, F, D, O> EvidenceIdentity<R, S, F, D, O> {
    /// Creates an identity from already-validated components.
    #[must_use]
    pub const fn new(
        repository: R,
        snapshot: S,
        path: F,
        content_digest: D,
        location: EvidenceLocation<O>,
    ) -> Self {
        Self {
            repository,
            snapshot,
            path,
            content_digest,
            location,
        }
    }

    /// Returns the repository identity.
    #[must_use]
    pub const fn repository(&self) -> &R {
        &self.repository
    }

    /// Returns the concrete revision or worktree snapshot.
    #[must_use]
    pub const fn snapshot(&self) -> &S {
        &self.snapshot
    }

    /// Returns the normalized repository-relative path.
    #[must_use]
    pub const fn path(&self) -> &F {
        &self.path
    }

    /// Returns the digest of the exact referenced file content.
    #[must_use]
    pub const fn content_digest(&self) -> &D {
        &self.content_digest
    }

    /// Returns the most specific available location.
    #[must_use]
    pub const fn location(&self) -> &EvidenceLocation<O> {
        &self.location
    }
}

/// Identity and version of the component that produced evidence.
///
/// Concrete producer IDs and versions remain validated component types so an
/// adapter can represent its actual versioning guarantees without leaking a
/// wire or persistence schema into the domain.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProducerIdentity<I, V> {
    id: I,
    version: V,
}

impl<I, V> ProducerIdentity<I, V> {
    /// Creates a producer identity from validated components.
    #[must_use]
    pub const fn new(id: I, version: V) -> Self {
        Self { id, version }
    }

    /// Returns the producer identifier.
    #[must_use]
    pub const fn id(&self) -> &I {
        &self.id
    }

    /// Returns the producer version.
    #[must_use]
    pub const fn version(&self) -> &V {
        &self.version
    }
}

/// The producer class from which evidence originated.
///
/// Tiers describe provenance and expected precision. Their order is not a
/// calibrated confidence score, so this type intentionally does not implement
/// ordering traits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EvidenceTier {
    /// Evidence emitted by a compiler or a SCIP index.
    CompilerOrScip,
    /// Evidence emitted by a language server.
    LanguageServer,
    /// Evidence obtained directly from parsed syntax.
    Syntax,
    /// Evidence produced by a documented heuristic.
    Heuristic,
    /// Evidence obtained from observed runtime behavior.
    RuntimeObservation,
    /// Evidence explicitly asserted by a person.
    HumanAssertion,
}

/// How one evidence record relates to a claim.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EvidenceRelation {
    /// The evidence supports the claim.
    Supports,
    /// The evidence contradicts the claim.
    Contradicts,
}

/// Attributed evidence associated with a material result.
///
/// `I` is the validated evidence identity and `P` is the validated producer
/// identity. Their concrete encodings remain separate from this envelope.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceRecord<I, P> {
    identity: I,
    producer: P,
    tier: EvidenceTier,
    relation: EvidenceRelation,
}

impl<I, P> EvidenceRecord<I, P> {
    /// Creates an attributed evidence record.
    #[must_use]
    pub const fn new(
        identity: I,
        producer: P,
        tier: EvidenceTier,
        relation: EvidenceRelation,
    ) -> Self {
        Self {
            identity,
            producer,
            tier,
            relation,
        }
    }

    /// Returns the validated evidence identity.
    #[must_use]
    pub const fn identity(&self) -> &I {
        &self.identity
    }

    /// Returns the validated producer identity.
    #[must_use]
    pub const fn producer(&self) -> &P {
        &self.producer
    }

    /// Returns the evidence provenance tier.
    #[must_use]
    pub const fn tier(&self) -> EvidenceTier {
        self.tier
    }

    /// Returns how the evidence relates to the claim.
    #[must_use]
    pub const fn relation(&self) -> EvidenceRelation {
        self.relation
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ByteLength, ByteOffset, ByteSpan, ByteSpanError, EvidenceIdentity, EvidenceLocation,
        ProducerIdentity,
    };

    #[test]
    fn byte_span_rejects_reversed_endpoints() {
        let start = ByteOffset::new(9);
        let end = ByteOffset::new(4);

        let error = ByteSpan::try_new(start, end)
            .expect_err("an exclusive end before the start must be rejected");

        assert_eq!(error, ByteSpanError::EndBeforeStart { start, end });
        assert_eq!(error.to_string(), "byte span end 4 precedes start 9");
    }

    #[test]
    fn byte_span_accepts_empty_and_maximum_width_ranges() {
        let empty = ByteSpan::try_new(ByteOffset::new(7), ByteOffset::new(7))
            .expect("equal endpoints identify a valid point");
        let maximum = ByteSpan::try_new(ByteOffset::ZERO, ByteOffset::new(u64::MAX))
            .expect("ordered fixed-width endpoints form a valid span");

        assert!(empty.is_empty());
        assert_eq!(empty.len(), ByteLength::ZERO);
        assert_eq!(maximum.start(), ByteOffset::ZERO);
        assert_eq!(maximum.end().get(), u64::MAX);
        assert_eq!(maximum.len(), ByteLength::new(u64::MAX));
        assert_eq!(maximum.len().get(), u64::MAX);
        assert!(!maximum.is_empty());
    }

    #[test]
    fn evidence_identity_preserves_exact_source_components() {
        let span = ByteSpan::try_new(ByteOffset::new(10), ByteOffset::new(18))
            .expect("ordered endpoints form a valid span");
        let identity = EvidenceIdentity::new(
            "repository:1",
            "worktree:abc",
            "src/lib.rs",
            "digest:123",
            EvidenceLocation::<&str>::ByteSpan(span),
        );

        assert_eq!(*identity.repository(), "repository:1");
        assert_eq!(*identity.snapshot(), "worktree:abc");
        assert_eq!(*identity.path(), "src/lib.rs");
        assert_eq!(*identity.content_digest(), "digest:123");
        assert_eq!(identity.location(), &EvidenceLocation::ByteSpan(span));
    }

    #[test]
    fn evidence_locations_keep_whole_file_and_symbol_occurrence_explicit() {
        let whole_file = EvidenceLocation::<&str>::WholeFile;
        let occurrence = EvidenceLocation::SymbolOccurrence("occurrence:7");

        assert_eq!(whole_file, EvidenceLocation::WholeFile);
        assert_eq!(
            occurrence,
            EvidenceLocation::SymbolOccurrence("occurrence:7")
        );
    }

    #[test]
    fn producer_identity_preserves_id_and_version_separately() {
        let producer = ProducerIdentity::new("rust-syntax", "1.0.0");

        assert_eq!(*producer.id(), "rust-syntax");
        assert_eq!(*producer.version(), "1.0.0");
    }
}
