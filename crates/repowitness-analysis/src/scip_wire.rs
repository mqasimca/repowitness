//! Bounded framing for the outer SCIP Protobuf message.
//!
//! This module deliberately validates only the outer wire framing and yields
//! one raw document payload at a time. The sibling overlay adapter turns those
//! bounded payloads into provider-neutral domain facts.

use std::{
    error::Error,
    fmt,
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

use repowitness_domain::{
    ByteOffset, ByteSpan, RepositoryPath, RepositoryPathLimits, SourceContentDigest,
    SourceFileKind, SourceManifest, SourceManifestEntry,
};
use sha2::{Digest, Sha256};

const MAX_INPUT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_DOCUMENTS: u32 = 100_000;
const MAX_DOCUMENT_BYTES: u32 = 1024 * 1024;
const MAX_METADATA_BYTES: u32 = 64 * 1024;
const MAX_IGNORED_FIELD_BYTES: u32 = 1024 * 1024;
const MAX_DOCUMENT_OCCURRENCES: u32 = 250_000;
const MAX_DOCUMENT_SYMBOLS: u32 = 50_000;
const MAX_SYMBOL_BYTES: u32 = 16 * 1024;
const MAX_SYMBOL_RELATIONSHIPS: u32 = 10_000;

const WIRE_VARINT: u8 = 0;
const WIRE_FIXED64: u8 = 1;
const WIRE_LENGTH_DELIMITED: u8 = 2;
const WIRE_FIXED32: u8 = 5;
const METADATA_FIELD: u32 = 1;
const DOCUMENT_FIELD: u32 = 2;
const METADATA_PROTOCOL_VERSION_FIELD: u32 = 1;
const METADATA_TEXT_ENCODING_FIELD: u32 = 4;
const SUPPORTED_PROTOCOL_VERSION: u32 = 0;

/// Source-text units declared by an admitted SCIP metadata message.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ScipWireTextEncoding {
    /// Source ranges count UTF-8 code units from the start of each line.
    Utf8,
    /// Source ranges count UTF-16 code units from the start of each line.
    Utf16,
}

/// Source-range units declared by an admitted SCIP document.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ScipWirePositionEncoding {
    /// The range character is a UTF-8 byte offset from the line start.
    Utf8,
    /// The range character is a UTF-16 code-unit offset from the line start.
    Utf16,
    /// The range character is a UTF-32 code-point offset from the line start.
    Utf32,
}

/// One zero-based SCIP source position, before conversion to a byte offset.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ScipWireSourcePosition {
    line: u32,
    character: u32,
}

impl ScipWireSourcePosition {
    /// Creates a source position from validated non-negative wire values.
    pub(crate) const fn new(line: u32, character: u32) -> Self {
        Self { line, character }
    }
}

/// One validated half-open SCIP range before conversion to source bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ScipWireSourceRange {
    start: ScipWireSourcePosition,
    end: ScipWireSourcePosition,
}

/// Bounded semantic occurrence data retained from one raw SCIP occurrence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ScipWireOccurrence<'a> {
    range: ScipWireSourceRange,
    symbol: Option<&'a str>,
    roles: u32,
}

/// One occurrence whose producer range has been validated against exact bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ScipWireValidatedOccurrence<'a> {
    ordinal: u32,
    span: ByteSpan,
    symbol: Option<&'a str>,
    roles: u32,
}

/// One opaque, explicitly typed relationship from a SCIP symbol.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ScipWireRelationship<'a> {
    symbol: &'a str,
    is_reference: bool,
    is_implementation: bool,
    is_type_definition: bool,
    is_definition: bool,
}

impl<'a> ScipWireRelationship<'a> {
    /// Returns the bounded opaque related symbol.
    pub(crate) const fn symbol(self) -> &'a str {
        self.symbol
    }

    /// Whether the edge expands a references query.
    pub(crate) const fn is_reference(self) -> bool {
        self.is_reference
    }

    /// Whether the edge expands an implementations query.
    pub(crate) const fn is_implementation(self) -> bool {
        self.is_implementation
    }

    /// Whether the edge expands a type-definition query.
    pub(crate) const fn is_type_definition(self) -> bool {
        self.is_type_definition
    }

    /// Whether the edge overrides the related symbol's definition role.
    pub(crate) const fn is_definition(self) -> bool {
        self.is_definition
    }
}

/// Bounded symbol information emitted with its source symbol and relationships.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ScipWireSymbolInformation<'a> {
    symbol: &'a str,
    relationships: u32,
}

impl<'a> ScipWireSymbolInformation<'a> {
    /// Returns the bounded opaque source symbol.
    pub(crate) const fn symbol(self) -> &'a str {
        self.symbol
    }

    /// Returns the number of validated relationships yielded for this symbol.
    pub(crate) const fn relationships(self) -> u32 {
        self.relationships
    }
}

impl<'a> ScipWireValidatedOccurrence<'a> {
    /// Returns the source-order ordinal within the containing document.
    pub(crate) const fn ordinal(self) -> u32 {
        self.ordinal
    }

    /// Returns the exact half-open source byte span.
    pub(crate) const fn span(self) -> ByteSpan {
        self.span
    }

    /// Returns the optional opaque producer symbol.
    pub(crate) const fn symbol(self) -> Option<&'a str> {
        self.symbol
    }

    /// Returns the producer role bitset for the evidence occurrence.
    pub(crate) const fn roles(self) -> u32 {
        self.roles
    }
}

/// Validated document identity and completion counters from one bounded decode.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ScipWireDecodedDocument {
    path: RepositoryPath,
    position_encoding: ScipWirePositionEncoding,
    occurrences: u32,
}

impl ScipWireDecodedDocument {
    /// Returns the canonical path admitted through the pinned manifest.
    pub(crate) const fn path(&self) -> &RepositoryPath {
        &self.path
    }

    /// Returns the encoding used to derive occurrence byte spans.
    pub(crate) const fn position_encoding(&self) -> ScipWirePositionEncoding {
        self.position_encoding
    }

    /// Returns the number of fully validated occurrence ranges yielded.
    pub(crate) const fn occurrences(&self) -> u32 {
        self.occurrences
    }
}

impl<'a> ScipWireOccurrence<'a> {
    /// Returns the producer range before exact-source byte validation.
    pub(crate) const fn range(self) -> ScipWireSourceRange {
        self.range
    }

    /// Returns the optional opaque producer symbol without interpreting it.
    pub(crate) const fn symbol(self) -> Option<&'a str> {
        self.symbol
    }

    /// Returns the producer role bitset without assigning unsupported meaning.
    pub(crate) const fn roles(self) -> u32 {
        self.roles
    }
}

impl ScipWireSourceRange {
    /// Creates a range from producer positions; byte validation checks ordering.
    pub(crate) const fn new(start: ScipWireSourcePosition, end: ScipWireSourcePosition) -> Self {
        Self { start, end }
    }
}

/// Independent hard bounds for one outer SCIP wire message.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ScipWireLimits {
    max_input_bytes: u64,
    max_documents: u32,
    max_document_bytes: u32,
    max_metadata_bytes: u32,
    max_ignored_field_bytes: u32,
}

impl ScipWireLimits {
    /// Conservative bounded defaults for the decoder spike.
    pub(crate) const DEFAULT: Self = Self {
        max_input_bytes: MAX_INPUT_BYTES,
        max_documents: MAX_DOCUMENTS,
        max_document_bytes: MAX_DOCUMENT_BYTES,
        max_metadata_bytes: MAX_METADATA_BYTES,
        max_ignored_field_bytes: MAX_IGNORED_FIELD_BYTES,
    };

    /// Creates independently positive limits bounded by compiled ceilings.
    #[cfg(test)]
    pub(crate) const fn try_new(
        max_input_bytes: u64,
        max_documents: u32,
        max_document_bytes: u32,
        max_metadata_bytes: u32,
        max_ignored_field_bytes: u32,
    ) -> Result<Self, ScipWireError> {
        let limits = Self {
            max_input_bytes,
            max_documents,
            max_document_bytes,
            max_metadata_bytes,
            max_ignored_field_bytes,
        };
        if limits.is_valid() {
            Ok(limits)
        } else {
            Err(ScipWireError::InvalidLimits)
        }
    }

    #[cfg(test)]
    const fn is_valid(self) -> bool {
        self.max_input_bytes != 0
            && self.max_input_bytes <= MAX_INPUT_BYTES
            && self.max_documents != 0
            && self.max_documents <= MAX_DOCUMENTS
            && self.max_document_bytes != 0
            && self.max_document_bytes <= MAX_DOCUMENT_BYTES
            && self.max_metadata_bytes != 0
            && self.max_metadata_bytes <= MAX_METADATA_BYTES
            && self.max_ignored_field_bytes != 0
            && self.max_ignored_field_bytes <= MAX_IGNORED_FIELD_BYTES
    }
}

impl Default for ScipWireLimits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Cooperative cancellation and a monotonic deadline for one wire scan.
#[derive(Clone, Copy)]
pub(crate) struct ScipWireControl<'a> {
    cancelled: &'a AtomicBool,
    deadline: Instant,
}

impl<'a> ScipWireControl<'a> {
    /// Constructs control state from one cancellation flag and deadline.
    pub(crate) const fn new(cancelled: &'a AtomicBool, deadline: Instant) -> Self {
        Self {
            cancelled,
            deadline,
        }
    }

    pub(crate) fn check(self) -> Result<(), ScipWireError> {
        if self.cancelled.load(Ordering::Acquire) {
            Err(ScipWireError::Cancelled)
        } else if Instant::now() >= self.deadline {
            Err(ScipWireError::DeadlineExceeded)
        } else {
            Ok(())
        }
    }
}

