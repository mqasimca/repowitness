//! Test-only evaluation of a strict, bounded YAML-to-canonical-memory boundary.
//!
//! This spike proves admission behavior required by ADR-0007 without making
//! its deliberately small DTO a production memory schema.

use std::fmt;

use granit_parser::{Event, Parser};
use repowitness_domain::CanonicalMemoryDigest;
use serde::{Deserialize, Serialize};
use serde_saphyr::{DuplicateKeyPolicy, MergeKeyPolicy};
use sha2::{Digest, Sha256};

const MAX_INPUT_BYTES: usize = 64 * 1024;
const MAX_RECORD_ID_BYTES: usize = 64;
const MAX_PARENT_DIGESTS: usize = 8;
const MAX_TITLE_BYTES: usize = 256;
const MAX_BODY_BYTES: usize = 16 * 1024;
const MAX_TOTAL_SCALAR_BYTES: usize = 32 * 1024;

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum MemoryKind {
    Decision,
    Failure,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MemoryRecordDto {
    schema_version: u32,
    record_id: String,
    display_revision: u32,
    #[serde(default)]
    parent_digests: Vec<String>,
    kind: MemoryKind,
    title: String,
    body: String,
    tombstone: bool,
}

struct ValidatedMemoryRecord {
    schema_version: u32,
    record_id: String,
    display_revision: u32,
    parent_digests: Vec<String>,
    kind: MemoryKind,
    title: String,
    body: String,
    tombstone: bool,
}

#[derive(Serialize)]
struct CanonicalMemoryRecord<'a> {
    schema_version: u32,
    record_id: &'a str,
    parent_digests: &'a [String],
    kind: MemoryKind,
    title: &'a str,
    body: &'a str,
    tombstone: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StrictMemoryError {
    InputTooLarge,
    InvalidYaml,
    InvalidRecord,
    CanonicalizationFailed,
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
            Event::DocumentStart(..) => increment_bounded(&mut self.documents, 1),
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

impl fmt::Display for StrictMemoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InputTooLarge => "memory YAML exceeds its byte limit",
            Self::InvalidYaml => "memory YAML is invalid",
            Self::InvalidRecord => "memory record is invalid",
            Self::CanonicalizationFailed => "memory canonicalization failed",
        })
    }
}

fn parse_strict_memory(input: &[u8]) -> Result<ValidatedMemoryRecord, StrictMemoryError> {
    if input.len() > MAX_INPUT_BYTES {
        return Err(StrictMemoryError::InputTooLarge);
    }
    reject_yaml_extensions(input)?;

    let options = serde_saphyr::options! {
        budget: serde_saphyr::budget! {
            max_reader_input_bytes: Some(MAX_INPUT_BYTES),
            max_events: 4_096,
            max_aliases: 0,
            max_anchors: 0,
            max_depth: 8,
            max_inclusion_depth: 0,
            max_documents: 1,
            max_nodes: 2_048,
            max_total_scalar_bytes: MAX_TOTAL_SCALAR_BYTES,
            max_total_comment_bytes: 4 * 1_024,
            max_merge_keys: 0,
        },
        duplicate_keys: DuplicateKeyPolicy::Error,
        merge_keys: MergeKeyPolicy::Error,
        alias_limits: serde_saphyr::alias_limits! {
            max_total_replayed_events: 0,
            max_replay_stack_depth: 0,
            max_alias_expansions_per_anchor: 0,
        },
        legacy_octal_numbers: false,
        strict_booleans: true,
        no_schema: true,
        with_snippet: false,
        crop_radius: 0,
    };
    let dto: MemoryRecordDto = serde_saphyr::from_slice_with_options(input, options)
        .map_err(|_| StrictMemoryError::InvalidYaml)?;

    validate_memory_record(dto)
}

fn reject_yaml_extensions(input: &[u8]) -> Result<(), StrictMemoryError> {
    let input = std::str::from_utf8(input).map_err(|_| StrictMemoryError::InvalidYaml)?;
    let mut preflight = YamlPreflight::default();

    for parsed in Parser::new_from_str(input) {
        let (event, _) = parsed.map_err(|_| StrictMemoryError::InvalidYaml)?;
        preflight.observe(event)?;
    }
    Ok(())
}

fn increment_bounded(value: &mut usize, limit: usize) -> Result<(), StrictMemoryError> {
    *value = value.checked_add(1).ok_or(StrictMemoryError::InvalidYaml)?;
    if *value > limit {
        return Err(StrictMemoryError::InvalidYaml);
    }
    Ok(())
}

