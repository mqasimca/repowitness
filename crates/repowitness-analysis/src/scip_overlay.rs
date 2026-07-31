//! Provider-neutral bounded SCIP overlay document decoding.

use std::{error::Error, fmt, sync::atomic::AtomicBool, time::Instant};

use repowitness_domain::{
    RepositoryPath, RepositoryPathLimits, ScipOccurrence, ScipRelationship, ScipRelationshipKinds,
    ScipSymbol, ScipSymbolRoles, SourceContentDigest, SourceFileKind, SourceManifest,
};

use crate::scip_wire::{
    ScipWireControl, ScipWireError, ScipWireLimits, ScipWireTextEncoding, decode_scip_document,
    parse_scip_document_header, parse_scip_metadata, parse_scip_symbol_information,
    read_length_delimited, read_varint, scan_scip_wire, validate_scip_document_path,
    validate_scip_document_source,
};

const WIRE_LENGTH_DELIMITED: u8 = 2;
const DOCUMENT_SYMBOLS_FIELD: u32 = 3;
const MAX_DOCUMENT_BYTES: u32 = 1024 * 1024;
const MAX_DOCUMENT_RELATIONSHIPS: usize = 250_000;
const MAX_DOCUMENT_OWNED_SYMBOL_BYTES: u64 = 16 * 1024 * 1024;

/// Fixed importer implementation contract used for immutable overlay identity.
pub const SCIP_OVERLAY_IMPORTER_VERSION: u16 = 1;
/// Exact upstream SCIP schema revision reviewed by this importer.
pub const SCIP_SCHEMA_REVISION: &str = "44d39fcfc95486d066a796e2cec8c7ec5d429aae";
/// SHA-256 of `scip.proto` at [`SCIP_SCHEMA_REVISION`].
pub const SCIP_SCHEMA_SHA256: [u8; 32] = [
    0xb3, 0x80, 0x21, 0xb6, 0x5e, 0xf9, 0x0c, 0xbb, 0xf6, 0xaf, 0x9c, 0x82, 0x9f, 0xf7, 0x51, 0x92,
    0x85, 0x9a, 0xd9, 0xb5, 0xda, 0x05, 0x43, 0x9e, 0xf1, 0x54, 0xbe, 0xa4, 0xce, 0xb2, 0xbf, 0x03,
];

/// A bounded decoded SCIP document expressed as provider-neutral domain facts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScipOverlayDocument {
    path: RepositoryPath,
    content: SourceContentDigest,
    occurrences: Box<[ScipOccurrence]>,
    relationships: Box<[ScipRelationship]>,
}

/// Immutable source bytes from the exact view against which an overlay imports.
///
/// Implementations must not read the live filesystem. The application adapter
/// supplies already-pinned source bytes from its immutable source view.
pub trait ScipImmutableSourceLookup {
    /// Returns exact source bytes for one canonical repository-relative path.
    fn source_bytes(&self, path: &RepositoryPath) -> Option<&[u8]>;
}

/// The source-text encoding declared by the admitted SCIP index metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScipSourceTextEncoding {
    /// Source files are represented as UTF-8 bytes on disk.
    Utf8,
    /// Source files are represented as UTF-16 code units on disk.
    Utf16,
}

/// Counters for a fully framed, staged SCIP overlay import.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScipOverlayIndexSummary {
    documents: u32,
    occurrences: u64,
    relationships: u64,
    source_text_encoding: ScipSourceTextEncoding,
}

impl ScipOverlayIndexSummary {
    /// Returns the number of source-order document batches staged by the sink.
    #[must_use]
    pub const fn documents(self) -> u32 {
        self.documents
    }

    /// Returns the sum of bounded occurrence facts in all staged batches.
    #[must_use]
    pub const fn occurrences(self) -> u64 {
        self.occurrences
    }

    /// Returns the sum of bounded relationship facts in all staged batches.
    #[must_use]
    pub const fn relationships(self) -> u64 {
        self.relationships
    }

    /// Returns the source-text representation admitted from metadata.
    #[must_use]
    pub const fn source_text_encoding(self) -> ScipSourceTextEncoding {
        self.source_text_encoding
    }
}

