#[test]
#[allow(clippy::too_many_lines)]
fn every_mutable_semantic_component_changes_the_digest() {
    let baseline = parse_strict_memory(COMMIT_YAML)
        .expect("baseline fixture must parse")
        .0;
    let baseline_digest = validated_digest(baseline.clone());

    assert_semantic_change(&baseline, baseline_digest, |record| {
        record.record_id = "mem_00041061050R3GG28A1C60T3GF".to_owned();
    });
    assert_semantic_change(&baseline, baseline_digest, |record| {
        record.parent_revision_digests.push("0".repeat(64));
    });
    assert_semantic_change(&baseline, baseline_digest, |record| {
        record.kind = MemoryKind::Failure;
    });
    assert_semantic_change(&baseline, baseline_digest, |record| {
        record.title.push('!');
    });
    assert_semantic_change(&baseline, baseline_digest, |record| {
        record.body.push('!');
    });
    assert_semantic_change(&baseline, baseline_digest, |record| {
        record.scope.repository_id =
            "rwi1:h:BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB".to_owned();
    });
    assert_semantic_change(&baseline, baseline_digest, |record| {
        record.provenance.actor_id = "reviewer".to_owned();
    });
    assert_semantic_change(&baseline, baseline_digest, |record| {
        record.lifecycle = Lifecycle::NeedsReview;
    });
    assert_semantic_change(&baseline, baseline_digest, |record| {
        let ValidityDto::Commits { introduced_by, .. } = &mut record.validity else {
            panic!("commit fixture must use commit validity");
        };
        introduced_by[0].object_id = "2".repeat(40);
    });
    assert_semantic_change(&baseline, baseline_digest, |record| {
        let ValidityDto::Commits { introduced_by, .. } = &mut record.validity else {
            panic!("commit fixture must use commit validity");
        };
        introduced_by[0].object_format = ObjectFormat::Sha256;
        introduced_by[0].object_id = "2".repeat(64);
    });
    assert_semantic_change(&baseline, baseline_digest, |record| {
        let ValidityDto::Commits { invalidated_by, .. } = &mut record.validity else {
            panic!("commit fixture must use commit validity");
        };
        invalidated_by.push(CommitIdDto {
            object_format: ObjectFormat::Sha1,
            object_id: "2".repeat(40),
        });
    });
    assert_semantic_change(&baseline, baseline_digest, |record| {
        record.evidence[0].source_snapshot_digest = "6".repeat(64);
    });
    assert_semantic_change(&baseline, baseline_digest, |record| {
        record.evidence[0].path = "rwp1:h:7372632F6D61696E2E7273".to_owned();
    });
    assert_semantic_change(&baseline, baseline_digest, |record| {
        record.evidence[0].content_digest = "7".repeat(64);
    });
    assert_semantic_change(&baseline, baseline_digest, |record| {
        record.evidence[0].artifact_digest = "8".repeat(64);
    });
    assert_semantic_change(&baseline, baseline_digest, |record| {
        record.evidence[0].fact_ordinal = 1;
    });
    assert_semantic_change(&baseline, baseline_digest, |record| {
        record.evidence[0].symbol_kind = SymbolKind::Method;
    });
    assert_semantic_change(&baseline, baseline_digest, |record| {
        record.evidence[0].name = "publish_now".to_owned();
        record.evidence[0].name_length = 11;
    });
    assert_semantic_change(&baseline, baseline_digest, |record| {
        record.evidence[0].qualified_name = "crate::atomic::publish".to_owned();
    });
    assert_semantic_change(&baseline, baseline_digest, |record| {
        record.evidence[0].name_start = 4;
    });
    assert_semantic_change(&baseline, baseline_digest, |record| {
        record.evidence[0].declaration_start = 1;
    });
    assert_semantic_change(&baseline, baseline_digest, |record| {
        record.evidence[0].declaration_length = 21;
    });
    assert_semantic_change(&baseline, baseline_digest, |record| {
        record.evidence[0].declaration_digest = "9".repeat(64);
    });
    assert_semantic_change(&baseline, baseline_digest, |record| {
        record.evidence[0].producer_id = "repowitness.rust.precise".to_owned();
    });
    assert_semantic_change(&baseline, baseline_digest, |record| {
        record.evidence[0].producer_version = "phase0-rust-syntax-v2".to_owned();
    });
    assert_semantic_change(&baseline, baseline_digest, |record| {
        record.relationships.push(RelationshipDto {
            kind: RelationshipKind::Supersedes,
            record_id: "mem_7ZZZZZZZZZZZZZZZZZZZZZZZZZ".to_owned(),
            revision_digest: "a".repeat(64),
        });
    });
    assert_semantic_change(&baseline, baseline_digest, |record| {
        record.validity = ValidityDto::Worktree {
            source_snapshot_digest: "a".repeat(64),
        };
    });
    assert_semantic_change(&baseline, baseline_digest, |record| {
        record.parent_revision_digests.push("a".repeat(64));
        record.lifecycle = Lifecycle::Tombstoned;
        record.tombstone = true;
    });

    let mut two_evidence = baseline.clone();
    two_evidence.evidence.push(two_evidence.evidence[0].clone());
    let two_evidence_digest = validated_digest(two_evidence.clone());
    assert_semantic_change(&two_evidence, two_evidence_digest, |record| {
        record.scope.subject_evidence = 1;
    });

    let record = validate_memory_record(baseline).expect("baseline remains valid");
    let canonical = String::from_utf8(canonical_bytes(&record).expect("baseline canonicalizes"))
        .expect("canonical JSON is UTF-8");
    for constant_semantic_field in [
        "\"schema_version\":1",
        "\"assurance\":\"locally_approved\"",
        "\"origin\":\"human\"",
        "\"actor_kind\":\"local_asserted\"",
        "\"kind\":\"rust_symbol\"",
    ] {
        assert!(canonical.contains(constant_semantic_field));
    }

    let relationship_baseline = parse_strict_memory(WORKTREE_YAML)
        .expect("relationship fixture must parse")
        .0;
    let relationship_digest = validated_digest(relationship_baseline.clone());
    assert_semantic_change(&relationship_baseline, relationship_digest, |record| {
        let ValidityDto::Worktree {
            source_snapshot_digest,
        } = &mut record.validity
        else {
            panic!("worktree fixture must use worktree validity");
        };
        *source_snapshot_digest = "d".repeat(64);
    });
    assert_semantic_change(&relationship_baseline, relationship_digest, |record| {
        record.relationships[0].kind = RelationshipKind::Supersedes;
    });
    assert_semantic_change(&relationship_baseline, relationship_digest, |record| {
        record.relationships[0].record_id = "mem_00041061050R3GG28A1C60T3GF".to_owned();
    });
    assert_semantic_change(&relationship_baseline, relationship_digest, |record| {
        record.relationships[0].revision_digest = "e".repeat(64);
    });
}

