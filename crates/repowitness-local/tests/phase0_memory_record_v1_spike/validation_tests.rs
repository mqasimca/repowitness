#[test]
fn directives_and_ambiguous_yaml_features_fail_closed() {
    let fixture = str::from_utf8(COMMIT_YAML).expect("fixture is UTF-8");
    let invalid = [
        fixture.replacen(
            "title: \"Keep generation publication atomic\"",
            "title: !secret \"Keep generation publication atomic\"",
            1,
        ),
        fixture.replacen(
            "title: \"Keep generation publication atomic\"",
            "title: &title \"Keep generation publication atomic\"",
            1,
        ),
        fixture.replacen(
            "body: \"Readers must never observe a partially staged generation.\"",
            "body: *title",
            1,
        ),
        fixture.replacen(
            "actor_id: \"maintainer\"",
            "actor_id: \"maintainer\"\n  actor_id: \"forged\"",
            1,
        ),
        fixture.replacen(
            "actor_id: \"maintainer\"",
            "<<: {actor_id: \"forged\"}\n  actor_id: \"maintainer\"",
            1,
        ),
        fixture.replacen(
            "subject_evidence: 0",
            "subject_evidence: 0\n  unknown: true",
            1,
        ),
        fixture.replacen("fact_ordinal: 0", "fact_ordinal: 1.5", 1),
        format!("%YAML 1.2\n---\n{fixture}"),
        format!("%TAG !e! tag:example.invalid,2026:\n---\n{fixture}"),
        format!("%REPOWITNESS forbidden\n---\n{fixture}"),
        format!("{fixture}---\n{fixture}"),
        fixture.replace('\n', "\r\n"),
    ];

    for input in invalid {
        assert!(parse_strict_memory(input.as_bytes()).is_err());
    }
    assert!(matches!(
        parse_strict_memory(&[0xff]),
        Err(StrictMemoryError::InvalidYaml)
    ));
}

#[test]
fn input_and_canonical_output_resource_bounds_are_independent() {
    let baseline = parse_strict_memory(COMMIT_YAML).expect("baseline fixture must parse");
    let baseline_digest = canonical_digest(&baseline).expect("baseline fixture must hash");

    let mut exact_input = COMMIT_YAML.to_vec();
    exact_input.resize(MAX_INPUT_BYTES, b' ');
    let exact = parse_strict_memory(&exact_input).expect("exact input limit must pass");
    assert_eq!(
        canonical_digest(&exact).expect("exact-limit input must hash"),
        baseline_digest
    );
    exact_input.push(b' ');
    assert!(matches!(
        parse_strict_memory(&exact_input),
        Err(StrictMemoryError::InputTooLarge)
    ));

    let mut oversized_canonical = baseline.0;
    oversized_canonical.body = "x".repeat(MAX_CANONICAL_BYTES + 1);
    assert!(matches!(
        canonical_bytes(&ValidatedMemoryRecord(oversized_canonical)),
        Err(StrictMemoryError::CanonicalizationFailed)
    ));
}

#[test]
fn deterministic_parser_mutations_never_panic_or_bypass_canonicalization() {
    let replacements = [0, b'\n', b':', b'&', b'*', b'!', 0xff];
    for index in (0..COMMIT_YAML.len()).step_by(4) {
        for replacement in replacements {
            let mut mutated = COMMIT_YAML.to_vec();
            mutated[index] = replacement;
            if let Ok(record) = parse_strict_memory(&mutated) {
                canonical_digest(&record).expect("every admitted mutation must canonicalize");
            }
        }
    }

    for end in (0..COMMIT_YAML.len()).step_by(31) {
        if let Ok(record) = parse_strict_memory(&COMMIT_YAML[..end]) {
            canonical_digest(&record).expect("every admitted truncation must canonicalize");
        }
    }

    let mut state = 0xbb67_ae85_84ca_a73b_u64;
    for _ in 0..512 {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let length = usize::try_from(state % 257).expect("bounded length fits usize");
        let mut input = Vec::with_capacity(length);
        for _ in 0..length {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            input.push(state.to_le_bytes()[0]);
        }
        if let Ok(record) = parse_strict_memory(&input) {
            canonical_digest(&record).expect("every admitted generated input must canonicalize");
        }
    }
}