impl ScipOverlayDocument {
    /// Returns the canonical source file selected from the pinned manifest.
    #[must_use]
    pub const fn path(&self) -> &RepositoryPath {
        &self.path
    }

    /// Returns the exact admitted source-content digest.
    #[must_use]
    pub const fn content(&self) -> SourceContentDigest {
        self.content
    }

    /// Returns source-order exact occurrence evidence.
    #[must_use]
    pub fn occurrences(&self) -> &[ScipOccurrence] {
        &self.occurrences
    }

    /// Returns explicit package-aware symbol relationships.
    #[must_use]
    pub fn relationships(&self) -> &[ScipRelationship] {
        &self.relationships
    }
}

/// Categorical failure while decoding one immutable SCIP document batch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScipOverlayDocumentError {
    /// The producer document or its semantic fields were malformed.
    InvalidInput,
    /// The document did not match a regular file in the pinned manifest.
    SourceMismatch,
    /// The admitted SCIP index uses a source-file encoding this importer cannot validate.
    UnsupportedSourceEncoding,
    /// The caller cancelled before one complete batch was produced.
    Cancelled,
    /// The monotonic deadline elapsed before one complete batch was produced.
    DeadlineExceeded,
    /// A bounded producer field or collection exceeded its allowance.
    ResourceLimit,
}

impl fmt::Display for ScipOverlayDocumentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidInput => "invalid SCIP overlay document",
            Self::SourceMismatch => "SCIP overlay document does not match pinned source",
            Self::UnsupportedSourceEncoding => "SCIP overlay source-file encoding is not supported",
            Self::Cancelled => "SCIP overlay document decoding cancelled",
            Self::DeadlineExceeded => "SCIP overlay document decoding deadline exceeded",
            Self::ResourceLimit => "SCIP overlay document exceeded a resource limit",
        })
    }
}

impl Error for ScipOverlayDocumentError {}

struct ScipIndexDecodeContext<'input> {
    manifest: &'input SourceManifest<RepositoryPath, SourceFileKind, SourceContentDigest>,
    path_limits: RepositoryPathLimits,
    source_lookup: &'input dyn ScipImmutableSourceLookup,
    cancelled: &'input AtomicBool,
    deadline: Instant,
}

struct ScipIndexStreamState<'sink> {
    consumer_error: Option<ScipOverlayDocumentError>,
    staged_documents: u32,
    occurrences: u64,
    relationships: u64,
    on_document: &'sink mut dyn FnMut(ScipOverlayDocument) -> Result<(), ScipOverlayDocumentError>,
}

impl ScipIndexStreamState<'_> {
    fn stage(
        &mut self,
        raw_document: crate::scip_wire::ScipWireDocument<'_>,
        context: &ScipIndexDecodeContext<'_>,
    ) -> Result<(), ScipWireError> {
        if raw_document.ordinal() != self.staged_documents {
            return Err(self.reject(ScipOverlayDocumentError::InvalidInput));
        }
        let header = parse_scip_document_header(
            raw_document.bytes(),
            ScipWireControl::new(context.cancelled, context.deadline),
        )?;
        let path = validate_scip_document_path(header, context.path_limits)?;
        let Some(source_bytes) = context.source_lookup.source_bytes(&path) else {
            return Err(ScipWireError::SourceNotInManifest);
        };
        let document = decode_scip_overlay_document(
            raw_document.bytes(),
            context.manifest,
            context.path_limits,
            source_bytes,
            context.cancelled,
            context.deadline,
        )
        .map_err(|error| self.reject(error))?;
        self.reserve_document(&document)?;
        (self.on_document)(document).map_err(|error| self.reject(error))?;
        self.staged_documents = self
            .staged_documents
            .checked_add(1)
            .ok_or_else(|| self.reject(ScipOverlayDocumentError::ResourceLimit))?;
        Ok(())
    }

    fn reserve_document(&mut self, document: &ScipOverlayDocument) -> Result<(), ScipWireError> {
        let occurrences = u64::try_from(document.occurrences().len())
            .map_err(|_| self.reject(ScipOverlayDocumentError::ResourceLimit))?;
        let relationships = u64::try_from(document.relationships().len())
            .map_err(|_| self.reject(ScipOverlayDocumentError::ResourceLimit))?;
        self.occurrences = self
            .occurrences
            .checked_add(occurrences)
            .ok_or_else(|| self.reject(ScipOverlayDocumentError::ResourceLimit))?;
        self.relationships = self
            .relationships
            .checked_add(relationships)
            .ok_or_else(|| self.reject(ScipOverlayDocumentError::ResourceLimit))?;
        Ok(())
    }

    fn reject(&mut self, error: ScipOverlayDocumentError) -> ScipWireError {
        self.consumer_error = Some(error);
        ScipWireError::ConsumerRejected
    }
}

