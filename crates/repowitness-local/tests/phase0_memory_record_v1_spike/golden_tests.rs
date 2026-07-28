#[test]
fn commit_record_has_stable_generated_yaml_canonical_json_and_digest() {
    assert_golden(
        COMMIT_YAML,
        COMMIT_YAML_SHA256,
        COMMIT_CANONICAL,
        COMMIT_DIGEST,
    );
}

#[test]
fn worktree_relationship_has_stable_generated_yaml_canonical_json_and_digest() {
    assert_golden(
        WORKTREE_YAML,
        WORKTREE_YAML_SHA256,
        WORKTREE_CANONICAL,
        WORKTREE_DIGEST,
    );
}

#[test]
fn record_id_encoding_has_exact_golden_vectors_and_rejects_alternates() {
    let vectors = [
        ([0_u8; 16], "mem_00000000000000000000000000"),
        ([u8::MAX; 16], "mem_7ZZZZZZZZZZZZZZZZZZZZZZZZZ"),
        (
            [
                0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d,
                0x0e, 0x0f,
            ],
            "mem_00041061050R3GG28A1C60T3GF",
        ),
    ];
    for (bytes, expected) in vectors {
        assert_eq!(encode_record_id(bytes), expected);
        assert_eq!(decode_record_id(expected), Some(bytes));
    }

    for invalid in [
        "mem_80000000000000000000000000",
        "mem_0000000000000000000000000I",
        "mem_0000000000000000000000000l",
        "MEM_00000000000000000000000000",
        "mem_0000000000000000000000000",
        "mem_000000000000000000000000000",
    ] {
        assert_eq!(decode_record_id(invalid), None, "{invalid}");
    }
}

#[test]
fn presentation_and_display_revision_do_not_change_semantic_identity() {
    let baseline = parse_strict_memory(WORKTREE_YAML).expect("baseline fixture must parse");
    let input = str::from_utf8(WORKTREE_YAML).expect("fixture is UTF-8");
    let reordered = input
        .replacen("display_revision: 9", "display_revision: 10", 1)
        .replacen(
            "  - \"6666666666666666666666666666666666666666666666666666666666666666\"\n  - \"7777777777777777777777777777777777777777777777777777777777777777\"",
            "  - \"7777777777777777777777777777777777777777777777777777777777777777\"\n  - \"6666666666666666666666666666666666666666666666666666666666666666\"",
            1,
        )
        .replacen(
            "title: \"Avoid publishing stale worktree evidence\"",
            "title: 'Avoid publishing stale worktree evidence'",
            1,
        );
    let reordered = format!("# presentation-only comment\n{reordered}");
    let reordered =
        parse_strict_memory(reordered.as_bytes()).expect("presentation variant must parse");
    assert_eq!(
        canonical_digest(&baseline).expect("baseline hashes"),
        canonical_digest(&reordered).expect("variant hashes")
    );
}

#[test]
fn generated_yaml_escapes_unicode_line_breaks_without_semantic_drift() {
    let mut dto = parse_strict_memory(COMMIT_YAML)
        .expect("baseline fixture must parse")
        .0;
    dto.body = "quote \" slash \\ tab \t controls \u{1}\u{7f}\u{80}\u{84}\u{85}\u{86}\u{9f}\n\
                line two\u{2028}line three\u{2029}line four"
        .to_owned();
    let record = validate_memory_record(dto).expect("line-break record must validate");
    let generated = generated_yaml(&record).expect("line-break record must generate");
    let generated_text = str::from_utf8(&generated).expect("generated YAML is UTF-8");

    assert!(generated_text.contains("\\\""));
    assert!(generated_text.contains("\\\\"));
    assert!(generated_text.contains("\\t"));
    assert!(generated_text.contains("\\u0001"));
    assert!(generated_text.contains("\\u007f"));
    assert!(generated_text.contains("\\u0080"));
    assert!(generated_text.contains("\\u0084"));
    assert!(generated_text.contains("\\n"));
    assert!(generated_text.contains("\\u0085"));
    assert!(generated_text.contains("\\u0086"));
    assert!(generated_text.contains("\\u009f"));
    assert!(generated_text.contains("\\u2028"));
    assert!(generated_text.contains("\\u2029"));
    assert!(!generated_text.contains('\u{85}'));
    assert!(!generated_text.contains('\u{2028}'));
    assert!(!generated_text.contains('\u{2029}'));

    let reparsed = parse_strict_memory(&generated).expect("generated YAML must reparse");
    assert_eq!(
        canonical_digest(&record).expect("source record must hash"),
        canonical_digest(&reparsed).expect("reparsed record must hash")
    );
}
