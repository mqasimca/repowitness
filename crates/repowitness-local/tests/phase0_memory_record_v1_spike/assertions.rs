fn assert_golden(
    yaml: &[u8],
    expected_yaml_sha256: &str,
    expected_canonical: &str,
    expected_digest: &str,
) {
    assert!(yaml.len() <= MAX_INPUT_BYTES);
    assert!(yaml.ends_with(b"\n"));
    assert!(!yaml.contains(&b'\r'));
    let yaml_digest: [u8; 32] = Sha256::digest(yaml).into();
    assert_eq!(hex(&yaml_digest), expected_yaml_sha256);
    let record = parse_strict_memory(yaml).expect("golden YAML must parse");
    assert_eq!(
        generated_yaml(&record).expect("golden record must generate"),
        yaml
    );
    let canonical = canonical_bytes(&record).expect("golden record must canonicalize");
    let canonical = str::from_utf8(&canonical).expect("canonical JSON is UTF-8");
    let digest = hex(canonical_digest(&record)
        .expect("golden record must hash")
        .as_bytes());
    let expected_canonical = expected_canonical.trim_end_matches('\n');
    let expected_digest = expected_digest.trim_end_matches('\n');
    assert_eq!(canonical, expected_canonical);
    assert_eq!(digest, expected_digest);
}

fn validated_digest(dto: MemoryRecordDto) -> CanonicalMemoryDigest {
    let record = validate_memory_record(dto).expect("mutated record must remain valid");
    canonical_digest(&record).expect("mutated record must hash")
}

fn assert_semantic_change(
    baseline: &MemoryRecordDto,
    baseline_digest: CanonicalMemoryDigest,
    mutate: impl FnOnce(&mut MemoryRecordDto),
) {
    let mut changed = baseline.clone();
    mutate(&mut changed);
    assert_ne!(validated_digest(changed), baseline_digest);
}