fn validate_memory_record(
    dto: MemoryRecordDto,
) -> Result<ValidatedMemoryRecord, StrictMemoryError> {
    if dto.schema_version != 1
        || !valid_record_id(&dto.record_id)
        || dto.parent_digests.len() > MAX_PARENT_DIGESTS
        || dto.title.is_empty()
        || dto.title.len() > MAX_TITLE_BYTES
        || dto.body.len() > MAX_BODY_BYTES
    {
        return Err(StrictMemoryError::InvalidRecord);
    }

    let mut parent_digests = dto.parent_digests;
    if parent_digests
        .iter()
        .any(|digest| !valid_lower_hex_sha256(digest))
    {
        return Err(StrictMemoryError::InvalidRecord);
    }
    parent_digests.sort_unstable();
    if parent_digests.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(StrictMemoryError::InvalidRecord);
    }

    Ok(ValidatedMemoryRecord {
        schema_version: dto.schema_version,
        record_id: dto.record_id,
        display_revision: dto.display_revision,
        parent_digests,
        kind: dto.kind,
        title: dto.title,
        body: dto.body,
        tombstone: dto.tombstone,
    })
}

fn valid_record_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= MAX_RECORD_ID_BYTES
        && bytes[0].is_ascii_lowercase()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
}

fn valid_lower_hex_sha256(value: &str) -> bool {
    value.len() == 64
        && value.as_bytes().iter().all(u8::is_ascii_hexdigit)
        && !value.as_bytes().iter().any(u8::is_ascii_uppercase)
}

fn canonical_bytes(record: &ValidatedMemoryRecord) -> Result<Vec<u8>, StrictMemoryError> {
    let semantic = CanonicalMemoryRecord {
        schema_version: record.schema_version,
        record_id: &record.record_id,
        parent_digests: &record.parent_digests,
        kind: record.kind,
        title: &record.title,
        body: &record.body,
        tombstone: record.tombstone,
    };
    serde_json_canonicalizer::to_vec(&semantic)
        .map_err(|_| StrictMemoryError::CanonicalizationFailed)
}

fn canonical_digest(
    record: &ValidatedMemoryRecord,
) -> Result<CanonicalMemoryDigest, StrictMemoryError> {
    let canonical = canonical_bytes(record)?;
    let length =
        u64::try_from(canonical.len()).map_err(|_| StrictMemoryError::CanonicalizationFailed)?;
    let mut hasher = Sha256::new();
    hasher.update(b"RepoWitness\0memory-record\0");
    hasher.update(1_u32.to_be_bytes());
    hasher.update(length.to_be_bytes());
    hasher.update(canonical);
    Ok(CanonicalMemoryDigest::new(hasher.finalize().into()))
}

fn valid_yaml() -> Vec<u8> {
    format!(
        "schema_version: 1\n\
         record_id: memory-1\n\
         display_revision: 7\n\
         parent_digests:\n\
           - \"{}\"\n\
         kind: decision\n\
         title: \"Atomic generations\"\n\
         body: \"Use atomic publication.\"\n\
         tombstone: false\n",
        "1".repeat(64)
    )
    .into_bytes()
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}

fn assert_error(
    result: Result<ValidatedMemoryRecord, StrictMemoryError>,
    expected: StrictMemoryError,
    context: &str,
) {
    match result {
        Ok(_) => panic!("{context} unexpectedly produced a memory record"),
        Err(actual) => assert_eq!(actual, expected, "{context}"),
    }
}

#[test]
fn accepted_input_has_stable_canonical_json_and_digest() {
    let record = parse_strict_memory(&valid_yaml()).expect("fixture should parse");
    let bytes = canonical_bytes(&record).expect("fixture should canonicalize");
    assert_eq!(
        String::from_utf8(bytes).expect("canonical JSON is UTF-8"),
        format!(
            "{{\"body\":\"Use atomic publication.\",\"kind\":\"decision\",\
             \"parent_digests\":[\"{}\"],\"record_id\":\"memory-1\",\
             \"schema_version\":1,\"title\":\"Atomic generations\",\
             \"tombstone\":false}}",
            "1".repeat(64)
        )
    );
    assert_eq!(
        hex(canonical_digest(&record)
            .expect("fixture should hash")
            .as_bytes()),
        "1332aa1fe5ecbe998b78df6f785b8c253c5703946a90eee217a50069791c95ec"
    );
}

#[test]
fn presentation_and_parent_order_do_not_change_the_digest() {
    let first = format!(
        "schema_version: 1\nrecord_id: memory-1\ndisplay_revision: 1\n\
         parent_digests: [\"{}\", \"{}\"]\nkind: decision\n\
         title: \"Atomic generations\"\nbody: \"Use atomic publication.\"\n\
         tombstone: false\n",
        "2".repeat(64),
        "1".repeat(64)
    );
    let second = format!(
        "# presentation-only comment\nbody: \"Use atomic publication.\"\n\
         tombstone: false\ntitle: \"Atomic generations\"\nkind: decision\n\
         parent_digests:\n  - \"{}\"\n  - \"{}\"\n\
         display_revision: 99\nrecord_id: memory-1\nschema_version: 1\n",
        "1".repeat(64),
        "2".repeat(64)
    );
    let first = parse_strict_memory(first.as_bytes()).expect("fixture should parse");
    let second = parse_strict_memory(second.as_bytes()).expect("fixture should parse");
    assert_eq!(first.display_revision, 1);
    assert_eq!(second.display_revision, 99);
    assert_eq!(
        canonical_digest(&first).expect("fixture should hash"),
        canonical_digest(&second).expect("fixture should hash")
    );
}

