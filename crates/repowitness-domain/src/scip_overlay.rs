//! Provider-neutral validated SCIP precision-overlay facts.
//!
//! These types deliberately contain no Protobuf, filesystem, SQLite, or
//! producer-process dependency. Adapters validate hostile wire data before
//! constructing them.

use core::fmt;

use crate::{ByteSpan, RepositoryPath, SourceContentDigest};

/// The first persisted/wire-compatible SCIP overlay fact contract.
pub const SCIP_OVERLAY_SCHEMA_VERSION: u16 = 1;
/// Inclusive byte ceiling for one opaque SCIP symbol identifier.
pub const MAX_SCIP_SYMBOL_BYTES: u64 = 16 * 1024;

/// A bounded opaque producer-local SCIP symbol identifier.
#[derive(Clone, Eq, Ord, PartialEq, PartialOrd)]
pub struct ScipSymbol(Box<str>);

impl ScipSymbol {
    /// Validates and stores one non-empty UTF-8 producer symbol.
    pub fn try_new(value: String) -> Result<Self, ScipSymbolError> {
        let bytes = u64::try_from(value.len()).map_err(|_| ScipSymbolError::TooLong)?;
        if value.is_empty() {
            return Err(ScipSymbolError::Empty);
        }
        if bytes > MAX_SCIP_SYMBOL_BYTES {
            return Err(ScipSymbolError::TooLong);
        }
        Ok(Self(value.into_boxed_str()))
    }

    /// Returns the opaque identifier for exact internal comparison only.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the identifier into its owned bounded text representation.
    #[must_use]
    pub fn into_string(self) -> String {
        self.0.into()
    }
}

impl fmt::Debug for ScipSymbol {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ScipSymbol")
            .field("bytes", &self.0.len())
            .field("value", &"<redacted-scip-symbol>")
            .finish()
    }
}

/// Failure to construct a bounded opaque SCIP symbol.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScipSymbolError {
    /// The producer symbol was empty.
    Empty,
    /// The producer symbol exceeded the fixed contract ceiling.
    TooLong,
}

impl fmt::Display for ScipSymbolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Empty => "SCIP symbol is empty",
            Self::TooLong => "SCIP symbol exceeds its byte limit",
        })
    }
}

impl std::error::Error for ScipSymbolError {}

/// Opaque SCIP occurrence-role bits with explicit supported role accessors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScipSymbolRoles(u32);

impl ScipSymbolRoles {
    /// No producer roles were declared.
    pub const NONE: Self = Self(0);
    /// The SCIP definition role bit.
    pub const DEFINITION: Self = Self(0x1);
    /// The SCIP import role bit.
    pub const IMPORT: Self = Self(0x2);
    /// The SCIP write-access role bit.
    pub const WRITE_ACCESS: Self = Self(0x4);
    /// The SCIP read-access role bit.
    pub const READ_ACCESS: Self = Self(0x8);

    /// Preserves all validated non-negative producer bits.
    #[must_use]
    pub const fn new(bits: u32) -> Self {
        Self(bits)
    }

    /// Returns all preserved producer bits for canonical persistence.
    #[must_use]
    pub const fn bits(self) -> u32 {
        self.0
    }

    /// Whether the producer explicitly marked this occurrence as a definition.
    #[must_use]
    pub const fn is_definition(self) -> bool {
        self.0 & Self::DEFINITION.0 != 0
    }

    /// Whether the producer explicitly marked this occurrence as an import.
    #[must_use]
    pub const fn is_import(self) -> bool {
        self.0 & Self::IMPORT.0 != 0
    }
}

/// Explicit relationship flags from one source SCIP symbol to one target.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScipRelationshipKinds {
    reference: bool,
    implementation: bool,
    type_definition: bool,
    definition: bool,
}

impl ScipRelationshipKinds {
    /// Creates a non-empty explicit set of relationship flags.
    pub const fn try_new(
        reference: bool,
        implementation: bool,
        type_definition: bool,
        definition: bool,
    ) -> Result<Self, ScipRelationshipError> {
        if !reference && !implementation && !type_definition && !definition {
            return Err(ScipRelationshipError::NoKind);
        }
        Ok(Self {
            reference,
            implementation,
            type_definition,
            definition,
        })
    }

    /// Whether the relationship contributes to references queries.
    #[must_use]
    pub const fn is_reference(self) -> bool {
        self.reference
    }

    /// Whether the relationship contributes to implementations queries.
    #[must_use]
    pub const fn is_implementation(self) -> bool {
        self.implementation
    }