#[test]
fn cross_field_invariants_fail_closed() {
    let baseline = parse_strict_memory(COMMIT_YAML)
        .expect("baseline fixture must parse")
        .0;
    let mut cases = Vec::new();

    let mut invalid = baseline.clone();
    invalid.record_id = "mem_80000000000000000000000000".to_owned();
    cases.push(invalid);

    let mut invalid = baseline.clone();
    invalid.scope.subject_evidence = 1;
    cases.push(invalid);

    let mut invalid = baseline.clone();
    invalid.evidence[0].name_length = 6;
    cases.push(invalid);

    let mut invalid = baseline.clone();
    invalid.evidence[0].fact_ordinal = MAX_INTEROPERABLE_INTEGER + 1;
    cases.push(invalid);

    let mut invalid = baseline.clone();
    invalid.evidence[0].content_digest = "A".repeat(64);
    cases.push(invalid);

    let mut invalid = baseline.clone();
    invalid.tombstone = true;
    cases.push(invalid);

    let mut invalid = baseline.clone();
    if let ValidityDto::Commits {
        introduced_by,
        invalidated_by,
    } = &mut invalid.validity
    {
        invalidated_by.push(introduced_by[0].clone());
    }
    cases.push(invalid);

    let mut invalid = baseline;
    invalid.relationships = vec![
        RelationshipDto {
            kind: RelationshipKind::Contradicts,
            record_id: "mem_00000000000000000000000000".to_owned(),
            revision_digest: "a".repeat(64),
        };
        MAX_RELATIONSHIPS + 1
    ];
    cases.push(invalid);

    for (index, invalid) in cases.into_iter().enumerate() {
        assert!(
            matches!(
                validate_memory_record(invalid),
                Err(StrictMemoryError::InvalidRecord)
            ),
            "invalid case {index} unexpectedly passed"
        );
    }
}

#[test]
fn diagnostics_do_not_expose_memory_or_yaml_content() {
    let secret = "do-not-expose-this-memory";
    let error = parse_strict_memory(format!("{secret}: [").as_bytes())
        .expect_err("malformed input must fail");
    assert!(!error.to_string().contains(secret));
    assert!(!format!("{error:?}").contains(secret));
}

#[test]
#[ignore = "manual synthetic parser/canonicalizer resource probe"]
fn parser_and_canonicalizer_resource_probe() {
    const ITERATIONS: usize = 10_000;
    let fixtures = [COMMIT_YAML, WORKTREE_YAML];
    let mut input_bytes = 0_u64;
    let mut canonical_bytes_total = 0_u64;
    let mut digest_checksum = 0_u64;

    for iteration in 0..ITERATIONS {
        let input = fixtures[iteration % fixtures.len()];
        let record = parse_strict_memory(input).expect("resource fixture must parse");
        let canonical = canonical_bytes(&record).expect("resource fixture must canonicalize");
        let digest =
            digest_canonical_bytes(&canonical).expect("resource fixture canonical bytes must hash");
        input_bytes = input_bytes
            .checked_add(u64::try_from(input.len()).expect("fixture length fits u64"))
            .expect("bounded probe input total fits u64");
        canonical_bytes_total = canonical_bytes_total
            .checked_add(u64::try_from(canonical.len()).expect("canonical length fits u64"))
            .expect("bounded probe canonical total fits u64");
        let digest_prefix = u64::from_be_bytes(
            digest.as_bytes()[..8]
                .try_into()
                .expect("digest prefix is fixed"),
        );
        digest_checksum = digest_checksum.wrapping_add(digest_prefix);
    }

    eprintln!(
        "memory_v1_resource_probe iterations={ITERATIONS} input_bytes={input_bytes} \
         canonical_bytes={canonical_bytes_total} digest_checksum={digest_checksum}"
    );
}

#[test]
#[ignore = "manual maximum-input parser/canonicalizer resource probe"]
fn maximum_input_resource_probe() {
    const ITERATIONS: usize = 1_000;
    let mut input = COMMIT_YAML.to_vec();
    input.resize(MAX_INPUT_BYTES, b' ');
    let mut canonical_bytes_total = 0_u64;
    let mut digest_checksum = 0_u64;

    for _ in 0..ITERATIONS {
        let record = parse_strict_memory(&input).expect("maximum input fixture must parse");
        let canonical = canonical_bytes(&record).expect("maximum input fixture must canonicalize");
        let digest = digest_canonical_bytes(&canonical)
            .expect("maximum input fixture canonical bytes must hash");
        canonical_bytes_total = canonical_bytes_total
            .checked_add(u64::try_from(canonical.len()).expect("canonical length fits u64"))
            .expect("bounded probe canonical total fits u64");
        let digest_prefix = u64::from_be_bytes(
            digest.as_bytes()[..8]
                .try_into()
                .expect("digest prefix is fixed"),
        );
        digest_checksum = digest_checksum.wrapping_add(digest_prefix);
    }

    let input_bytes = u64::try_from(input.len())
        .expect("maximum input length fits u64")
        .checked_mul(u64::try_from(ITERATIONS).expect("iteration count fits u64"))
        .expect("bounded probe input total fits u64");
    eprintln!(
        "memory_v1_maximum_input_probe iterations={ITERATIONS} input_bytes={input_bytes} \
         canonical_bytes={canonical_bytes_total} digest_checksum={digest_checksum}"
    );
}
