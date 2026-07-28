use super::{
    MAX_RUST_CORRESPONDENCE_CANDIDATES, RustAutomaticCorrespondence, RustCorrespondenceCandidate,
    RustCorrespondenceError, RustCorrespondenceIndeterminateReason, RustCorrespondenceResolution,
    RustCorrespondenceSubject, RustPathContinuity, fingerprint_rust_occurrence,
    resolve_rust_correspondence,
};
use crate::{RustAnalysisLimits, RustSymbolFact, RustSymbolKind};
use repowitness_domain::{
    AnalysisArtifactDigest, ByteOffset, ByteSpan, CorrespondenceFingerprintDigest,
    DeclarationDigest, RepositoryPath, RepositoryPathLimits,
};

fn path(value: &str) -> RepositoryPath {
    RepositoryPath::try_from_bytes(value.as_bytes(), RepositoryPathLimits::new(4_096, 256))
        .expect("path")
}

fn span(start: u64, length: u64) -> ByteSpan {
    ByteSpan::try_new(
        ByteOffset::new(start),
        ByteOffset::new(start.checked_add(length).expect("span end")),
    )
    .expect("span")
}

fn fact(kind: RustSymbolKind, name: &str, qualified_name: &str, source: &str) -> RustSymbolFact {
    let name_start = source.find(name).expect("name");
    RustSymbolFact::try_new(
        kind,
        name.to_owned(),
        qualified_name.to_owned(),
        span(
            u64::try_from(name_start).expect("offset"),
            u64::try_from(name.len()).expect("length"),
        ),
        span(0, u64::try_from(source.len()).expect("length")),
        RustAnalysisLimits::default(),
    )
    .expect("fact")
}

fn subject(
    old_path: &str,
    old_source: &str,
    old_name: &str,
    old_qualified_name: &str,
) -> RustCorrespondenceSubject {
    let old_fact = fact(
        RustSymbolKind::Function,
        old_name,
        old_qualified_name,
        old_source,
    );
    let fingerprint =
        fingerprint_rust_occurrence(old_source.as_bytes(), &old_fact).expect("fingerprint");
    RustCorrespondenceSubject::try_new(
        path(old_path),
        RustSymbolKind::Function,
        old_name.to_owned(),
        old_qualified_name.to_owned(),
        fingerprint.declaration(),
        Some(fingerprint.name_elided()),
    )
    .expect("subject")
}

fn candidate(
    candidate_path: &str,
    source: &str,
    name: &str,
    qualified_name: &str,
    ordinal: u64,
    continuity: RustPathContinuity,
) -> RustCorrespondenceCandidate {
    let fact = fact(RustSymbolKind::Function, name, qualified_name, source);
    let fingerprint = fingerprint_rust_occurrence(source.as_bytes(), &fact).expect("fingerprint");
    let fact = RustSymbolFact::try_new_with_correspondence(
        fact.kind(),
        fact.name().to_owned(),
        fact.qualified_name().to_owned(),
        fact.name_span(),
        fact.declaration_span(),
        fingerprint,
        RustAnalysisLimits::default(),
    )
    .expect("correspondence fact");
    RustCorrespondenceCandidate::try_from_fact(
        path(candidate_path),
        AnalysisArtifactDigest::new([u8::try_from(ordinal + 1).expect("byte"); 32]),
        ordinal,
        &fact,
        continuity,
    )
    .expect("candidate")
}