    /// Whether the relationship contributes to type-definition queries.
    #[must_use]
    pub const fn is_type_definition(self) -> bool {
        self.type_definition
    }

    /// Whether the relationship contributes to definition queries.
    #[must_use]
    pub const fn is_definition(self) -> bool {
        self.definition
    }
}

/// A validated package-aware relationship between opaque provider symbols.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScipRelationship {
    source: ScipSymbol,
    target: ScipSymbol,
    kinds: ScipRelationshipKinds,
}

impl ScipRelationship {
    /// Creates one attributed relationship from already validated components.
    #[must_use]
    pub const fn new(source: ScipSymbol, target: ScipSymbol, kinds: ScipRelationshipKinds) -> Self {
        Self {
            source,
            target,
            kinds,
        }
    }

    /// Returns the source opaque producer symbol.
    #[must_use]
    pub const fn source(&self) -> &ScipSymbol {
        &self.source
    }

    /// Returns the target opaque producer symbol.
    #[must_use]
    pub const fn target(&self) -> &ScipSymbol {
        &self.target
    }

    /// Returns the explicit relationship flags.
    #[must_use]
    pub const fn kinds(&self) -> ScipRelationshipKinds {
        self.kinds
    }
}

/// Failure to construct a meaningful SCIP relationship.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScipRelationshipError {
    /// No explicit relationship flag was set.
    NoKind,
}

impl fmt::Display for ScipRelationshipError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SCIP relationship has no kind")
    }
}

impl std::error::Error for ScipRelationshipError {}

/// One source-validated precision occurrence in an immutable overlay.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScipOccurrence {
    path: RepositoryPath,
    content: SourceContentDigest,
    span: ByteSpan,
    ordinal: u32,
    symbol: Option<ScipSymbol>,
    roles: ScipSymbolRoles,
}

impl ScipOccurrence {
    /// Creates an occurrence from validated source and producer evidence.
    #[must_use]
    pub const fn new(
        path: RepositoryPath,
        content: SourceContentDigest,
        span: ByteSpan,
        ordinal: u32,
        symbol: Option<ScipSymbol>,
        roles: ScipSymbolRoles,
    ) -> Self {
        Self {
            path,
            content,
            span,
            ordinal,
            symbol,
            roles,
        }
    }

    /// Returns the canonical source path.
    #[must_use]
    pub const fn path(&self) -> &RepositoryPath {
        &self.path
    }

    /// Returns the exact source-content digest.
    #[must_use]
    pub const fn content(&self) -> SourceContentDigest {
        self.content
    }

    /// Returns the exact source byte span.
    #[must_use]
    pub const fn span(&self) -> ByteSpan {
        self.span
    }

    /// Returns the stable document-local ordinal.
    #[must_use]
    pub const fn ordinal(&self) -> u32 {
        self.ordinal
    }

    /// Returns the optional opaque producer symbol.
    #[must_use]
    pub const fn symbol(&self) -> Option<&ScipSymbol> {
        self.symbol.as_ref()
    }

    /// Returns the declared producer role bits.
    #[must_use]
    pub const fn roles(&self) -> ScipSymbolRoles {
        self.roles
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_SCIP_SYMBOL_BYTES, ScipRelationshipError, ScipRelationshipKinds, ScipSymbol,
        ScipSymbolError, ScipSymbolRoles,
    };

    #[test]
    fn symbols_are_bounded_opaque_and_redacted() {
        let symbol = ScipSymbol::try_new("scip-rust pkg 1 item.".to_owned()).expect("symbol");
        assert_eq!(symbol.as_str(), "scip-rust pkg 1 item.");
        assert!(!format!("{symbol:?}").contains(symbol.as_str()));
        assert_eq!(
            ScipSymbol::try_new(String::new()),
            Err(ScipSymbolError::Empty)
        );
        let oversized = "x".repeat(usize::try_from(MAX_SCIP_SYMBOL_BYTES + 1).expect("size"));
        assert_eq!(
            ScipSymbol::try_new(oversized),
            Err(ScipSymbolError::TooLong)
        );
    }

    #[test]
    fn roles_and_relationship_kinds_preserve_explicit_meaning() {
        let roles = ScipSymbolRoles::new(ScipSymbolRoles::DEFINITION.bits() | 0x80);
        assert!(roles.is_definition());
        assert!(!roles.is_import());
        assert_eq!(roles.bits(), 0x81);
        assert_eq!(
            ScipRelationshipKinds::try_new(false, false, false, false),
            Err(ScipRelationshipError::NoKind)
        );
        assert!(
            ScipRelationshipKinds::try_new(true, false, false, false)
                .expect("relationship")
                .is_reference()
        );
    }
}