#[test]
fn every_semantic_field_changes_the_digest() {
    let baseline = parse_strict_memory(&valid_yaml()).expect("baseline fixture should parse");
    let baseline_digest = canonical_digest(&baseline).expect("baseline fixture should hash");
    let replacements = [
        ("memory-1", "memory-2"),
        ("kind: decision", "kind: failure"),
        ("Atomic generations", "Retained generations"),
        ("Use atomic publication.", "Use staged publication."),
        ("tombstone: false", "tombstone: true"),
        (&"1".repeat(64), &"2".repeat(64)),
    ];

    let fixture = String::from_utf8(valid_yaml()).expect("fixture is UTF-8");
    for (old, new) in replacements {
        let changed = parse_strict_memory(fixture.replacen(old, new, 1).as_bytes())
            .expect("changed fixture should parse");
        assert_ne!(
            canonical_digest(&changed).expect("changed fixture should hash"),
            baseline_digest
        );
    }
}

#[test]
fn hostile_yaml_features_and_unknown_fields_are_rejected() {
    let fixture = String::from_utf8(valid_yaml()).expect("fixture is UTF-8");
    let invalid = [
        (
            "duplicate key",
            fixture.replacen(
                "record_id: memory-1",
                "record_id: memory-1\nrecord_id: memory-2",
                1,
            ),
        ),
        (
            "anchor",
            fixture.replacen(
                "title: \"Atomic generations\"",
                "title: &title \"Atomic generations\"",
                1,
            ),
        ),
        (
            "alias",
            fixture.replacen("title: \"Atomic generations\"", "title: *title", 1),
        ),
        (
            "custom tag",
            fixture.replacen(
                "title: \"Atomic generations\"",
                "title: !secret \"Atomic generations\"",
                1,
            ),
        ),
        (
            "float in string field",
            fixture.replacen("title: \"Atomic generations\"", "title: 1.5", 1),
        ),
        (
            "merge key",
            fixture.replacen(
                "tombstone: false",
                "<<: {tombstone: false}\ntombstone: false",
                1,
            ),
        ),
        (
            "unknown field",
            fixture.replacen("tombstone: false", "tombstone: false\nunknown: value", 1),
        ),
        ("multiple documents", format!("{fixture}---\n{fixture}")),
    ];

    for (context, input) in invalid {
        assert_error(
            parse_strict_memory(input.as_bytes()),
            StrictMemoryError::InvalidYaml,
            context,
        );
    }
}

#[test]
fn schema_and_resource_limits_fail_closed() {
    let fixture = String::from_utf8(valid_yaml()).expect("fixture is UTF-8");
    let oversized = vec![b'a'; MAX_INPUT_BYTES + 1];
    assert_error(
        parse_strict_memory(&oversized),
        StrictMemoryError::InputTooLarge,
        "oversized input",
    );
    assert_error(
        parse_strict_memory(&[0xff]),
        StrictMemoryError::InvalidYaml,
        "invalid UTF-8",
    );
    assert_error(
        parse_strict_memory(
            fixture
                .replacen("schema_version: 1", "schema_version: 2", 1)
                .as_bytes(),
        ),
        StrictMemoryError::InvalidRecord,
        "unsupported schema",
    );
    assert_error(
        parse_strict_memory(
            fixture
                .replacen("record_id: memory-1", "record_id: ../memory", 1)
                .as_bytes(),
        ),
        StrictMemoryError::InvalidRecord,
        "invalid record ID",
    );

    let too_many_parents = (0_u8..=MAX_PARENT_DIGESTS as u8)
        .map(|value| format!("- \"{:064x}\"\n", value))
        .collect::<String>();
    let input = fixture.replacen(&format!("- \"{}\"\n", "1".repeat(64)), &too_many_parents, 1);
    assert_error(
        parse_strict_memory(input.as_bytes()),
        StrictMemoryError::InvalidRecord,
        "too many parents",
    );
}

#[test]
fn diagnostics_do_not_expose_memory_content() {
    let secret = "do-not-expose-this-memory";
    let input = format!("{secret}: [");
    let error = match parse_strict_memory(input.as_bytes()) {
        Ok(_) => panic!("input unexpectedly produced a memory record"),
        Err(error) => error,
    };
    let display = error.to_string();
    let debug = format!("{error:?}");
    assert!(!display.contains(secret));
    assert!(!debug.contains(secret));
}