impl fmt::Debug for ScipWireControl<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ScipWireControl")
            .field("cancelled", &self.cancelled.load(Ordering::Acquire))
            .field("deadline", &"<monotonic>")
            .finish()
    }
}

/// One borrowed, independently length-bounded raw SCIP document payload.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ScipWireDocument<'a> {
    ordinal: u32,
    bytes: &'a [u8],
}

impl<'a> ScipWireDocument<'a> {
    /// Returns the source-order document ordinal.
    pub(crate) const fn ordinal(self) -> u32 {
        self.ordinal
    }

    /// Returns the exact raw document message bytes.
    pub(crate) const fn bytes(self) -> &'a [u8] {
        self.bytes
    }
}

/// Non-semantic framing counters for one successfully scanned outer message.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ScipWireSummary {
    metadata_present: bool,
    documents: u32,
    ignored_fields: u32,
}

/// Validated non-semantic header data from one raw SCIP document message.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ScipWireDocumentHeader<'a> {
    relative_path: &'a str,
    language: Option<&'a str>,
    position_encoding: Option<ScipWirePositionEncoding>,
    occurrences: u32,
    symbols: u32,
}

/// Validated schema-admission data from one SCIP metadata message.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ScipWireMetadata {
    protocol_version: u32,
    text_encoding: ScipWireTextEncoding,
}

impl ScipWireMetadata {
    /// Returns the accepted fixed SCIP protocol version.
    pub(crate) const fn protocol_version(self) -> u32 {
        self.protocol_version
    }

    /// Returns the source-text encoding declared for the index.
    pub(crate) const fn text_encoding(self) -> ScipWireTextEncoding {
        self.text_encoding
    }
}

impl<'a> ScipWireDocumentHeader<'a> {
    /// Returns the decoded producer-relative path; source containment is a later stage.
    pub(crate) const fn relative_path(self) -> &'a str {
        self.relative_path
    }

    /// Returns the optional producer-declared and recognized range encoding.
    pub(crate) const fn position_encoding(self) -> Option<ScipWirePositionEncoding> {
        self.position_encoding
    }

    /// Returns the bounded number of raw occurrence messages.
    pub(crate) const fn occurrences(self) -> u32 {
        self.occurrences
    }

    /// Returns the bounded number of raw symbol-information messages.
    pub(crate) const fn symbols(self) -> u32 {
        self.symbols
    }
}

impl ScipWireSummary {
    /// Whether exactly one metadata field was present.
    pub(crate) const fn metadata_present(self) -> bool {
        self.metadata_present
    }

    /// Returns the number of yielded document payloads.
    pub(crate) const fn documents(self) -> u32 {
        self.documents
    }

    /// Returns the number of skipped non-document fields.
    pub(crate) const fn ignored_fields(self) -> u32 {
        self.ignored_fields
    }
}

/// Categorical failure for hostile outer SCIP wire framing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ScipWireError {
    /// A test attempted to construct invalid limits.
    #[cfg(test)]
    InvalidLimits,
    /// Input exceeded its independently configured byte limit.
    InputTooLarge,
    /// The message ended before one declared value completed.
    Truncated,
    /// A varint exceeded its fixed Protobuf width.
    VarintOverflow,
    /// A field number was zero or a group/unknown wire form was encountered.
    UnsupportedWireType,
    /// A field payload exceeded its configured size limit.
    FieldTooLarge,
    /// More document fields appeared than the configured limit permits.
    TooManyDocuments,
    /// The singular outer metadata field appeared more than once.
    DuplicateMetadata,
    /// Metadata was absent or did not start the outer message.
    MetadataNotFirst,
    /// A required metadata field was missing or repeated.
    InvalidMetadata,
    /// Metadata declared a protocol version outside this importer's support.
    UnsupportedProtocolVersion,
    /// Metadata declared a source-text encoding outside this importer's support.
    UnsupportedTextEncoding,
    /// A singular document field appeared more than once or was absent.
    InvalidDocumentHeader,
    /// A text field was not valid UTF-8.
    InvalidUtf8,
    /// A document declared a range encoding outside this importer's support.
    UnsupportedPositionEncoding,
    /// One document exceeded its occurrence or symbol count bound.
    TooManyDocumentEntries,
    /// A document path did not identify a regular file in the pinned manifest.
    SourceNotInManifest,
    /// The supplied immutable source bytes did not match the pinned manifest digest.
    SourceDigestMismatch,
    /// Exact source bytes could not be interpreted as UTF-8 for range validation.
    SourceNotUtf8,
    /// A range was reversed, out of bounds, or ended between source code units.
    InvalidSourceRange,
    /// An occurrence did not contain one valid, internally consistent source range.
    InvalidOccurrenceRange,
    /// An occurrence contained malformed or duplicate semantic fields.
    InvalidOccurrence,
    /// Symbol information lacked one valid source symbol or had invalid fields.
    InvalidSymbolInformation,
    /// A relationship lacked a target or semantic relationship kind.
    InvalidRelationship,
    /// A symbol exceeded its independent relationship-count bound.
    TooManyRelationships,
    /// The caller cancelled before a next field was processed.
    Cancelled,
    /// The monotonic deadline elapsed before a next field was processed.
    DeadlineExceeded,
    /// A bounded caller-owned staging sink rejected one decoded item.
    ConsumerRejected,
}

impl fmt::Display for ScipWireError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            #[cfg(test)]
            Self::InvalidLimits => "invalid SCIP wire limits",
            Self::InputTooLarge => "SCIP input exceeded its byte limit",
            Self::Truncated => "truncated SCIP wire input",
            Self::VarintOverflow => "invalid SCIP wire varint",
            Self::UnsupportedWireType => "unsupported SCIP wire type",
            Self::FieldTooLarge => "SCIP field exceeded its byte limit",
            Self::TooManyDocuments => "SCIP input exceeded its document limit",
            Self::DuplicateMetadata => "SCIP input repeated metadata",
            Self::MetadataNotFirst => "SCIP metadata was absent or not first",
            Self::InvalidMetadata => "invalid SCIP metadata",
            Self::UnsupportedProtocolVersion => "unsupported SCIP protocol version",
            Self::UnsupportedTextEncoding => "unsupported SCIP text encoding",
            Self::InvalidDocumentHeader => "invalid SCIP document header",
            Self::InvalidUtf8 => "invalid SCIP UTF-8 field",
            Self::UnsupportedPositionEncoding => "unsupported SCIP position encoding",
            Self::TooManyDocumentEntries => "SCIP document exceeded its entry limit",
            Self::SourceNotInManifest => "SCIP document source is absent from the pinned manifest",
            Self::SourceDigestMismatch => {
                "SCIP document source bytes do not match the pinned manifest"
            }
            Self::SourceNotUtf8 => "SCIP source bytes are not valid UTF-8",
            Self::InvalidSourceRange => "invalid SCIP source range",
            Self::InvalidOccurrenceRange => "invalid SCIP occurrence range",
            Self::InvalidOccurrence => "invalid SCIP occurrence",
            Self::InvalidSymbolInformation => "invalid SCIP symbol information",
            Self::InvalidRelationship => "invalid SCIP relationship",
            Self::TooManyRelationships => "SCIP symbol exceeded its relationship limit",
            Self::Cancelled => "SCIP wire scan cancelled",
            Self::DeadlineExceeded => "SCIP wire scan deadline exceeded",
            Self::ConsumerRejected => "SCIP wire consumer rejected a decoded item",
        })
    }
}

/// Parses the bounded non-semantic header of one raw SCIP document message.
pub(crate) fn parse_scip_document_header<'input>(
    input: &'input [u8],
    control: ScipWireControl<'_>,
) -> Result<ScipWireDocumentHeader<'input>, ScipWireError> {
    control.check()?;
    let mut cursor = 0_usize;
    let mut relative_path = None;
    let mut language = None;
    let mut position_encoding = None;
    let mut occurrences = 0_u32;
    let mut symbols = 0_u32;

    while cursor < input.len() {
        control.check()?;
        let tag = read_varint(input, &mut cursor)?;
        let field = u32::try_from(tag >> 3).map_err(|_| ScipWireError::UnsupportedWireType)?;
        let wire = u8::try_from(tag & 0x07).map_err(|_| ScipWireError::UnsupportedWireType)?;
        if field == 0 {
            return Err(ScipWireError::UnsupportedWireType);
        }
        match (field, wire) {
            (1 | 4, WIRE_LENGTH_DELIMITED) => {
                let value = read_length_delimited(input, &mut cursor, MAX_METADATA_BYTES)?;
                let value = std::str::from_utf8(value).map_err(|_| ScipWireError::InvalidUtf8)?;
                let target = if field == 1 {
                    &mut relative_path
                } else {
                    &mut language
                };
                if target.replace(value).is_some() {
                    return Err(ScipWireError::InvalidDocumentHeader);
                }
            }
            (2 | 3, WIRE_LENGTH_DELIMITED) => {
                let _ = read_length_delimited(input, &mut cursor, MAX_DOCUMENT_BYTES)?;
                let count = if field == 2 {
                    &mut occurrences
                } else {
                    &mut symbols
                };
                let maximum = if field == 2 {
                    MAX_DOCUMENT_OCCURRENCES
                } else {
                    MAX_DOCUMENT_SYMBOLS
                };
                *count = count
                    .checked_add(1)
                    .ok_or(ScipWireError::TooManyDocumentEntries)?;
                if *count > maximum {
                    return Err(ScipWireError::TooManyDocumentEntries);
                }
            }
            (6, WIRE_VARINT) => {
                let value = u32::try_from(read_varint(input, &mut cursor)?)
                    .map_err(|_| ScipWireError::InvalidDocumentHeader)?;
                let value = decode_position_encoding(value)?;
                if position_encoding.replace(value).is_some() {
                    return Err(ScipWireError::InvalidDocumentHeader);
                }
            }
            (1..=6, _) => return Err(ScipWireError::InvalidDocumentHeader),
            (_, WIRE_VARINT) => {
                let _ = read_varint(input, &mut cursor)?;
            }
            (_, WIRE_FIXED64) => skip_exact(input, &mut cursor, 8)?,
            (_, WIRE_LENGTH_DELIMITED) => {
                let _ = read_length_delimited(input, &mut cursor, MAX_IGNORED_FIELD_BYTES)?;
            }
            (_, WIRE_FIXED32) => skip_exact(input, &mut cursor, 4)?,
            _ => return Err(ScipWireError::UnsupportedWireType),
        }
    }

    relative_path
        .map(|relative_path| ScipWireDocumentHeader {
            relative_path,
            language,
            position_encoding,
            occurrences,
            symbols,
        })
        .ok_or(ScipWireError::InvalidDocumentHeader)
}