/// Streams a SCIP index through a bounded caller-owned staging sink.
///
/// Batches are provisional until this function returns a summary. The caller
/// must therefore stage them in a bounded rollback-capable transaction and
/// publish an overlay only after successful completion and its final source
/// view fence. Neither this analysis adapter nor `source_lookup` performs I/O.
pub fn decode_scip_overlay_index(
    input: &[u8],
    manifest: &SourceManifest<RepositoryPath, SourceFileKind, SourceContentDigest>,
    path_limits: RepositoryPathLimits,
    source_lookup: &impl ScipImmutableSourceLookup,
    cancelled: &AtomicBool,
    deadline: Instant,
    mut on_document: impl FnMut(ScipOverlayDocument) -> Result<(), ScipOverlayDocumentError>,
) -> Result<ScipOverlayIndexSummary, ScipOverlayDocumentError> {
    let control = ScipWireControl::new(cancelled, deadline);
    let mut metadata = None;
    let context = ScipIndexDecodeContext {
        manifest,
        path_limits,
        source_lookup,
        cancelled,
        deadline,
    };
    let mut state = ScipIndexStreamState {
        consumer_error: None,
        staged_documents: 0,
        occurrences: 0,
        relationships: 0,
        on_document: &mut on_document,
    };

    let scan = scan_scip_wire(
        input,
        ScipWireLimits::DEFAULT,
        control,
        |raw_metadata| {
            let parsed = parse_scip_metadata(raw_metadata, control)?;
            if parsed.text_encoding() != ScipWireTextEncoding::Utf8 {
                return Err(ScipWireError::UnsupportedTextEncoding);
            }
            metadata = Some(parsed);
            Ok(())
        },
        |raw_document| state.stage(raw_document, &context),
    );

    if let Some(error) = state.consumer_error {
        return Err(error);
    }
    let summary = scan.map_err(map_wire_error)?;
    let metadata = metadata.ok_or(ScipOverlayDocumentError::InvalidInput)?;
    if metadata.protocol_version() != 0 || summary.documents() != state.staged_documents {
        return Err(ScipOverlayDocumentError::InvalidInput);
    }
    debug_assert!(summary.metadata_present());
    let documents = summary.documents();
    let _ = summary.ignored_fields();
    Ok(ScipOverlayIndexSummary {
        documents,
        occurrences: state.occurrences,
        relationships: state.relationships,
        source_text_encoding: map_text_encoding(metadata.text_encoding()),
    })
}

