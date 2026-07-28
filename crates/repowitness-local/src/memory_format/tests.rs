use std::{
    io::Write as _,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use sha2::{Digest, Sha256};

use super::{
    CanonicalMemoryOutput, MAX_CANONICAL_MEMORY_BYTES, MAX_MEMORY_YAML_BYTES, MemoryFormatControl,
    MemoryFormatError, MemoryYamlOutput, canonical_memory_digest, encode_lower_hex,
    generate_memory_yaml, parse_memory_record, parse_persisted_canonical_memory_record,
};
use repowitness_application::MemoryRecordIdTextV1;
use repowitness_domain::{
    CanonicalMemoryDigest, MemoryDisplayRevision, MemoryRecordError, MemoryRecordId,
};

const COMMIT_YAML: &[u8] = include_bytes!("../../tests/fixtures/memory-v1/commit.yaml");
const COMMIT_CANONICAL: &str = include_str!("../../tests/fixtures/memory-v1/commit.canonical.json");
const COMMIT_DIGEST: &str = include_str!("../../tests/fixtures/memory-v1/commit.digest");
const WORKTREE_YAML: &[u8] =
    include_bytes!("../../tests/fixtures/memory-v1/worktree-relationship.yaml");
const WORKTREE_CANONICAL: &str =
    include_str!("../../tests/fixtures/memory-v1/worktree-relationship.canonical.json");
const WORKTREE_DIGEST: &str =
    include_str!("../../tests/fixtures/memory-v1/worktree-relationship.digest");

fn control(cancelled: &AtomicBool) -> MemoryFormatControl<'_> {
    MemoryFormatControl::new(cancelled, Instant::now() + Duration::from_secs(5))
}

fn assert_golden(yaml: &[u8], expected_canonical: &str, expected_digest: &str) {
    let cancelled = AtomicBool::new(false);
    let parsed = parse_memory_record(yaml, control(&cancelled)).expect("golden YAML must parse");
    assert_eq!(
        parsed.canonical_json(),
        expected_canonical.trim_end().as_bytes()
    );
    assert_eq!(
        encode_lower_hex(parsed.digest().as_bytes()),
        expected_digest.trim_end()
    );
    assert_eq!(
        generate_memory_yaml(parsed.record(), control(&cancelled))
            .expect("golden record must generate"),
        yaml
    );
    assert_eq!(
        canonical_memory_digest(parsed.record(), control(&cancelled))
            .expect("golden record must hash"),
        parsed.digest()
    );
}

#[test]
fn production_parser_writer_and_digest_match_both_golden_profiles() {
    assert_golden(COMMIT_YAML, COMMIT_CANONICAL, COMMIT_DIGEST);
    assert_golden(WORKTREE_YAML, WORKTREE_CANONICAL, WORKTREE_DIGEST);
}

#[test]
fn record_id_encoding_has_exact_vectors_and_rejects_alternates() {
    let vectors = [
        ([0_u8; 16], "mem_00000000000000000000000000"),
        ([0xff_u8; 16], "mem_7ZZZZZZZZZZZZZZZZZZZZZZZZZ"),
        (
            [
                0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d,
                0x0e, 0x0f,
            ],
            "mem_00041061050R3GG28A1C60T3GF",
        ),
    ];
    for (bytes, text) in vectors {
        let record_id = MemoryRecordId::new(bytes);
        assert_eq!(MemoryRecordIdTextV1::encode(record_id).as_str(), text);
        assert_eq!(MemoryRecordIdTextV1::decode(text), Ok(record_id));
    }
    for invalid in [
        "mem_80000000000000000000000000",
        "mem_0000000000000000000000000O",
        "MEM_00000000000000000000000000",
        "mem_0000000000000000000000000",
    ] {
        assert!(MemoryRecordIdTextV1::decode(invalid).is_err());
    }
}

#[test]
fn hostile_yaml_and_unknown_semantics_fail_closed() {
    let cancelled = AtomicBool::new(false);
    let source = String::from_utf8(COMMIT_YAML.to_vec()).expect("fixture is UTF-8");
    for hostile in [
        source.replacen("title: ", "title: !secret ", 1),
        source.replacen("title: ", "title: &claim ", 1),
        source.replacen("body: ", "body: *claim\nignored: ", 1),
        source.replacen("tombstone: false", "unknown: value\ntombstone: false", 1),
        format!("{source}---\n"),
    ] {
        assert!(matches!(
            parse_memory_record(hostile.as_bytes(), control(&cancelled)),
            Err(MemoryFormatError::InvalidYaml) | Err(MemoryFormatError::InvalidRecord(_))
        ));
    }
    let crlf = source.replace('\n', "\r\n");
    assert_eq!(
        parse_memory_record(crlf.as_bytes(), control(&cancelled)),
        Err(MemoryFormatError::InvalidYaml)
    );
    let bad_schema = source.replacen("schema_version: 1", "schema_version: 2", 1);
    assert_eq!(
        parse_memory_record(bad_schema.as_bytes(), control(&cancelled)),
        Err(MemoryFormatError::InvalidRecord(
            MemoryRecordError::InvalidSchemaVersion
        ))
    );
}