/// Parses only the metadata values that establish schema and source-text units.
///
/// Producer provenance and the declared project root are intentionally left to
/// the later importer policy; neither becomes a filesystem authority here.
pub(crate) fn parse_scip_metadata(
    input: &[u8],
    control: ScipWireControl<'_>,
) -> Result<ScipWireMetadata, ScipWireError> {
    control.check()?;
    let mut cursor = 0_usize;
    let mut protocol_version = None;
    let mut text_encoding = None;

    while cursor < input.len() {
        control.check()?;
        let tag = read_varint(input, &mut cursor)?;
        let field = u32::try_from(tag >> 3).map_err(|_| ScipWireError::UnsupportedWireType)?;
        let wire = u8::try_from(tag & 0x07).map_err(|_| ScipWireError::UnsupportedWireType)?;
        if field == 0 {
            return Err(ScipWireError::UnsupportedWireType);
        }
        match (field, wire) {
            (METADATA_PROTOCOL_VERSION_FIELD, WIRE_VARINT) => {
                let value = u32::try_from(read_varint(input, &mut cursor)?)
                    .map_err(|_| ScipWireError::UnsupportedProtocolVersion)?;
                if value != SUPPORTED_PROTOCOL_VERSION {
                    return Err(ScipWireError::UnsupportedProtocolVersion);
                }
                if protocol_version.replace(value).is_some() {
                    return Err(ScipWireError::InvalidMetadata);
                }
            }
            (METADATA_TEXT_ENCODING_FIELD, WIRE_VARINT) => {
                let value = u32::try_from(read_varint(input, &mut cursor)?)
                    .map_err(|_| ScipWireError::UnsupportedTextEncoding)?;
                let value = decode_text_encoding(value)?;
                if text_encoding.replace(value).is_some() {
                    return Err(ScipWireError::InvalidMetadata);
                }
            }
            (METADATA_PROTOCOL_VERSION_FIELD | METADATA_TEXT_ENCODING_FIELD, _) => {
                return Err(ScipWireError::InvalidMetadata);
            }
            (_, WIRE_VARINT) => {
                let _ = read_varint(input, &mut cursor)?;
            }
            (_, WIRE_FIXED64) => skip_exact(input, &mut cursor, 8)?,
            (_, WIRE_LENGTH_DELIMITED) => {
                let _ = read_length_delimited(input, &mut cursor, MAX_METADATA_BYTES)?;
            }
            (_, WIRE_FIXED32) => skip_exact(input, &mut cursor, 4)?,
            _ => return Err(ScipWireError::UnsupportedWireType),
        }
    }

    Ok(ScipWireMetadata {
        protocol_version: protocol_version.ok_or(ScipWireError::InvalidMetadata)?,
        text_encoding: text_encoding.ok_or(ScipWireError::InvalidMetadata)?,
    })
}

fn decode_text_encoding(value: u32) -> Result<ScipWireTextEncoding, ScipWireError> {
    match value {
        1 => Ok(ScipWireTextEncoding::Utf8),
        2 => Ok(ScipWireTextEncoding::Utf16),
        _ => Err(ScipWireError::UnsupportedTextEncoding),
    }
}

fn decode_position_encoding(value: u32) -> Result<ScipWirePositionEncoding, ScipWireError> {
    match value {
        1 => Ok(ScipWirePositionEncoding::Utf8),
        2 => Ok(ScipWirePositionEncoding::Utf16),
        3 => Ok(ScipWirePositionEncoding::Utf32),
        _ => Err(ScipWireError::UnsupportedPositionEncoding),
    }
}

/// Parses the source range of one raw SCIP occurrence without retaining its
/// symbol, documentation, diagnostics, or other untrusted payloads.
pub(crate) fn parse_scip_occurrence_range(
    input: &[u8],
    control: ScipWireControl<'_>,
) -> Result<ScipWireSourceRange, ScipWireError> {
    control.check()?;
    let mut cursor = 0_usize;
    let mut legacy = LegacyRange::default();
    let mut typed = None;

    while cursor < input.len() {
        control.check()?;
        let tag = read_varint(input, &mut cursor)?;
        let field = u32::try_from(tag >> 3).map_err(|_| ScipWireError::UnsupportedWireType)?;
        let wire = u8::try_from(tag & 0x07).map_err(|_| ScipWireError::UnsupportedWireType)?;
        if field == 0 {
            return Err(ScipWireError::UnsupportedWireType);
        }
        match (field, wire) {
            (1, WIRE_VARINT) => {
                legacy.push(decode_nonnegative_i32(read_varint(input, &mut cursor)?)?)?
            }
            (1, WIRE_LENGTH_DELIMITED) => {
                let packed = read_length_delimited(input, &mut cursor, MAX_DOCUMENT_BYTES)?;
                let mut packed_cursor = 0_usize;
                while packed_cursor < packed.len() {
                    control.check()?;
                    legacy.push(decode_nonnegative_i32(read_varint(
                        packed,
                        &mut packed_cursor,
                    )?)?)?;
                }
            }
            (8, WIRE_LENGTH_DELIMITED) => {
                let raw = read_length_delimited(input, &mut cursor, MAX_DOCUMENT_BYTES)?;
                let range = parse_single_line_range(raw, control)?;
                if typed.replace(range).is_some() {
                    return Err(ScipWireError::InvalidOccurrenceRange);
                }
            }
            (9, WIRE_LENGTH_DELIMITED) => {
                let raw = read_length_delimited(input, &mut cursor, MAX_DOCUMENT_BYTES)?;
                let range = parse_multi_line_range(raw, control)?;
                if typed.replace(range).is_some() {
                    return Err(ScipWireError::InvalidOccurrenceRange);
                }
            }
            (_, WIRE_VARINT) => {
                let _ = read_varint(input, &mut cursor)?;
            }
            (_, WIRE_FIXED64) => skip_exact(input, &mut cursor, 8)?,
            (_, WIRE_LENGTH_DELIMITED) => {
                let _ = read_length_delimited(input, &mut cursor, MAX_DOCUMENT_BYTES)?;
            }
            (_, WIRE_FIXED32) => skip_exact(input, &mut cursor, 4)?,
            _ => return Err(ScipWireError::UnsupportedWireType),
        }
    }

    let legacy = legacy.finish()?;
    match (typed, legacy) {
        (Some(typed), Some(legacy)) if typed != legacy => {
            Err(ScipWireError::InvalidOccurrenceRange)
        }
        (Some(typed), _) => Ok(typed),
        (None, Some(legacy)) => Ok(legacy),
        (None, None) => Err(ScipWireError::InvalidOccurrenceRange),
    }
}

/// Parses the bounded semantic fields from one raw SCIP occurrence.
pub(crate) fn parse_scip_occurrence<'input>(
    input: &'input [u8],
    control: ScipWireControl<'_>,
) -> Result<ScipWireOccurrence<'input>, ScipWireError> {
    let range = parse_scip_occurrence_range(input, control)?;
    let mut cursor = 0_usize;
    let mut symbol = None;
    let mut roles = None;

    while cursor < input.len() {
        control.check()?;
        let tag = read_varint(input, &mut cursor)?;
        let field = u32::try_from(tag >> 3).map_err(|_| ScipWireError::UnsupportedWireType)?;
        let wire = u8::try_from(tag & 0x07).map_err(|_| ScipWireError::UnsupportedWireType)?;
        if field == 0 {
            return Err(ScipWireError::UnsupportedWireType);
        }
        match (field, wire) {
            (2, WIRE_LENGTH_DELIMITED) => {
                let value = read_length_delimited(input, &mut cursor, MAX_SYMBOL_BYTES)?;
                let value = std::str::from_utf8(value).map_err(|_| ScipWireError::InvalidUtf8)?;
                if value.is_empty() || symbol.replace(value).is_some() {
                    return Err(ScipWireError::InvalidOccurrence);
                }
            }
            (3, WIRE_VARINT) => {
                let value = decode_nonnegative_i32(read_varint(input, &mut cursor)?)?;
                if roles.replace(value).is_some() {
                    return Err(ScipWireError::InvalidOccurrence);
                }
            }
            (2 | 3, _) => return Err(ScipWireError::InvalidOccurrence),
            (_, WIRE_VARINT) => {
                let _ = read_varint(input, &mut cursor)?;
            }
            (_, WIRE_FIXED64) => skip_exact(input, &mut cursor, 8)?,
            (_, WIRE_LENGTH_DELIMITED) => {
                let _ = read_length_delimited(input, &mut cursor, MAX_DOCUMENT_BYTES)?;
            }
            (_, WIRE_FIXED32) => skip_exact(input, &mut cursor, 4)?,
            _ => return Err(ScipWireError::UnsupportedWireType),
        }
    }

    Ok(ScipWireOccurrence {
        range,
        symbol,
        roles: roles.unwrap_or(0),
    })
}