/// Decodes one raw SCIP document into a bounded, provider-neutral fact batch.
///
/// The caller supplies immutable source bytes for the document path. It must
/// obtain those bytes from the same pinned source view as `manifest`; this
/// function checks exact membership and digest before emitting any facts.
pub fn decode_scip_overlay_document(
    raw_document: &[u8],
    manifest: &SourceManifest<RepositoryPath, SourceFileKind, SourceContentDigest>,
    path_limits: RepositoryPathLimits,
    source_bytes: &[u8],
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<ScipOverlayDocument, ScipOverlayDocumentError> {
    let control = ScipWireControl::new(cancelled, deadline);
    let header = parse_scip_document_header(raw_document, control).map_err(map_wire_error)?;
    let path = validate_scip_document_path(header, path_limits).map_err(map_wire_error)?;
    let entry = validate_scip_document_source(header, manifest, path_limits, source_bytes)
        .map_err(map_wire_error)?;
    let content = *entry.content_digest();
    let mut owned_symbol_bytes = 0_u64;
    let mut occurrences = Vec::with_capacity(
        usize::try_from(header.occurrences())
            .map_err(|_| ScipOverlayDocumentError::ResourceLimit)?,
    );
    let decoded = decode_scip_document(
        raw_document,
        manifest,
        path_limits,
        source_bytes,
        control,
        |occurrence| {
            let symbol = occurrence
                .symbol()
                .map(str::to_owned)
                .map(ScipSymbol::try_new)
                .transpose()
                .map_err(|_| ScipWireError::InvalidOccurrence)?;
            if let Some(symbol) = &symbol {
                reserve_owned_symbol_bytes(&mut owned_symbol_bytes, symbol)
                    .map_err(|_| ScipWireError::FieldTooLarge)?;
            }
            occurrences.push(ScipOccurrence::new(
                path.clone(),
                content,
                occurrence.span(),
                occurrence.ordinal(),
                symbol,
                ScipSymbolRoles::new(occurrence.roles()),
            ));
            Ok(())
        },
    )
    .map_err(map_wire_error)?;
    debug_assert_eq!(decoded.path(), &path);
    debug_assert_eq!(
        decoded.position_encoding(),
        header.position_encoding().expect("parsed header")
    );
    debug_assert_eq!(decoded.occurrences(), header.occurrences());
    let relationships = decode_document_relationships(
        raw_document,
        control,
        header.symbols(),
        &mut owned_symbol_bytes,
    )?;

    Ok(ScipOverlayDocument {
        path,
        content,
        occurrences: occurrences.into_boxed_slice(),
        relationships: relationships.into_boxed_slice(),
    })
}

fn decode_document_relationships(
    raw_document: &[u8],
    control: ScipWireControl<'_>,
    expected_symbols: u32,
    owned_symbol_bytes: &mut u64,
) -> Result<Vec<ScipRelationship>, ScipOverlayDocumentError> {
    let mut cursor = 0_usize;
    let mut relationships = Vec::with_capacity(
        usize::try_from(expected_symbols).map_err(|_| ScipOverlayDocumentError::ResourceLimit)?,
    );
    while cursor < raw_document.len() {
        control.check().map_err(map_wire_error)?;
        let tag = read_varint(raw_document, &mut cursor).map_err(map_wire_error)?;
        let field = u32::try_from(tag >> 3).map_err(|_| ScipOverlayDocumentError::InvalidInput)?;
        let wire = u8::try_from(tag & 0x07).map_err(|_| ScipOverlayDocumentError::InvalidInput)?;
        if field == 0 {
            return Err(ScipOverlayDocumentError::InvalidInput);
        }
        match (field, wire) {
            (DOCUMENT_SYMBOLS_FIELD, WIRE_LENGTH_DELIMITED) => {
                let raw = read_length_delimited(raw_document, &mut cursor, MAX_DOCUMENT_BYTES)
                    .map_err(map_wire_error)?;
                let mut raw_relationships = Vec::new();
                let information = parse_scip_symbol_information(raw, control, |relationship| {
                    raw_relationships.push(relationship);
                    Ok(())
                })
                .map_err(map_wire_error)?;
                if usize::try_from(information.relationships())
                    .map_err(|_| ScipOverlayDocumentError::ResourceLimit)?
                    != raw_relationships.len()
                {
                    return Err(ScipOverlayDocumentError::InvalidInput);
                }
                let source = ScipSymbol::try_new(information.symbol().to_owned())
                    .map_err(|_| ScipOverlayDocumentError::InvalidInput)?;
                for relationship in raw_relationships {
                    if relationships.len() == MAX_DOCUMENT_RELATIONSHIPS {
                        return Err(ScipOverlayDocumentError::ResourceLimit);
                    }
                    let target = ScipSymbol::try_new(relationship.symbol().to_owned())
                        .map_err(|_| ScipOverlayDocumentError::InvalidInput)?;
                    reserve_owned_symbol_bytes(owned_symbol_bytes, &source)?;
                    reserve_owned_symbol_bytes(owned_symbol_bytes, &target)?;
                    let kinds = ScipRelationshipKinds::try_new(
                        relationship.is_reference(),
                        relationship.is_implementation(),
                        relationship.is_type_definition(),
                        relationship.is_definition(),
                    )
                    .map_err(|_| ScipOverlayDocumentError::InvalidInput)?;
                    relationships.push(ScipRelationship::new(source.clone(), target, kinds));
                }
            }
            (_, 0) => {
                let _ = read_varint(raw_document, &mut cursor).map_err(map_wire_error)?;
            }
            (_, 1) => {
                let end = cursor
                    .checked_add(8)
                    .ok_or(ScipOverlayDocumentError::InvalidInput)?;
                if raw_document.get(cursor..end).is_none() {
                    return Err(ScipOverlayDocumentError::InvalidInput);
                }
                cursor = end;
            }
            (_, WIRE_LENGTH_DELIMITED) => {
                let _ = read_length_delimited(raw_document, &mut cursor, MAX_DOCUMENT_BYTES)
                    .map_err(map_wire_error)?;
            }
            (_, 5) => {
                let end = cursor
                    .checked_add(4)
                    .ok_or(ScipOverlayDocumentError::InvalidInput)?;
                if raw_document.get(cursor..end).is_none() {
                    return Err(ScipOverlayDocumentError::InvalidInput);
                }
                cursor = end;
            }
            _ => return Err(ScipOverlayDocumentError::InvalidInput),
        }
    }
    Ok(relationships)
}

fn reserve_owned_symbol_bytes(
    total: &mut u64,
    symbol: &ScipSymbol,
) -> Result<(), ScipOverlayDocumentError> {
    let bytes = u64::try_from(symbol.as_str().len())
        .map_err(|_| ScipOverlayDocumentError::ResourceLimit)?;
    let next = total
        .checked_add(bytes)
        .ok_or(ScipOverlayDocumentError::ResourceLimit)?;
    if next > MAX_DOCUMENT_OWNED_SYMBOL_BYTES {
        return Err(ScipOverlayDocumentError::ResourceLimit);
    }
    *total = next;
    Ok(())
}

const fn map_text_encoding(encoding: ScipWireTextEncoding) -> ScipSourceTextEncoding {
    match encoding {
        ScipWireTextEncoding::Utf8 => ScipSourceTextEncoding::Utf8,
        ScipWireTextEncoding::Utf16 => ScipSourceTextEncoding::Utf16,
    }
}

fn map_wire_error(error: ScipWireError) -> ScipOverlayDocumentError {
    match error {
        ScipWireError::Cancelled => ScipOverlayDocumentError::Cancelled,
        ScipWireError::DeadlineExceeded => ScipOverlayDocumentError::DeadlineExceeded,
        ScipWireError::InputTooLarge
        | ScipWireError::FieldTooLarge
        | ScipWireError::TooManyDocuments
        | ScipWireError::TooManyDocumentEntries
        | ScipWireError::TooManyRelationships => ScipOverlayDocumentError::ResourceLimit,
        ScipWireError::SourceNotInManifest | ScipWireError::SourceDigestMismatch => {
            ScipOverlayDocumentError::SourceMismatch
        }
        ScipWireError::UnsupportedTextEncoding => {
            ScipOverlayDocumentError::UnsupportedSourceEncoding
        }
        _ => ScipOverlayDocumentError::InvalidInput,
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::atomic::AtomicBool, time::Duration};

    use repowitness_domain::{
        RepositoryPath, SourceFileLimit, SourceManifest as DomainSourceManifest,
        SourceManifestEntry,
    };
    use sha2::{Digest, Sha256};

    use super::*;

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

    fn manifest(
        source: &[u8],
    ) -> SourceManifest<RepositoryPath, SourceFileKind, SourceContentDigest> {
        DomainSourceManifest::try_from_vec(
            vec![SourceManifestEntry::new(
                RepositoryPath::try_from_bytes(b"src/lib.rs", RepositoryPathLimits::new(128, 8))
                    .expect("path"),
                SourceFileKind::Regular,
                SourceContentDigest::new(Sha256::digest(source).into()),
            )],
            SourceFileLimit::new(1),
        )
        .expect("manifest")
    }

    struct OneSource<'input> {
        path: RepositoryPath,
        bytes: &'input [u8],
    }

    impl ScipImmutableSourceLookup for OneSource<'_> {
        fn source_bytes(&self, path: &RepositoryPath) -> Option<&[u8]> {
            (path == &self.path).then_some(self.bytes)
        }
    }

    fn valid_document() -> Vec<u8> {
        let mut range = Vec::new();
        for value in [0_u64, 1, 2] {
            push_varint(value, &mut range);
        }
        let mut occurrence = field(1, WIRE_LENGTH_DELIMITED, &range);
        occurrence.extend(field(2, WIRE_LENGTH_DELIMITED, b"scip-rust pkg 1 Item."));
        occurrence.extend(field(3, 0, &[1]));
        let mut relationship = field(1, WIRE_LENGTH_DELIMITED, b"scip-rust pkg 1 Base.");
        relationship.extend(field(3, 0, &[1]));
        let mut symbol = field(1, WIRE_LENGTH_DELIMITED, b"scip-rust pkg 1 Item.");
        symbol.extend(field(4, WIRE_LENGTH_DELIMITED, &relationship));
        let mut document = field(1, WIRE_LENGTH_DELIMITED, b"src/lib.rs");
        document.extend(field(2, WIRE_LENGTH_DELIMITED, &occurrence));
        document.extend(field(3, WIRE_LENGTH_DELIMITED, &symbol));
        document.extend(field(6, 0, &[1]));
        document
    }

    #[test]
    fn document_batch_contains_only_domain_facts_from_pinned_source() {
        let source = b"abc\n";
        let document = valid_document();
        let cancelled = AtomicBool::new(false);

        let batch = decode_scip_overlay_document(
            &document,
            &manifest(source),
            RepositoryPathLimits::new(128, 8),
            source,
            &cancelled,
            Instant::now() + Duration::from_secs(1),
        )
        .expect("batch");
        assert_eq!(batch.path().as_bytes(), b"src/lib.rs");
        assert_eq!(batch.occurrences().len(), 1);
        assert!(batch.occurrences()[0].roles().is_definition());
        assert_eq!(batch.relationships().len(), 1);
        assert!(batch.relationships()[0].kinds().is_implementation());
    }

    #[test]
    fn index_streams_only_pinned_document_batches_after_metadata_admission() {
        let source = b"abc\n";
        let limits = RepositoryPathLimits::new(128, 8);
        let path = RepositoryPath::try_from_bytes(b"src/lib.rs", limits).expect("path");
        let lookup = OneSource {
            path,
            bytes: source,
        };
        let metadata = {
            let mut metadata = field(1, 0, &[0]);
            metadata.extend(field(4, 0, &[1]));
            metadata
        };
        let mut index = field(1, WIRE_LENGTH_DELIMITED, &metadata);
        index.extend(field(2, WIRE_LENGTH_DELIMITED, &valid_document()));
        let cancelled = AtomicBool::new(false);
        let mut staged = Vec::new();

        let summary = decode_scip_overlay_index(
            &index,
            &manifest(source),
            limits,
            &lookup,
            &cancelled,
            Instant::now() + Duration::from_secs(1),
            |document| {
                staged.push(document);
                Ok(())
            },
        )
        .expect("index");

        assert_eq!(summary.documents(), 1);
        assert_eq!(summary.occurrences(), 1);
        assert_eq!(summary.relationships(), 1);
        assert_eq!(summary.source_text_encoding(), ScipSourceTextEncoding::Utf8);
        assert_eq!(staged.len(), 1);
    }

    #[test]
    fn repeated_relationship_sources_cannot_expand_one_bounded_document_unboundedly() {
        let source = b"abc\n";
        let large_symbol = vec![b's'; 16 * 1024];
        let mut relationship = field(1, WIRE_LENGTH_DELIMITED, b"x");
        relationship.extend(field(3, 0, &[1]));
        let mut symbol = field(1, WIRE_LENGTH_DELIMITED, &large_symbol);
        for _ in 0..1024 {
            symbol.extend(field(4, WIRE_LENGTH_DELIMITED, &relationship));
        }
        let mut document = field(1, WIRE_LENGTH_DELIMITED, b"src/lib.rs");
        document.extend(field(3, WIRE_LENGTH_DELIMITED, &symbol));
        document.extend(field(6, 0, &[1]));
        let cancelled = AtomicBool::new(false);

        assert_eq!(
            decode_scip_overlay_document(
                &document,
                &manifest(source),
                RepositoryPathLimits::new(128, 8),
                source,
                &cancelled,
                Instant::now() + Duration::from_secs(1),
            ),
            Err(ScipOverlayDocumentError::ResourceLimit)
        );
    }

    #[test]
    fn malformed_trailing_wire_data_cannot_report_a_completed_index() {
        let source = b"abc\n";
        let limits = RepositoryPathLimits::new(128, 8);
        let path = RepositoryPath::try_from_bytes(b"src/lib.rs", limits).expect("path");
        let lookup = OneSource {
            path,
            bytes: source,
        };
        let metadata = {
            let mut metadata = field(1, 0, &[0]);
            metadata.extend(field(4, 0, &[1]));
            metadata
        };
        let mut index = field(1, WIRE_LENGTH_DELIMITED, &metadata);
        index.extend(field(2, WIRE_LENGTH_DELIMITED, &valid_document()));
        index.push(0x80);
        let cancelled = AtomicBool::new(false);

        assert_eq!(
            decode_scip_overlay_index(
                &index,
                &manifest(source),
                limits,
                &lookup,
                &cancelled,
                Instant::now() + Duration::from_secs(1),
                |_| Ok(()),
            ),
            Err(ScipOverlayDocumentError::InvalidInput)
        );
    }

    #[test]
    fn utf16_source_metadata_is_rejected_before_any_document_batch() {
        let source = b"abc\n";
        let limits = RepositoryPathLimits::new(128, 8);
        let path = RepositoryPath::try_from_bytes(b"src/lib.rs", limits).expect("path");
        let lookup = OneSource {
            path,
            bytes: source,
        };
        let metadata = {
            let mut metadata = field(1, 0, &[0]);
            metadata.extend(field(4, 0, &[2]));
            metadata
        };
        let mut index = field(1, WIRE_LENGTH_DELIMITED, &metadata);
        index.extend(field(2, WIRE_LENGTH_DELIMITED, &valid_document()));
        let cancelled = AtomicBool::new(false);
        let mut batches = 0_u32;

        assert_eq!(
            decode_scip_overlay_index(
                &index,
                &manifest(source),
                limits,
                &lookup,
                &cancelled,
                Instant::now() + Duration::from_secs(1),
                |_| {
                    batches += 1;
                    Ok(())
                },
            ),
            Err(ScipOverlayDocumentError::UnsupportedSourceEncoding)
        );
        assert_eq!(batches, 0);
    }

    #[test]
    fn importer_schema_identity_is_fixed_and_nonempty() {
        assert_eq!(SCIP_OVERLAY_IMPORTER_VERSION, 1);
        assert_eq!(SCIP_SCHEMA_REVISION.len(), 40);
        assert_eq!(
            SCIP_SCHEMA_SHA256,
            [
                0xb3, 0x80, 0x21, 0xb6, 0x5e, 0xf9, 0x0c, 0xbb, 0xf6, 0xaf, 0x9c, 0x82, 0x9f, 0xf7,
                0x51, 0x92, 0x85, 0x9a, 0xd9, 0xb5, 0xda, 0x05, 0x43, 0x9e, 0xf1, 0x54, 0xbe, 0xa4,
                0xce, 0xb2, 0xbf, 0x03,
            ]
        );
    }
}