#[test]
fn every_record_id_byte_pattern_round_trips_deterministically() {
    let mut state = 0x6a09_e667_f3bc_c909_u64;
    for _ in 0..4_096 {
        let mut bytes = [0_u8; 16];
        for chunk in bytes.chunks_exact_mut(8) {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            chunk.copy_from_slice(&state.to_be_bytes());
        }
        let encoded = encode_record_id(bytes);
        assert_eq!(decode_record_id(&encoded), Some(bytes));
    }
}

#[test]
fn set_like_input_order_is_not_semantic_but_evidence_order_is() {
    let baseline = parse_strict_memory(COMMIT_YAML)
        .expect("baseline fixture must parse")
        .0;
    let first_relationship = RelationshipDto {
        kind: RelationshipKind::Contradicts,
        record_id: "mem_00041061050R3GG28A1C60T3GF".to_owned(),
        revision_digest: "a".repeat(64),
    };
    let second_relationship = RelationshipDto {
        kind: RelationshipKind::Supersedes,
        record_id: "mem_7ZZZZZZZZZZZZZZZZZZZZZZZZZ".to_owned(),
        revision_digest: "b".repeat(64),
    };
    let first_commit = CommitIdDto {
        object_format: ObjectFormat::Sha1,
        object_id: "2".repeat(40),
    };
    let second_commit = CommitIdDto {
        object_format: ObjectFormat::Sha256,
        object_id: "3".repeat(64),
    };

    let mut forward = baseline.clone();
    forward.parent_revision_digests = vec!["a".repeat(64), "b".repeat(64)];
    forward.relationships = vec![first_relationship.clone(), second_relationship.clone()];
    let ValidityDto::Commits { introduced_by, .. } = &mut forward.validity else {
        panic!("commit fixture must use commit validity");
    };
    introduced_by.extend([first_commit.clone(), second_commit.clone()]);

    let mut reverse = baseline;
    reverse.parent_revision_digests = vec!["b".repeat(64), "a".repeat(64)];
    reverse.relationships = vec![second_relationship, first_relationship];
    let ValidityDto::Commits { introduced_by, .. } = &mut reverse.validity else {
        panic!("commit fixture must use commit validity");
    };
    introduced_by.extend([second_commit, first_commit]);
    introduced_by.reverse();

    assert_eq!(validated_digest(forward.clone()), validated_digest(reverse));

    let mut evidence_reversed = forward.clone();
    let mut second_evidence = evidence_reversed.evidence[0].clone();
    second_evidence.fact_ordinal = 1;
    second_evidence.name = "release".to_owned();
    second_evidence.qualified_name = "crate::release".to_owned();
    evidence_reversed.evidence.push(second_evidence);
    let forward_evidence_digest = validated_digest(evidence_reversed.clone());
    evidence_reversed.evidence.reverse();
    assert_ne!(validated_digest(evidence_reversed), forward_evidence_digest);
}