/// Parses one symbol-information message and yields its typed relationships.
pub(crate) fn parse_scip_symbol_information<'input>(
    input: &'input [u8],
    control: ScipWireControl<'_>,
    mut on_relationship: impl FnMut(ScipWireRelationship<'input>) -> Result<(), ScipWireError>,
) -> Result<ScipWireSymbolInformation<'input>, ScipWireError> {
    control.check()?;
    let mut cursor = 0_usize;
    let mut symbol = None;
    let mut relationships = 0_u32;

    while cursor < input.len() {
        control.check()?;
        let tag = read_varint(input, &mut cursor)?;
        let field = u32::try_from(tag >> 3).map_err(|_| ScipWireError::UnsupportedWireType)?;
        let wire = u8::try_from(tag & 0x07).map_err(|_| ScipWireError::UnsupportedWireType)?;
        if field == 0 {
            return Err(ScipWireError::UnsupportedWireType);
        }
        match (field, wire) {
            (1, WIRE_LENGTH_DELIMITED) => {
                let value = read_length_delimited(input, &mut cursor, MAX_SYMBOL_BYTES)?;
                let value = std::str::from_utf8(value).map_err(|_| ScipWireError::InvalidUtf8)?;
                if value.is_empty() || symbol.replace(value).is_some() {
                    return Err(ScipWireError::InvalidSymbolInformation);
                }
            }
            (4, WIRE_LENGTH_DELIMITED) => {
                let raw = read_length_delimited(input, &mut cursor, MAX_DOCUMENT_BYTES)?;
                let relationship = parse_scip_relationship(raw, control)?;
                relationships = relationships
                    .checked_add(1)
                    .ok_or(ScipWireError::TooManyRelationships)?;
                if relationships > MAX_SYMBOL_RELATIONSHIPS {
                    return Err(ScipWireError::TooManyRelationships);
                }
                on_relationship(relationship)?;
            }
            (1 | 4, _) => return Err(ScipWireError::InvalidSymbolInformation),
            (_, WIRE_VARINT) => {
                let _ = read_varint(input, &mut cursor)?;
            }
            (_, WIRE_FIXED64) => skip_exact(input, &mut cursor, 8)?,
            (_, WIRE_LENGTH_DELIMITED) => {
                let _ = read_length_delimited(input, &mut cursor, MAX_DOCUMENT_BYTES)?;
            }
            (_, WIRE_FIXED32) => skip_exact(input, &mut cursor, 4)?,
            _ => return Err(ScipWireError::UnsupportedWireType),
        }
    }

    Ok(ScipWireSymbolInformation {
        symbol: symbol.ok_or(ScipWireError::InvalidSymbolInformation)?,
        relationships,
    })
}

fn parse_scip_relationship<'input>(
    input: &'input [u8],
    control: ScipWireControl<'_>,
) -> Result<ScipWireRelationship<'input>, ScipWireError> {
    control.check()?;
    let mut cursor = 0_usize;
    let mut symbol = None;
    let mut is_reference = None;
    let mut is_implementation = None;
    let mut is_type_definition = None;
    let mut is_definition = None;

    while cursor < input.len() {
        control.check()?;
        let tag = read_varint(input, &mut cursor)?;
        let field = u32::try_from(tag >> 3).map_err(|_| ScipWireError::UnsupportedWireType)?;
        let wire = u8::try_from(tag & 0x07).map_err(|_| ScipWireError::UnsupportedWireType)?;
        if field == 0 {
            return Err(ScipWireError::UnsupportedWireType);
        }
        match (field, wire) {
            (1, WIRE_LENGTH_DELIMITED) => {
                let value = read_length_delimited(input, &mut cursor, MAX_SYMBOL_BYTES)?;
                let value = std::str::from_utf8(value).map_err(|_| ScipWireError::InvalidUtf8)?;
                if value.is_empty() || symbol.replace(value).is_some() {
                    return Err(ScipWireError::InvalidRelationship);
                }
            }
            (2..=5, WIRE_VARINT) => {
                let value = decode_bool(read_varint(input, &mut cursor)?)?;
                let target = match field {
                    2 => &mut is_reference,
                    3 => &mut is_implementation,
                    4 => &mut is_type_definition,
                    5 => &mut is_definition,
                    _ => return Err(ScipWireError::InvalidRelationship),
                };
                if target.replace(value).is_some() {
                    return Err(ScipWireError::InvalidRelationship);
                }
            }
            (1..=5, _) => return Err(ScipWireError::InvalidRelationship),
            (_, WIRE_VARINT) => {
                let _ = read_varint(input, &mut cursor)?;
            }
            (_, WIRE_FIXED64) => skip_exact(input, &mut cursor, 8)?,
            (_, WIRE_LENGTH_DELIMITED) => {
                let _ = read_length_delimited(input, &mut cursor, MAX_DOCUMENT_BYTES)?;
            }
            (_, WIRE_FIXED32) => skip_exact(input, &mut cursor, 4)?,
            _ => return Err(ScipWireError::UnsupportedWireType),
        }
    }

    let relationship = ScipWireRelationship {
        symbol: symbol.ok_or(ScipWireError::InvalidRelationship)?,
        is_reference: is_reference.unwrap_or(false),
        is_implementation: is_implementation.unwrap_or(false),
        is_type_definition: is_type_definition.unwrap_or(false),
        is_definition: is_definition.unwrap_or(false),
    };
    if !relationship.is_reference
        && !relationship.is_implementation
        && !relationship.is_type_definition
        && !relationship.is_definition
    {
        return Err(ScipWireError::InvalidRelationship);
    }
    Ok(relationship)
}

fn decode_bool(value: u64) -> Result<bool, ScipWireError> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(ScipWireError::InvalidRelationship),
    }
}

#[derive(Default)]
struct LegacyRange {
    values: [u32; 4],
    count: u8,
}

impl LegacyRange {
    fn push(&mut self, value: u32) -> Result<(), ScipWireError> {
        let index = usize::from(self.count);
        let slot = self
            .values
            .get_mut(index)
            .ok_or(ScipWireError::InvalidOccurrenceRange)?;
        *slot = value;
        self.count = self
            .count
            .checked_add(1)
            .ok_or(ScipWireError::InvalidOccurrenceRange)?;
        Ok(())
    }

    fn finish(self) -> Result<Option<ScipWireSourceRange>, ScipWireError> {
        match self.count {
            0 => Ok(None),
            3 => Ok(Some(ScipWireSourceRange::new(
                ScipWireSourcePosition::new(self.values[0], self.values[1]),
                ScipWireSourcePosition::new(self.values[0], self.values[2]),
            ))),
            4 => Ok(Some(ScipWireSourceRange::new(
                ScipWireSourcePosition::new(self.values[0], self.values[1]),
                ScipWireSourcePosition::new(self.values[2], self.values[3]),
            ))),
            _ => Err(ScipWireError::InvalidOccurrenceRange),
        }
    }
}

fn parse_single_line_range(
    input: &[u8],
    control: ScipWireControl<'_>,
) -> Result<ScipWireSourceRange, ScipWireError> {
    let Some([line, start, end]) = parse_range_components::<3>(input, control, 3)? else {
        return Err(ScipWireError::InvalidOccurrenceRange);
    };
    Ok(ScipWireSourceRange::new(
        ScipWireSourcePosition::new(line, start),
        ScipWireSourcePosition::new(line, end),
    ))
}

fn parse_multi_line_range(
    input: &[u8],
    control: ScipWireControl<'_>,
) -> Result<ScipWireSourceRange, ScipWireError> {
    let Some([start_line, start_character, end_line, end_character]) =
        parse_range_components::<4>(input, control, 4)?
    else {
        return Err(ScipWireError::InvalidOccurrenceRange);
    };
    Ok(ScipWireSourceRange::new(
        ScipWireSourcePosition::new(start_line, start_character),
        ScipWireSourcePosition::new(end_line, end_character),
    ))
}

fn parse_range_components<const COMPONENTS: usize>(
    input: &[u8],
    control: ScipWireControl<'_>,
    expected: u32,
) -> Result<Option<[u32; COMPONENTS]>, ScipWireError> {
    let mut cursor = 0_usize;
    let mut values = [0_u32; COMPONENTS];
    let mut seen = [false; COMPONENTS];

    while cursor < input.len() {
        control.check()?;
        let tag = read_varint(input, &mut cursor)?;
        let field = u32::try_from(tag >> 3).map_err(|_| ScipWireError::UnsupportedWireType)?;
        let wire = u8::try_from(tag & 0x07).map_err(|_| ScipWireError::UnsupportedWireType)?;
        if field == 0 {
            return Err(ScipWireError::UnsupportedWireType);
        }
        if field <= expected && wire == WIRE_VARINT {
            let index =
                usize::try_from(field - 1).map_err(|_| ScipWireError::InvalidOccurrenceRange)?;
            if seen[index] {
                return Err(ScipWireError::InvalidOccurrenceRange);
            }
            values[index] = decode_nonnegative_i32(read_varint(input, &mut cursor)?)?;
            seen[index] = true;
            continue;
        }
        match wire {
            WIRE_VARINT => {
                let _ = read_varint(input, &mut cursor)?;
            }
            WIRE_FIXED64 => skip_exact(input, &mut cursor, 8)?,
            WIRE_LENGTH_DELIMITED => {
                let _ = read_length_delimited(input, &mut cursor, MAX_DOCUMENT_BYTES)?;
            }
            WIRE_FIXED32 => skip_exact(input, &mut cursor, 4)?,
            _ => return Err(ScipWireError::UnsupportedWireType),
        }
    }

    Ok(seen
        .into_iter()
        .all(core::convert::identity)
        .then_some(values))
}