#[test]
fn presentation_revision_is_not_semantic_but_claim_text_is() {
    let cancelled = AtomicBool::new(false);
    let source = String::from_utf8(COMMIT_YAML.to_vec()).expect("fixture is UTF-8");
    let baseline =
        parse_memory_record(source.as_bytes(), control(&cancelled)).expect("valid fixture");
    let display = source.replacen("display_revision: 1", "display_revision: 9", 1);
    let display =
        parse_memory_record(display.as_bytes(), control(&cancelled)).expect("valid revision");
    assert_eq!(baseline.digest(), display.digest());

    let semantic = source.replacen("kind: decision", "kind: failure", 1);
    let semantic =
        parse_memory_record(semantic.as_bytes(), control(&cancelled)).expect("valid claim");
    assert_ne!(baseline.digest(), semantic.digest());
}

#[test]
fn persisted_canonical_records_are_reconstructed_and_verified_exactly() {
    let cancelled = AtomicBool::new(false);
    let parsed =
        parse_memory_record(COMMIT_YAML, control(&cancelled)).expect("valid memory fixture");
    let reconstructed = parse_persisted_canonical_memory_record(
        parsed.canonical_json(),
        MemoryDisplayRevision::try_new(42).expect("display revision"),
        parsed.digest(),
        control(&cancelled),
    )
    .expect("persisted canonical record");
    assert_eq!(reconstructed.digest(), parsed.digest());
    assert_eq!(reconstructed.record().header().display_revision().get(), 42);

    let mut noncanonical = parsed.canonical_json().to_vec();
    noncanonical.insert(1, b' ');
    assert_eq!(
        parse_persisted_canonical_memory_record(
            &noncanonical,
            MemoryDisplayRevision::try_new(1).expect("display revision"),
            parsed.digest(),
            control(&cancelled),
        ),
        Err(MemoryFormatError::InvalidCanonicalRecord)
    );
    assert_eq!(
        parse_persisted_canonical_memory_record(
            parsed.canonical_json(),
            MemoryDisplayRevision::try_new(1).expect("display revision"),
            CanonicalMemoryDigest::new([0xff; 32]),
            control(&cancelled),
        ),
        Err(MemoryFormatError::InvalidCanonicalRecord)
    );
}

#[test]
fn cancellation_deadline_and_diagnostics_are_explicit_and_redacted() {
    let cancelled = AtomicBool::new(true);
    assert_eq!(
        parse_memory_record(COMMIT_YAML, control(&cancelled)),
        Err(MemoryFormatError::Cancelled)
    );

    cancelled.store(false, Ordering::Relaxed);
    let expired = MemoryFormatControl::new(
        &cancelled,
        Instant::now()
            .checked_sub(Duration::from_secs(1))
            .expect("instant supports subtraction"),
    );
    assert_eq!(
        parse_memory_record(COMMIT_YAML, expired),
        Err(MemoryFormatError::DeadlineExceeded)
    );

    let secret = b"schema_version: 1\ntitle: do-not-expose-this\n";
    let error = parse_memory_record(secret, control(&cancelled)).expect_err("must fail");
    let diagnostic = format!("{error:?} {error}");
    assert!(!diagnostic.contains("do-not-expose-this"));
    assert!(!diagnostic.contains("title:"));
}

#[test]
fn fixture_integrity_checks_remain_independent_of_record_identity() {
    let commit: [u8; 32] = Sha256::digest(COMMIT_YAML).into();
    let worktree: [u8; 32] = Sha256::digest(WORKTREE_YAML).into();
    assert_eq!(
        encode_lower_hex(&commit),
        "916d2366754e37a20ac49416172a88815d5bd47aa5477c5eaac41062e7c90c1f"
    );
    assert_eq!(
        encode_lower_hex(&worktree),
        "762a1220300cc182a129c20864dd15c3bdbc4a59b997ecb3c963f970a7b8e083"
    );
}

#[test]
fn canonical_and_yaml_outputs_never_cross_their_declared_bounds() {
    let mut canonical = CanonicalMemoryOutput::default();
    canonical
        .write_all(&vec![0_u8; MAX_CANONICAL_MEMORY_BYTES])
        .expect("the inclusive canonical limit must fit");
    assert!(canonical.write_all(&[0]).is_err());
    assert_eq!(canonical.into_bytes().len(), MAX_CANONICAL_MEMORY_BYTES);

    let mut yaml = MemoryYamlOutput::default();
    std::fmt::Write::write_str(&mut yaml, &"x".repeat(MAX_MEMORY_YAML_BYTES))
        .expect("the inclusive YAML limit must fit");
    assert!(std::fmt::Write::write_str(&mut yaml, "x").is_err());
    assert_eq!(yaml.into_bytes().len(), MAX_MEMORY_YAML_BYTES);
}