#[test]
fn fingerprint_has_a_stable_vector_and_elides_only_the_declared_name() {
    let first_source = "fn publish(value: u8) -> u8 { value + 1 }";
    let renamed_source = "fn send(value: u8) -> u8 { value + 1 }";
    let first_fact = fact(
        RustSymbolKind::Function,
        "publish",
        "crate::publish",
        first_source,
    );
    let renamed_fact = fact(
        RustSymbolKind::Function,
        "send",
        "crate::send",
        renamed_source,
    );
    let first = fingerprint_rust_occurrence(first_source.as_bytes(), &first_fact).expect("first");
    let renamed =
        fingerprint_rust_occurrence(renamed_source.as_bytes(), &renamed_fact).expect("renamed");

    assert_eq!(
        first.declaration().into_bytes(),
        [
            0x24, 0x0F, 0xFE, 0x2A, 0xF8, 0x4E, 0xB8, 0x1C, 0x74, 0x8E, 0xE1, 0xDF, 0xF4, 0xCF,
            0xF1, 0x18, 0x37, 0x6C, 0x2E, 0xF5, 0xFA, 0x1A, 0xEB, 0xA9, 0x2E, 0x82, 0xF5, 0xAC,
            0x77, 0x35, 0x9E, 0x64,
        ]
    );
    assert_eq!(
        first.name_elided().into_bytes(),
        [
            0xED, 0xA0, 0x48, 0x19, 0xEC, 0x30, 0x80, 0x94, 0x2B, 0xAF, 0x37, 0x37, 0x95, 0x46,
            0x86, 0x13, 0x72, 0x52, 0x76, 0x6C, 0x01, 0x81, 0xD5, 0x68, 0x6A, 0xF8, 0xDA, 0xF8,
            0xD3, 0xF3, 0x6A, 0x69,
        ]
    );
    assert_eq!(first.name_elided(), renamed.name_elided());
    assert_ne!(first.declaration(), renamed.declaration());

    let body_changed = "fn publish(value: u8) -> u8 { value + 2 }";
    let body_fact = fact(
        RustSymbolKind::Function,
        "publish",
        "crate::publish",
        body_changed,
    );
    assert_ne!(
        first.name_elided(),
        fingerprint_rust_occurrence(body_changed.as_bytes(), &body_fact)
            .expect("body")
            .name_elided()
    );

    let other_container = fact(
        RustSymbolKind::Function,
        "publish",
        "other::publish",
        first_source,
    );
    assert_ne!(
        first.name_elided(),
        fingerprint_rust_occurrence(first_source.as_bytes(), &other_container)
            .expect("container")
            .name_elided()
    );
}

#[test]
fn unchanged_name_only_rename_and_exact_git_move_are_categorical() {
    let old_source = "fn publish(value: u8) -> u8 { value + 1 }";
    let old = subject("src/lib.rs", old_source, "publish", "crate::publish");

    let exact = candidate(
        "src/lib.rs",
        old_source,
        "publish",
        "crate::publish",
        0,
        RustPathContinuity::None,
    );
    assert!(matches!(
        resolve_rust_correspondence(&old, &[exact], 1).expect("exact"),
        RustCorrespondenceResolution::Exact { .. }
    ));

    let renamed = candidate(
        "src/lib.rs",
        "fn send(value: u8) -> u8 { value + 1 }",
        "send",
        "crate::send",
        0,
        RustPathContinuity::None,
    );
    assert!(matches!(
        resolve_rust_correspondence(&old, &[renamed], 1).expect("rename"),
        RustCorrespondenceResolution::Automatic {
            relationship: RustAutomaticCorrespondence::Renamed,
            ..
        }
    ));

    let moved = candidate(
        "src/moved.rs",
        old_source,
        "publish",
        "crate::publish",
        0,
        RustPathContinuity::GitExactMove,
    );
    assert!(matches!(
        resolve_rust_correspondence(&old, &[moved], 1).expect("move"),
        RustCorrespondenceResolution::Automatic {
            relationship: RustAutomaticCorrespondence::Moved,
            ..
        }
    ));
}

#[test]
fn semantic_change_is_stale_evidence_not_correspondence() {
    let old = subject(
        "src/lib.rs",
        "fn publish() -> bool { true }",
        "publish",
        "crate::publish",
    );
    let changed = candidate(
        "src/lib.rs",
        "fn publish() -> bool { false }",
        "publish",
        "crate::publish",
        0,
        RustPathContinuity::None,
    );

    assert!(matches!(
        resolve_rust_correspondence(&old, &[changed], 1).expect("changed"),
        RustCorrespondenceResolution::Changed { .. }
    ));
}

#[test]
fn copies_duplicates_and_move_plus_rename_require_review() {
    let old_source = "fn publish() -> bool { true }";
    let old = subject("src/lib.rs", old_source, "publish", "crate::publish");
    let first = candidate(
        "src/lib.rs",
        "fn send() -> bool { true }",
        "send",
        "crate::send",
        0,
        RustPathContinuity::None,
    );
    let second = candidate(
        "src/lib.rs",
        "fn emit() -> bool { true }",
        "emit",
        "crate::emit",
        1,
        RustPathContinuity::None,
    );
    assert!(matches!(
        resolve_rust_correspondence(&old, &[first, second], 2).expect("ambiguous"),
        RustCorrespondenceResolution::NeedsReview { candidates }
            if candidates.len() == 2
    ));

    let moved_and_renamed = candidate(
        "src/moved.rs",
        "fn send() -> bool { true }",
        "send",
        "crate::send",
        0,
        RustPathContinuity::GitExactMove,
    );
    assert!(matches!(
        resolve_rust_correspondence(&old, &[moved_and_renamed], 1)
            .expect("move and rename"),
        RustCorrespondenceResolution::NeedsReview { candidates }
            if candidates.len() == 1
    ));
}