fn decode_nonnegative_i32(value: u64) -> Result<u32, ScipWireError> {
    let value = i32::try_from(value).map_err(|_| ScipWireError::InvalidOccurrenceRange)?;
    u32::try_from(value).map_err(|_| ScipWireError::InvalidOccurrenceRange)
}

/// Converts a decoded SCIP path through the canonical repository-path grammar.
///
/// This validates only path shape and limits. A later importer must still prove
/// membership and exact content against its pinned immutable source manifest.
pub(crate) fn validate_scip_document_path(
    header: ScipWireDocumentHeader<'_>,
    limits: RepositoryPathLimits,
) -> Result<RepositoryPath, ScipWireError> {
    RepositoryPath::try_from_bytes(header.relative_path().as_bytes(), limits)
        .map_err(|_| ScipWireError::InvalidDocumentHeader)
}

/// Admits one SCIP document only when it names and exactly matches a regular
/// file in the caller's immutable source manifest.
///
/// `source_bytes` are caller-supplied immutable snapshot bytes. This pure
/// analysis boundary performs no filesystem access and retains neither the
/// bytes nor untrusted document payloads after returning.
pub(crate) fn validate_scip_document_source<'input, 'manifest>(
    header: ScipWireDocumentHeader<'input>,
    manifest: &'manifest SourceManifest<RepositoryPath, SourceFileKind, SourceContentDigest>,
    path_limits: RepositoryPathLimits,
    source_bytes: &[u8],
) -> Result<
    &'manifest SourceManifestEntry<RepositoryPath, SourceFileKind, SourceContentDigest>,
    ScipWireError,
> {
    let path = validate_scip_document_path(header, path_limits)?;
    let entries = manifest.as_slice();
    let entry = entries
        .binary_search_by(|candidate| candidate.path().cmp(&path))
        .map(|index| &entries[index])
        .map_err(|_| ScipWireError::SourceNotInManifest)?;
    if *entry.file_type() != SourceFileKind::Regular {
        return Err(ScipWireError::SourceNotInManifest);
    }

    let actual = SourceContentDigest::new(Sha256::digest(source_bytes).into());
    if *entry.content_digest() != actual {
        return Err(ScipWireError::SourceDigestMismatch);
    }

    Ok(entry)
}

/// Decodes one bounded SCIP document against caller-supplied immutable source
/// state and yields only occurrence facts with exact byte spans.
///
/// The callback must stage its output: a later malformed occurrence can still
/// fail this document after an earlier callback invocation.
pub(crate) fn decode_scip_document(
    input: &[u8],
    manifest: &SourceManifest<RepositoryPath, SourceFileKind, SourceContentDigest>,
    path_limits: RepositoryPathLimits,
    source_bytes: &[u8],
    control: ScipWireControl<'_>,
    mut on_occurrence: impl FnMut(ScipWireValidatedOccurrence<'_>) -> Result<(), ScipWireError>,
) -> Result<ScipWireDecodedDocument, ScipWireError> {
    let header = parse_scip_document_header(input, control)?;
    let position_encoding = header
        .position_encoding()
        .ok_or(ScipWireError::UnsupportedPositionEncoding)?;
    let path = validate_scip_document_path(header, path_limits)?;
    let _ = validate_scip_document_source(header, manifest, path_limits, source_bytes)?;

    let mut cursor = 0_usize;
    let mut occurrences = 0_u32;
    while cursor < input.len() {
        control.check()?;
        let tag = read_varint(input, &mut cursor)?;
        let field = u32::try_from(tag >> 3).map_err(|_| ScipWireError::UnsupportedWireType)?;
        let wire = u8::try_from(tag & 0x07).map_err(|_| ScipWireError::UnsupportedWireType)?;
        if field == 0 {
            return Err(ScipWireError::UnsupportedWireType);
        }
        match (field, wire) {
            (2, WIRE_LENGTH_DELIMITED) => {
                let raw = read_length_delimited(input, &mut cursor, MAX_DOCUMENT_BYTES)?;
                let occurrence = parse_scip_occurrence(raw, control)?;
                let span = validate_scip_source_range(
                    occurrence.range(),
                    position_encoding,
                    source_bytes,
                )?;
                let ordinal = occurrences;
                occurrences = occurrences
                    .checked_add(1)
                    .ok_or(ScipWireError::TooManyDocumentEntries)?;
                if occurrences > MAX_DOCUMENT_OCCURRENCES {
                    return Err(ScipWireError::TooManyDocumentEntries);
                }
                on_occurrence(ScipWireValidatedOccurrence {
                    ordinal,
                    span,
                    symbol: occurrence.symbol(),
                    roles: occurrence.roles(),
                })?;
            }
            (1 | 3 | 4 | 5, WIRE_LENGTH_DELIMITED) => {
                let _ = read_length_delimited(input, &mut cursor, MAX_DOCUMENT_BYTES)?;
            }
            (6, WIRE_VARINT) => {
                let _ = read_varint(input, &mut cursor)?;
            }
            (1..=6, _) => return Err(ScipWireError::InvalidDocumentHeader),
            (_, WIRE_VARINT) => {
                let _ = read_varint(input, &mut cursor)?;
            }
            (_, WIRE_FIXED64) => skip_exact(input, &mut cursor, 8)?,
            (_, WIRE_LENGTH_DELIMITED) => {
                let _ = read_length_delimited(input, &mut cursor, MAX_DOCUMENT_BYTES)?;
            }
            (_, WIRE_FIXED32) => skip_exact(input, &mut cursor, 4)?,
            _ => return Err(ScipWireError::UnsupportedWireType),
        }
    }

    Ok(ScipWireDecodedDocument {
        path,
        position_encoding,
        occurrences,
    })
}

/// Converts one producer range to an exact half-open byte span in immutable
/// source bytes, rejecting every out-of-bounds or code-unit-boundary claim.
pub(crate) fn validate_scip_source_range(
    range: ScipWireSourceRange,
    encoding: ScipWirePositionEncoding,
    source_bytes: &[u8],
) -> Result<ByteSpan, ScipWireError> {
    let source = std::str::from_utf8(source_bytes).map_err(|_| ScipWireError::SourceNotUtf8)?;
    let start = source_position_to_byte_offset(source, range.start, encoding)?;
    let end = source_position_to_byte_offset(source, range.end, encoding)?;
    if end < start {
        return Err(ScipWireError::InvalidSourceRange);
    }
    ByteSpan::try_new(ByteOffset::new(start), ByteOffset::new(end))
        .map_err(|_| ScipWireError::InvalidSourceRange)
}

fn source_position_to_byte_offset(
    source: &str,
    position: ScipWireSourcePosition,
    encoding: ScipWirePositionEncoding,
) -> Result<u64, ScipWireError> {
    let (line_start, line) = source_line(source, position.line)?;
    let relative = line_character_to_byte_offset(line, position.character, encoding)?;
    line_start
        .checked_add(relative)
        .ok_or(ScipWireError::InvalidSourceRange)
}

fn source_line(source: &str, requested_line: u32) -> Result<(u64, &str), ScipWireError> {
    let requested =
        usize::try_from(requested_line).map_err(|_| ScipWireError::InvalidSourceRange)?;
    if source.is_empty() {
        return if requested == 0 {
            Ok((0, source))
        } else {
            Err(ScipWireError::InvalidSourceRange)
        };
    }

    let mut start = 0_u64;
    let mut line_count = 0_usize;
    for (index, line) in source.split_inclusive('\n').enumerate() {
        line_count = index
            .checked_add(1)
            .ok_or(ScipWireError::InvalidSourceRange)?;
        if index == requested {
            let line = line
                .strip_suffix("\r\n")
                .or_else(|| line.strip_suffix('\n'))
                .unwrap_or(line);
            return Ok((start, line));
        }
        start = start
            .checked_add(u64::try_from(line.len()).map_err(|_| ScipWireError::InvalidSourceRange)?)
            .ok_or(ScipWireError::InvalidSourceRange)?;
    }
    if source.ends_with('\n') && requested == line_count {
        return Ok((start, ""));
    }
    Err(ScipWireError::InvalidSourceRange)
}

fn line_character_to_byte_offset(
    line: &str,
    character: u32,
    encoding: ScipWirePositionEncoding,
) -> Result<u64, ScipWireError> {
    let requested = usize::try_from(character).map_err(|_| ScipWireError::InvalidSourceRange)?;
    let byte_offset = match encoding {
        ScipWirePositionEncoding::Utf8 => {
            if requested > line.len() || !line.is_char_boundary(requested) {
                return Err(ScipWireError::InvalidSourceRange);
            }
            requested
        }
        ScipWirePositionEncoding::Utf16 => {
            unit_offset_to_byte_offset(line, requested, |character| character.len_utf16())?
        }
        ScipWirePositionEncoding::Utf32 => unit_offset_to_byte_offset(line, requested, |_| 1)?,
    };
    u64::try_from(byte_offset).map_err(|_| ScipWireError::InvalidSourceRange)
}

fn unit_offset_to_byte_offset(
    line: &str,
    requested: usize,
    unit_count: impl Fn(char) -> usize,
) -> Result<usize, ScipWireError> {
    let mut units = 0_usize;
    for (offset, character) in line.char_indices() {
        if units == requested {
            return Ok(offset);
        }
        units = units
            .checked_add(unit_count(character))
            .ok_or(ScipWireError::InvalidSourceRange)?;
        if units > requested {
            return Err(ScipWireError::InvalidSourceRange);
        }
    }
    if units == requested {
        Ok(line.len())
    } else {
        Err(ScipWireError::InvalidSourceRange)
    }
}

impl Error for ScipWireError {}

/// Scans a bounded outer SCIP message, yielding metadata first and each raw
/// document afterwards.
///
/// The callback must not publish externally visible state. Later framing errors
/// remain possible after a prior document has been yielded, so an importer must
/// retain any work in disposable staging until this scan and all document-level
/// validation complete successfully.
pub(crate) fn scan_scip_wire(
    input: &[u8],
    limits: ScipWireLimits,
    control: ScipWireControl<'_>,
    mut on_metadata: impl FnMut(&[u8]) -> Result<(), ScipWireError>,
    mut on_document: impl FnMut(ScipWireDocument<'_>) -> Result<(), ScipWireError>,
) -> Result<ScipWireSummary, ScipWireError> {
    let input_bytes = u64::try_from(input.len()).map_err(|_| ScipWireError::InputTooLarge)?;
    if input_bytes > limits.max_input_bytes {
        return Err(ScipWireError::InputTooLarge);
    }
    control.check()?;

    let mut cursor = 0_usize;
    let mut metadata_present = false;
    let mut documents = 0_u32;
    let mut ignored_fields = 0_u32;

    while cursor < input.len() {
        control.check()?;
        let tag = read_varint(input, &mut cursor)?;
        let field = u32::try_from(tag >> 3).map_err(|_| ScipWireError::UnsupportedWireType)?;
        let wire = u8::try_from(tag & 0x07).map_err(|_| ScipWireError::UnsupportedWireType)?;
        if field == 0 {
            return Err(ScipWireError::UnsupportedWireType);
        }

        match (field, wire) {
            (METADATA_FIELD, WIRE_LENGTH_DELIMITED) => {
                if metadata_present {
                    return Err(ScipWireError::DuplicateMetadata);
                }
                if cursor != 1 {
                    // Field one with a length-delimited wire type has the
                    // one-byte tag 0x0a, so any later metadata has a cursor
                    // beyond the first tag.
                    return Err(ScipWireError::MetadataNotFirst);
                }
                let bytes = read_length_delimited(input, &mut cursor, limits.max_metadata_bytes)?;
                on_metadata(bytes)?;
                metadata_present = true;
            }
            (DOCUMENT_FIELD, WIRE_LENGTH_DELIMITED) => {
                if !metadata_present {
                    return Err(ScipWireError::MetadataNotFirst);
                }
                if documents == limits.max_documents {
                    return Err(ScipWireError::TooManyDocuments);
                }
                let bytes = read_length_delimited(input, &mut cursor, limits.max_document_bytes)?;
                let ordinal = documents;
                documents = documents
                    .checked_add(1)
                    .ok_or(ScipWireError::TooManyDocuments)?;
                on_document(ScipWireDocument { ordinal, bytes })?;
            }
            (_, WIRE_VARINT) => {
                if !metadata_present {
                    return Err(ScipWireError::MetadataNotFirst);
                }
                let _ = read_varint(input, &mut cursor)?;
                ignored_fields = ignored_fields
                    .checked_add(1)
                    .ok_or(ScipWireError::FieldTooLarge)?;
            }
            (_, WIRE_FIXED64) => {
                if !metadata_present {
                    return Err(ScipWireError::MetadataNotFirst);
                }
                skip_exact(input, &mut cursor, 8)?;
                ignored_fields = ignored_fields
                    .checked_add(1)
                    .ok_or(ScipWireError::FieldTooLarge)?;
            }
            (_, WIRE_LENGTH_DELIMITED) => {
                if !metadata_present {
                    return Err(ScipWireError::MetadataNotFirst);
                }
                let _ = read_length_delimited(input, &mut cursor, limits.max_ignored_field_bytes)?;
                ignored_fields = ignored_fields
                    .checked_add(1)
                    .ok_or(ScipWireError::FieldTooLarge)?;
            }
            (_, WIRE_FIXED32) => {
                if !metadata_present {
                    return Err(ScipWireError::MetadataNotFirst);
                }
                skip_exact(input, &mut cursor, 4)?;
                ignored_fields = ignored_fields
                    .checked_add(1)
                    .ok_or(ScipWireError::FieldTooLarge)?;
            }
            _ => return Err(ScipWireError::UnsupportedWireType),
        }
    }

    if !metadata_present {
        return Err(ScipWireError::MetadataNotFirst);
    }

    Ok(ScipWireSummary {
        metadata_present,
        documents,
        ignored_fields,
    })
}

pub(crate) fn read_varint(input: &[u8], cursor: &mut usize) -> Result<u64, ScipWireError> {
    let mut value = 0_u64;
    for shift in (0_u32..64).step_by(7) {
        let byte = *input.get(*cursor).ok_or(ScipWireError::Truncated)?;
        *cursor = cursor.checked_add(1).ok_or(ScipWireError::Truncated)?;
        let payload = u64::from(byte & 0x7f);
        if shift == 63 && payload > 1 {
            return Err(ScipWireError::VarintOverflow);
        }
        value |= payload << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err(ScipWireError::VarintOverflow)
}

pub(crate) fn read_length_delimited<'a>(
    input: &'a [u8],
    cursor: &mut usize,
    maximum: u32,
) -> Result<&'a [u8], ScipWireError> {
    let length = read_varint(input, cursor)?;
    if length > u64::from(maximum) {
        return Err(ScipWireError::FieldTooLarge);
    }
    let length = usize::try_from(length).map_err(|_| ScipWireError::FieldTooLarge)?;
    let end = cursor.checked_add(length).ok_or(ScipWireError::Truncated)?;
    let value = input.get(*cursor..end).ok_or(ScipWireError::Truncated)?;
    *cursor = end;
    Ok(value)
}

fn skip_exact(input: &[u8], cursor: &mut usize, length: usize) -> Result<(), ScipWireError> {
    let end = cursor.checked_add(length).ok_or(ScipWireError::Truncated)?;
    if input.get(*cursor..end).is_none() {
        return Err(ScipWireError::Truncated);
    }
    *cursor = end;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{sync::atomic::AtomicBool, time::Duration};

    use repowitness_domain::SourceFileLimit;

    use super::*;

    fn control(cancelled: &AtomicBool) -> ScipWireControl<'_> {
        ScipWireControl::new(cancelled, Instant::now() + Duration::from_secs(1))
    }

    fn field(number: u32, wire: u8, value: &[u8]) -> Vec<u8> {
        let mut output = Vec::new();
        push_varint(u64::from((number << 3) | u32::from(wire)), &mut output);
        if wire == WIRE_LENGTH_DELIMITED {
            push_varint(u64::try_from(value.len()).expect("length"), &mut output);
        }
        output.extend_from_slice(value);
        output
    }

    fn push_varint(mut value: u64, output: &mut Vec<u8>) {
        while value >= 0x80 {
            output.push(u8::try_from(value & 0x7f).expect("byte") | 0x80);
            value >>= 7;
        }
        output.push(u8::try_from(value).expect("byte"));
    }

    #[test]
    fn streams_document_payloads_in_source_order() {
        let cancelled = AtomicBool::new(false);
        let mut input = field(METADATA_FIELD, WIRE_LENGTH_DELIMITED, b"metadata");
        input.extend(field(DOCUMENT_FIELD, WIRE_LENGTH_DELIMITED, b"first"));
        input.extend(field(9, WIRE_LENGTH_DELIMITED, b"ignored"));
        input.extend(field(DOCUMENT_FIELD, WIRE_LENGTH_DELIMITED, b"second"));
        let mut received = Vec::new();

        let summary = scan_scip_wire(
            &input,
            ScipWireLimits::default(),
            control(&cancelled),
            |_| Ok(()),
            |document| {
                received.push((document.ordinal(), document.bytes().to_vec()));
                Ok(())
            },
        )
        .expect("scan");

        assert_eq!(received, [(0, b"first".to_vec()), (1, b"second".to_vec())]);
        assert!(summary.metadata_present());
        assert_eq!(summary.documents(), 2);
        assert_eq!(summary.ignored_fields(), 1);
    }

    #[test]
    fn parses_bounded_document_header_without_retaining_entry_payloads() {
        let cancelled = AtomicBool::new(false);
        let mut document = field(1, WIRE_LENGTH_DELIMITED, b"src/lib.rs");
        document.extend(field(4, WIRE_LENGTH_DELIMITED, b"Rust"));
        document.extend(field(2, WIRE_LENGTH_DELIMITED, b"occurrence"));
        document.extend(field(2, WIRE_LENGTH_DELIMITED, b"occurrence"));
        document.extend(field(3, WIRE_LENGTH_DELIMITED, b"symbol"));
        document.extend(field(6, WIRE_VARINT, &[1]));

        let header = parse_scip_document_header(&document, control(&cancelled)).expect("header");

        assert_eq!(header.relative_path(), "src/lib.rs");
        assert_eq!(
            header.position_encoding(),
            Some(ScipWirePositionEncoding::Utf8)
        );
        assert_eq!(header.occurrences(), 2);
        assert_eq!(header.symbols(), 1);
    }

    #[test]
    fn admits_only_the_pinned_protocol_and_text_encodings() {
        let cancelled = AtomicBool::new(false);
        let mut metadata = field(METADATA_PROTOCOL_VERSION_FIELD, WIRE_VARINT, &[0]);
        metadata.extend(field(METADATA_TEXT_ENCODING_FIELD, WIRE_VARINT, &[1]));

        assert_eq!(
            parse_scip_metadata(&metadata, control(&cancelled)),
            Ok(ScipWireMetadata {
                protocol_version: SUPPORTED_PROTOCOL_VERSION,
                text_encoding: ScipWireTextEncoding::Utf8,
            })
        );

        let unsupported_protocol = field(METADATA_PROTOCOL_VERSION_FIELD, WIRE_VARINT, &[1]);
        assert_eq!(
            parse_scip_metadata(&unsupported_protocol, control(&cancelled)),
            Err(ScipWireError::UnsupportedProtocolVersion)
        );

        let mut unsupported_encoding = field(METADATA_PROTOCOL_VERSION_FIELD, WIRE_VARINT, &[0]);
        unsupported_encoding.extend(field(METADATA_TEXT_ENCODING_FIELD, WIRE_VARINT, &[3]));
        assert_eq!(
            parse_scip_metadata(&unsupported_encoding, control(&cancelled)),
            Err(ScipWireError::UnsupportedTextEncoding)
        );
    }

    #[test]
    fn rejects_invalid_document_headers() {
        let cancelled = AtomicBool::new(false);
        let mut duplicate = field(1, WIRE_LENGTH_DELIMITED, b"one.rs");
        duplicate.extend(field(1, WIRE_LENGTH_DELIMITED, b"two.rs"));
        assert_eq!(
            parse_scip_document_header(&duplicate, control(&cancelled)),
            Err(ScipWireError::InvalidDocumentHeader)
        );

        let invalid_utf8 = field(1, WIRE_LENGTH_DELIMITED, &[0xff]);
        assert_eq!(
            parse_scip_document_header(&invalid_utf8, control(&cancelled)),
            Err(ScipWireError::InvalidUtf8)
        );

        let mut unsupported_encoding = field(1, WIRE_LENGTH_DELIMITED, b"one.rs");
        unsupported_encoding.extend(field(6, WIRE_VARINT, &[4]));
        assert_eq!(
            parse_scip_document_header(&unsupported_encoding, control(&cancelled)),
            Err(ScipWireError::UnsupportedPositionEncoding)
        );
    }

    #[test]
    fn validates_document_paths_through_the_canonical_grammar() {
        let cancelled = AtomicBool::new(false);
        let valid_bytes = field(1, WIRE_LENGTH_DELIMITED, b"src/lib.rs");
        let valid = parse_scip_document_header(&valid_bytes, control(&cancelled)).expect("header");
        assert_eq!(
            validate_scip_document_path(valid, RepositoryPathLimits::new(128, 8))
                .expect("path")
                .as_bytes(),
            b"src/lib.rs"
        );

        let invalid_bytes = field(1, WIRE_LENGTH_DELIMITED, b"../outside.rs");
        let invalid =
            parse_scip_document_header(&invalid_bytes, control(&cancelled)).expect("header");
        assert_eq!(
            validate_scip_document_path(invalid, RepositoryPathLimits::new(128, 8)),
            Err(ScipWireError::InvalidDocumentHeader)
        );
    }

    fn manifest(
        path: &[u8],
        kind: SourceFileKind,
        source: &[u8],
    ) -> SourceManifest<RepositoryPath, SourceFileKind, SourceContentDigest> {
        SourceManifest::try_from_vec(
            vec![SourceManifestEntry::new(
                RepositoryPath::try_from_bytes(path, RepositoryPathLimits::new(128, 8))
                    .expect("fixture path"),
                kind,
                SourceContentDigest::new(Sha256::digest(source).into()),
            )],
            SourceFileLimit::new(1),
        )
        .expect("fixture manifest")
    }

    #[test]
    fn document_source_admission_requires_manifest_membership_regular_kind_and_exact_bytes() {
        let cancelled = AtomicBool::new(false);
        let document = field(1, WIRE_LENGTH_DELIMITED, b"src/lib.rs");
        let header = parse_scip_document_header(&document, control(&cancelled)).expect("header");
        let regular = manifest(b"src/lib.rs", SourceFileKind::Regular, b"exact bytes");

        let entry = validate_scip_document_source(
            header,
            &regular,
            RepositoryPathLimits::new(128, 8),
            b"exact bytes",
        )
        .expect("admitted document");
        assert_eq!(entry.path().as_bytes(), b"src/lib.rs");

        assert_eq!(
            validate_scip_document_source(
                header,
                &regular,
                RepositoryPathLimits::new(128, 8),
                b"changed bytes",
            ),
            Err(ScipWireError::SourceDigestMismatch)
        );

        let absent = manifest(b"src/other.rs", SourceFileKind::Regular, b"exact bytes");
        assert_eq!(
            validate_scip_document_source(
                header,
                &absent,
                RepositoryPathLimits::new(128, 8),
                b"exact bytes",
            ),
            Err(ScipWireError::SourceNotInManifest)
        );

        let link = manifest(b"src/lib.rs", SourceFileKind::SymbolicLink, b"exact bytes");
        assert_eq!(
            validate_scip_document_source(
                header,
                &link,
                RepositoryPathLimits::new(128, 8),
                b"exact bytes",
            ),
            Err(ScipWireError::SourceNotInManifest)
        );
    }

    #[test]
    fn source_ranges_map_all_supported_units_to_exact_byte_spans() {
        let source = "a🚀b\nxy\r\n";
        let start = ScipWireSourcePosition::new(0, 1);

        for (encoding, end_character) in [
            (ScipWirePositionEncoding::Utf8, 5),
            (ScipWirePositionEncoding::Utf16, 3),
            (ScipWirePositionEncoding::Utf32, 2),
        ] {
            let span = validate_scip_source_range(
                ScipWireSourceRange::new(start, ScipWireSourcePosition::new(0, end_character)),
                encoding,
                source.as_bytes(),
            )
            .expect("rocket range");
            assert_eq!(span.start().get(), 1);
            assert_eq!(span.end().get(), 5);
        }

        let crlf_line = validate_scip_source_range(
            ScipWireSourceRange::new(
                ScipWireSourcePosition::new(1, 0),
                ScipWireSourcePosition::new(1, 2),
            ),
            ScipWirePositionEncoding::Utf8,
            source.as_bytes(),
        )
        .expect("second line range");
        assert_eq!(crlf_line.start().get(), 7);
        assert_eq!(crlf_line.end().get(), 9);
    }

    #[test]
    fn source_ranges_reject_non_boundaries_reversed_positions_and_non_utf8_bytes() {
        let source = "a🚀b\n";
        assert_eq!(
            validate_scip_source_range(
                ScipWireSourceRange::new(
                    ScipWireSourcePosition::new(0, 2),
                    ScipWireSourcePosition::new(0, 5),
                ),
                ScipWirePositionEncoding::Utf8,
                source.as_bytes(),
            ),
            Err(ScipWireError::InvalidSourceRange)
        );
        assert_eq!(
            validate_scip_source_range(
                ScipWireSourceRange::new(
                    ScipWireSourcePosition::new(0, 2),
                    ScipWireSourcePosition::new(0, 3),
                ),
                ScipWirePositionEncoding::Utf16,
                source.as_bytes(),
            ),
            Err(ScipWireError::InvalidSourceRange)
        );
        assert_eq!(
            validate_scip_source_range(
                ScipWireSourceRange::new(
                    ScipWireSourcePosition::new(1, 0),
                    ScipWireSourcePosition::new(0, 0),
                ),
                ScipWirePositionEncoding::Utf32,
                source.as_bytes(),
            ),
            Err(ScipWireError::InvalidSourceRange)
        );
        assert_eq!(
            validate_scip_source_range(
                ScipWireSourceRange::new(
                    ScipWireSourcePosition::new(0, 0),
                    ScipWireSourcePosition::new(0, 0),
                ),
                ScipWirePositionEncoding::Utf8,
                &[0xff],
            ),
            Err(ScipWireError::SourceNotUtf8)
        );
    }

    #[test]
    fn occurrence_ranges_accept_legacy_and_typed_forms_but_reject_disagreement() {
        let cancelled = AtomicBool::new(false);
        let mut packed = Vec::new();
        for value in [2_u64, 3, 7] {
            push_varint(value, &mut packed);
        }
        let legacy = field(1, WIRE_LENGTH_DELIMITED, &packed);
        let expected = ScipWireSourceRange::new(
            ScipWireSourcePosition::new(2, 3),
            ScipWireSourcePosition::new(2, 7),
        );
        assert_eq!(
            parse_scip_occurrence_range(&legacy, control(&cancelled)),
            Ok(expected)
        );

        let mut typed_body = field(1, WIRE_VARINT, &[2]);
        typed_body.extend(field(2, WIRE_VARINT, &[3]));
        typed_body.extend(field(3, WIRE_VARINT, &[7]));
        let typed = field(8, WIRE_LENGTH_DELIMITED, &typed_body);
        assert_eq!(
            parse_scip_occurrence_range(&typed, control(&cancelled)),
            Ok(expected)
        );

        let mut agreeing = legacy.clone();
        agreeing.extend(&typed);
        assert_eq!(
            parse_scip_occurrence_range(&agreeing, control(&cancelled)),
            Ok(expected)
        );

        let mut conflicting_body = field(1, WIRE_VARINT, &[2]);
        conflicting_body.extend(field(2, WIRE_VARINT, &[3]));
        conflicting_body.extend(field(3, WIRE_VARINT, &[8]));
        let mut conflicting = legacy;
        conflicting.extend(field(8, WIRE_LENGTH_DELIMITED, &conflicting_body));
        assert_eq!(
            parse_scip_occurrence_range(&conflicting, control(&cancelled)),
            Err(ScipWireError::InvalidOccurrenceRange)
        );
    }

    #[test]
    fn occurrence_ranges_reject_missing_incomplete_and_negative_forms() {
        let cancelled = AtomicBool::new(false);
        assert_eq!(
            parse_scip_occurrence_range(&[], control(&cancelled)),
            Err(ScipWireError::InvalidOccurrenceRange)
        );

        let mut incomplete = Vec::new();
        for value in [0_u64, 1] {
            push_varint(value, &mut incomplete);
        }
        let incomplete = field(1, WIRE_LENGTH_DELIMITED, &incomplete);
        assert_eq!(
            parse_scip_occurrence_range(&incomplete, control(&cancelled)),
            Err(ScipWireError::InvalidOccurrenceRange)
        );

        let mut negative = Vec::new();
        push_varint(u64::MAX, &mut negative);
        let negative = field(1, WIRE_LENGTH_DELIMITED, &negative);
        assert_eq!(
            parse_scip_occurrence_range(&negative, control(&cancelled)),
            Err(ScipWireError::InvalidOccurrenceRange)
        );
    }

    #[test]
    fn occurrences_retain_only_one_bounded_opaque_symbol() {
        let cancelled = AtomicBool::new(false);
        let mut packed = Vec::new();
        for value in [0_u64, 1, 2] {
            push_varint(value, &mut packed);
        }
        let mut occurrence = field(1, WIRE_LENGTH_DELIMITED, &packed);
        occurrence.extend(field(2, WIRE_LENGTH_DELIMITED, b"scip-java pkg 1 A."));
        occurrence.extend(field(3, WIRE_VARINT, &[1]));

        let parsed = parse_scip_occurrence(&occurrence, control(&cancelled)).expect("occurrence");
        assert_eq!(parsed.symbol(), Some("scip-java pkg 1 A."));
        assert_eq!(parsed.roles(), 1);
        assert_eq!(
            parsed.range(),
            ScipWireSourceRange::new(
                ScipWireSourcePosition::new(0, 1),
                ScipWireSourcePosition::new(0, 2),
            )
        );

        let mut duplicate = occurrence.clone();
        duplicate.extend(field(2, WIRE_LENGTH_DELIMITED, b"other"));
        assert_eq!(
            parse_scip_occurrence(&duplicate, control(&cancelled)),
            Err(ScipWireError::InvalidOccurrence)
        );

        let mut empty = occurrence;
        empty.clear();
        empty.extend(field(1, WIRE_LENGTH_DELIMITED, &packed));
        empty.extend(field(2, WIRE_LENGTH_DELIMITED, b""));
        assert_eq!(
            parse_scip_occurrence(&empty, control(&cancelled)),
            Err(ScipWireError::InvalidOccurrence)
        );
    }

    #[test]
    fn document_decode_requires_pinned_source_and_yields_exact_occurrence_evidence() {
        let cancelled = AtomicBool::new(false);
        let source = b"abc\n";
        let mut packed = Vec::new();
        for value in [0_u64, 1, 2] {
            push_varint(value, &mut packed);
        }
        let mut occurrence = field(1, WIRE_LENGTH_DELIMITED, &packed);
        occurrence.extend(field(2, WIRE_LENGTH_DELIMITED, b"scip-rust pkg 1 answer."));
        let mut document = field(1, WIRE_LENGTH_DELIMITED, b"src/lib.rs");
        document.extend(field(2, WIRE_LENGTH_DELIMITED, &occurrence));
        document.extend(field(6, WIRE_VARINT, &[1]));
        let manifest = manifest(b"src/lib.rs", SourceFileKind::Regular, source);
        let mut yielded = Vec::new();

        let decoded = decode_scip_document(
            &document,
            &manifest,
            RepositoryPathLimits::new(128, 8),
            source,
            control(&cancelled),
            |occurrence| {
                yielded.push((
                    occurrence.ordinal(),
                    occurrence.span().start().get(),
                    occurrence.span().end().get(),
                    occurrence.symbol().map(str::to_owned),
                ));
                Ok(())
            },
        )
        .expect("document decode");

        assert_eq!(decoded.path().as_bytes(), b"src/lib.rs");
        assert_eq!(decoded.position_encoding(), ScipWirePositionEncoding::Utf8);
        assert_eq!(decoded.occurrences(), 1);
        assert_eq!(
            yielded,
            [(0, 1, 2, Some("scip-rust pkg 1 answer.".to_owned()))]
        );

        assert_eq!(
            decode_scip_document(
                &document,
                &manifest,
                RepositoryPathLimits::new(128, 8),
                b"changed\n",
                control(&cancelled),
                |_| Ok(())
            ),
            Err(ScipWireError::SourceDigestMismatch)
        );
    }

    #[test]
    fn symbol_information_yields_only_bounded_typed_relationships() {
        let cancelled = AtomicBool::new(false);
        let mut relationship = field(1, WIRE_LENGTH_DELIMITED, b"scip-rust pkg 1 Target.");
        relationship.extend(field(2, WIRE_VARINT, &[1]));
        relationship.extend(field(3, WIRE_VARINT, &[1]));
        let mut information = field(1, WIRE_LENGTH_DELIMITED, b"scip-rust pkg 1 Source.");
        information.extend(field(4, WIRE_LENGTH_DELIMITED, &relationship));
        let mut relationships = Vec::new();

        let parsed = parse_scip_symbol_information(&information, control(&cancelled), |item| {
            relationships.push((
                item.symbol().to_owned(),
                item.is_reference(),
                item.is_implementation(),
                item.is_type_definition(),
                item.is_definition(),
            ));
            Ok(())
        })
        .expect("symbol information");
        assert_eq!(parsed.symbol(), "scip-rust pkg 1 Source.");
        assert_eq!(parsed.relationships(), 1);
        assert_eq!(
            relationships,
            [(
                "scip-rust pkg 1 Target.".to_owned(),
                true,
                true,
                false,
                false
            )]
        );

        let target_only = field(1, WIRE_LENGTH_DELIMITED, b"scip-rust pkg 1 Target.");
        let mut invalid = field(1, WIRE_LENGTH_DELIMITED, b"scip-rust pkg 1 Source.");
        invalid.extend(field(4, WIRE_LENGTH_DELIMITED, &target_only));
        assert_eq!(
            parse_scip_symbol_information(&invalid, control(&cancelled), |_| Ok(())),
            Err(ScipWireError::InvalidRelationship)
        );
    }

    #[test]
    fn rejects_oversized_input_before_yielding() {
        let cancelled = AtomicBool::new(false);
        let input = field(DOCUMENT_FIELD, WIRE_LENGTH_DELIMITED, b"document");
        let limits = ScipWireLimits::try_new(1, 1, 64, 64, 64).expect("limits");
        let mut yielded = false;

        let error = scan_scip_wire(
            &input,
            limits,
            control(&cancelled),
            |_| Ok(()),
            |_| {
                yielded = true;
                Ok(())
            },
        )
        .expect_err("oversized input");

        assert_eq!(error, ScipWireError::InputTooLarge);
        assert!(!yielded);
    }

    #[test]
    fn rejects_oversized_document_before_yielding_it() {
        let cancelled = AtomicBool::new(false);
        let mut input = field(METADATA_FIELD, WIRE_LENGTH_DELIMITED, b"metadata");
        input.extend(field(DOCUMENT_FIELD, WIRE_LENGTH_DELIMITED, b"document"));
        let limits = ScipWireLimits::try_new(64, 1, 1, 64, 64).expect("limits");
        let mut yielded = false;

        let error = scan_scip_wire(
            &input,
            limits,
            control(&cancelled),
            |_| Ok(()),
            |_| {
                yielded = true;
                Ok(())
            },
        )
        .expect_err("oversized document");

        assert_eq!(error, ScipWireError::FieldTooLarge);
        assert!(!yielded);
    }

    #[test]
    fn rejects_duplicate_metadata_and_invalid_wire_forms() {
        let cancelled = AtomicBool::new(false);
        let mut duplicate = field(METADATA_FIELD, WIRE_LENGTH_DELIMITED, b"one");
        duplicate.extend(field(METADATA_FIELD, WIRE_LENGTH_DELIMITED, b"two"));
        assert_eq!(
            scan_scip_wire(
                &duplicate,
                ScipWireLimits::default(),
                control(&cancelled),
                |_| Ok(()),
                |_| Ok(())
            ),
            Err(ScipWireError::DuplicateMetadata)
        );

        let document_first = field(DOCUMENT_FIELD, WIRE_LENGTH_DELIMITED, b"document");
        assert_eq!(
            scan_scip_wire(
                &document_first,
                ScipWireLimits::default(),
                control(&cancelled),
                |_| Ok(()),
                |_| Ok(())
            ),
            Err(ScipWireError::MetadataNotFirst)
        );

        let group = vec![u8::try_from((9 << 3) | 3).expect("tag")];
        assert_eq!(
            scan_scip_wire(
                &group,
                ScipWireLimits::default(),
                control(&cancelled),
                |_| Ok(()),
                |_| Ok(())
            ),
            Err(ScipWireError::UnsupportedWireType)
        );
    }

    #[test]
    fn rejects_truncation_and_cancellation() {
        let cancelled = AtomicBool::new(false);
        let mut truncated = field(METADATA_FIELD, WIRE_LENGTH_DELIMITED, b"metadata");
        truncated.extend([
            u8::try_from((DOCUMENT_FIELD << 3) | u32::from(WIRE_LENGTH_DELIMITED)).expect("tag"),
            2,
            1,
        ]);
        assert_eq!(
            scan_scip_wire(
                &truncated,
                ScipWireLimits::default(),
                control(&cancelled),
                |_| Ok(()),
                |_| Ok(())
            ),
            Err(ScipWireError::Truncated)
        );

        cancelled.store(true, Ordering::Release);
        assert_eq!(
            scan_scip_wire(
                &[],
                ScipWireLimits::default(),
                control(&cancelled),
                |_| Ok(()),
                |_| Ok(())
            ),
            Err(ScipWireError::Cancelled)
        );
    }
}