#[test]
fn incomplete_candidates_or_historical_source_are_indeterminate() {
    let old = subject(
        "src/lib.rs",
        "fn publish() -> bool { true }",
        "publish",
        "crate::publish",
    );
    assert_eq!(
        resolve_rust_correspondence(&old, &[], 17),
        Ok(RustCorrespondenceResolution::Indeterminate {
            reason: RustCorrespondenceIndeterminateReason::CandidateOverflow,
        })
    );

    let without_fingerprint = RustCorrespondenceSubject::try_new(
        path("src/lib.rs"),
        RustSymbolKind::Function,
        "publish".to_owned(),
        "crate::publish".to_owned(),
        DeclarationDigest::new([0x11; 32]),
        None,
    )
    .expect("subject");
    assert_eq!(
        resolve_rust_correspondence(&without_fingerprint, &[], 0),
        Ok(RustCorrespondenceResolution::Indeterminate {
            reason: RustCorrespondenceIndeterminateReason::MissingSubjectFingerprint,
        })
    );
}

#[test]
fn complete_absence_is_missing_and_invalid_candidate_sets_fail_closed() {
    let old = subject(
        "src/lib.rs",
        "fn publish() -> bool { true }",
        "publish",
        "crate::publish",
    );
    assert_eq!(
        resolve_rust_correspondence(&old, &[], 0),
        Ok(RustCorrespondenceResolution::Missing)
    );

    let exact_candidate = candidate(
        "src/lib.rs",
        "fn publish() -> bool { true }",
        "publish",
        "crate::publish",
        0,
        RustPathContinuity::None,
    );
    assert_eq!(
        resolve_rust_correspondence(&old, std::slice::from_ref(&exact_candidate), 0),
        Err(RustCorrespondenceError::InvalidCandidateCount)
    );
    assert_eq!(
        resolve_rust_correspondence(&old, &[exact_candidate.clone(), exact_candidate], 2,),
        Err(RustCorrespondenceError::DuplicateCandidate)
    );

    let over_limit = (0..=MAX_RUST_CORRESPONDENCE_CANDIDATES)
        .map(|ordinal| {
            candidate(
                &format!("src/{ordinal}.rs"),
                "fn unrelated() {}",
                "unrelated",
                "crate::unrelated",
                u64::try_from(ordinal).expect("ordinal"),
                RustPathContinuity::None,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        resolve_rust_correspondence(
            &old,
            &over_limit,
            u64::try_from(over_limit.len()).expect("count"),
        ),
        Err(RustCorrespondenceError::CandidateLimitExceeded)
    );
}

#[test]
fn malformed_spans_and_debug_output_do_not_expose_source_or_names() {
    let source = "fn publish() {}";
    let malformed = RustSymbolFact::try_new(
        RustSymbolKind::Function,
        "publish".to_owned(),
        "crate::publish".to_owned(),
        span(3, 7),
        span(0, 2),
        RustAnalysisLimits::default(),
    );
    assert!(malformed.is_err());

    let old = subject("src/private.rs", source, "publish", "crate::publish");
    let current = candidate(
        "src/private.rs",
        source,
        "publish",
        "crate::publish",
        0,
        RustPathContinuity::None,
    );
    for debug in [format!("{old:?}"), format!("{current:?}")] {
        assert!(!debug.contains("private"));
        assert!(!debug.contains("publish"));
        assert!(!debug.contains("crate"));
    }

    assert!(
        RustCorrespondenceSubject::try_new(
            path("src/lib.rs"),
            RustSymbolKind::Function,
            "name".to_owned(),
            "not_the_terminal_symbol".to_owned(),
            DeclarationDigest::new([0x11; 32]),
            Some(CorrespondenceFingerprintDigest::new([0x22; 32])),
        )
        .is_err()
    );
}
