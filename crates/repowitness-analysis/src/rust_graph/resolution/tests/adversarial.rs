use std::{sync::atomic::AtomicBool, time::Instant};

use repowitness_domain::{
    AnalysisArtifactDigest, MAX_MEMORY_INTEROPERABLE_INTEGER, RepositoryPath, RepositoryPathLimits,
    SourceSlotId,
};

use super::{bundle, control, definition, repository_path, resolve, site};
use crate::RustGraphSiteKind;
use crate::rust_graph::resolution::{
    RustGraphDefinitionOccurrence, RustGraphResolutionControl, RustGraphResolutionError,
    RustGraphResolutionLimits, RustGraphResolutionOutcome, RustGraphSiteOccurrence,
    RustGraphUnresolvedReason, resolve_rust_graph_sites,
};

fn limits(
    definitions: u32,
    sites: u32,
    candidates_per_site: u32,
    total_candidates: u64,
    input_text_bytes: u64,
    output_bytes: u64,
) -> RustGraphResolutionLimits {
    RustGraphResolutionLimits::try_new(
        definitions,
        sites,
        candidates_per_site,
        total_candidates,
        input_text_bytes,
        output_bytes,
    )
    .expect("test limits must be valid")
}

fn resolve_with(
    definitions: &[RustGraphDefinitionOccurrence],
    sites: &[RustGraphSiteOccurrence],
    limits: RustGraphResolutionLimits,
) -> Result<crate::RustGraphResolution, RustGraphResolutionError> {
    let cancelled = AtomicBool::new(false);
    resolve_rust_graph_sites(definitions, sites, limits, control(&cancelled))
}

fn duplicate_definitions(count: u8) -> Vec<RustGraphDefinitionOccurrence> {
    (0..count)
        .map(|index| {
            let fixture = bundle(
                b"fn duplicate() {}",
                &format!("definition-{index}.rs"),
                index.wrapping_add(1),
                index.wrapping_add(1),
            );
            definition(&fixture.definitions, "duplicate").clone()
        })
        .collect()
}

fn duplicate_call() -> RustGraphSiteOccurrence {
    let fixture = bundle(b"fn caller() { duplicate(); }", "caller.rs", 100, 100);
    site(&fixture.sites, RustGraphSiteKind::Call, "duplicate")
}

#[test]
fn invalid_limits_fail_closed() {
    assert_eq!(
        RustGraphResolutionLimits::try_new(0, 1, 2, 2, 1, 1),
        Err(RustGraphResolutionError::InvalidLimits)
    );
    assert_eq!(
        RustGraphResolutionLimits::try_new(1, 0, 2, 2, 1, 1),
        Err(RustGraphResolutionError::InvalidLimits)
    );
    assert_eq!(
        RustGraphResolutionLimits::try_new(1, 1, 1, 2, 1, 1),
        Err(RustGraphResolutionError::InvalidLimits)
    );
    assert_eq!(
        RustGraphResolutionLimits::try_new(1, 1, 2, 0, 1, 1),
        Err(RustGraphResolutionError::InvalidLimits)
    );
    assert_eq!(
        RustGraphResolutionLimits::try_new(1, 1, 2, 2, 0, 1),
        Err(RustGraphResolutionError::InvalidLimits)
    );
    assert_eq!(
        RustGraphResolutionLimits::try_new(1, 1, 2, 2, 1, 0),
        Err(RustGraphResolutionError::InvalidLimits)
    );
}

#[test]
fn definition_and_site_bounds_are_inclusive_then_fail_one_over() {
    let fixture = bundle(
        b"fn target() {} fn caller() { target(); target(); }",
        "bounds.rs",
        1,
        1,
    );
    let calls = fixture
        .sites
        .iter()
        .filter(|site| {
            site.site().kind() == RustGraphSiteKind::Call && site.site().raw_target() == "target"
        })
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(fixture.definitions.len(), 2);
    assert_eq!(calls.len(), 2);
    let exact = limits(2, 2, 2, 4, 1_000, 10_000);

    resolve_with(&fixture.definitions, &calls, exact).expect("inclusive bounds must pass");
    assert_eq!(
        resolve_with(
            &fixture.definitions,
            &calls,
            limits(1, 2, 2, 4, 1_000, 10_000)
        ),
        Err(RustGraphResolutionError::DefinitionLimitExceeded)
    );
    assert_eq!(
        resolve_with(
            &fixture.definitions,
            &calls,
            limits(2, 1, 2, 4, 1_000, 10_000)
        ),
        Err(RustGraphResolutionError::SiteLimitExceeded)
    );
}

#[test]
fn text_and_output_bounds_are_inclusive_then_fail_one_over() {
    let fixture = bundle(
        b"fn target() {} fn caller() { target(); }",
        "accounting.rs",
        1,
        1,
    );
    let call = site(&fixture.sites, RustGraphSiteKind::Call, "target");
    let baseline = resolve(&fixture.definitions, std::slice::from_ref(&call));
    let text = baseline.input_text_bytes();
    let output = baseline.output_bytes();

    resolve_with(
        &fixture.definitions,
        std::slice::from_ref(&call),
        limits(2, 1, 2, 2, text, output),
    )
    .expect("exact text and output bounds must pass");
    assert_eq!(
        resolve_with(
            &fixture.definitions,
            std::slice::from_ref(&call),
            limits(2, 1, 2, 2, text - 1, output)
        ),
        Err(RustGraphResolutionError::InputTextLimitExceeded)
    );
    assert_eq!(
        resolve_with(
            &fixture.definitions,
            &[call],
            limits(2, 1, 2, 2, text, output - 1)
        ),
        Err(RustGraphResolutionError::OutputLimitExceeded)
    );
}

#[test]
fn empty_output_still_enforces_its_fixed_accounting_bound() {
    let cancelled = AtomicBool::new(false);
    let exact = resolve_rust_graph_sites(&[], &[], limits(1, 1, 2, 1, 1, 64), control(&cancelled))
        .expect("fixed output budget is inclusive");
    assert_eq!(exact.output_bytes(), 64);
    assert_eq!(
        resolve_rust_graph_sites(&[], &[], limits(1, 1, 2, 1, 1, 63), control(&cancelled)),
        Err(RustGraphResolutionError::OutputLimitExceeded)
    );
}

#[test]
fn per_site_candidates_are_deterministically_truncated_only_after_the_bound() {
    let definitions = duplicate_definitions(4);
    let call = duplicate_call();
    let exact = resolve_with(
        &definitions[..3],
        std::slice::from_ref(&call),
        limits(3, 1, 3, 3, 10_000, 10_000),
    )
    .expect("three candidates at the limit must pass");
    assert!(matches!(
        exact.outcomes()[0].outcome(),
        RustGraphResolutionOutcome::Ambiguous { candidates } if candidates.len() == 3
    ));
    assert_eq!(exact.outcomes()[0].candidate_count(), 3);
    assert!(!exact.outcomes()[0].candidates_truncated());

    let truncated = resolve_with(&definitions, &[call], limits(4, 1, 3, 3, 10_000, 10_000))
        .expect("one-over per-site candidates must be explicit truncation");
    assert!(matches!(
        truncated.outcomes()[0].outcome(),
        RustGraphResolutionOutcome::Ambiguous { candidates } if candidates.len() == 3
    ));
    assert_eq!(truncated.outcomes()[0].candidate_count(), 4);
    assert!(truncated.outcomes()[0].candidates_truncated());
    assert_eq!(truncated.coverage().truncated_sites(), 1);
}

#[test]
fn aggregate_candidate_bound_is_inclusive_and_returns_no_partial_result() {
    let definitions = duplicate_definitions(3);
    let call = duplicate_call();
    resolve_with(
        &definitions,
        std::slice::from_ref(&call),
        limits(3, 1, 3, 3, 10_000, 10_000),
    )
    .expect("aggregate candidate bound is inclusive");
    assert_eq!(
        resolve_with(&definitions, &[call], limits(3, 1, 3, 2, 10_000, 10_000)),
        Err(RustGraphResolutionError::CandidateLimitExceeded)
    );
}

#[test]
fn cancellation_and_deadline_return_no_output() {
    let fixture = bundle(b"fn caller() { target(); }", "control.rs", 1, 1);
    let cancelled = AtomicBool::new(true);
    assert_eq!(
        resolve_rust_graph_sites(
            &fixture.definitions,
            &fixture.sites,
            RustGraphResolutionLimits::DEFAULT,
            RustGraphResolutionControl::new(&cancelled, super::deadline())
        ),
        Err(RustGraphResolutionError::Cancelled)
    );

    let active = AtomicBool::new(false);
    assert_eq!(
        resolve_rust_graph_sites(
            &fixture.definitions,
            &fixture.sites,
            RustGraphResolutionLimits::DEFAULT,
            RustGraphResolutionControl::new(&active, Instant::now())
        ),
        Err(RustGraphResolutionError::DeadlineExceeded)
    );
}

#[test]
fn duplicate_exact_occurrences_are_rejected_before_resolution() {
    let fixture = bundle(
        b"fn target() {} fn caller() { target(); }",
        "duplicates.rs",
        1,
        1,
    );
    let target = definition(&fixture.definitions, "target").clone();
    let call = site(&fixture.sites, RustGraphSiteKind::Call, "target");
    let cancelled = AtomicBool::new(false);

    assert_eq!(
        resolve_rust_graph_sites(
            &[target.clone(), target],
            std::slice::from_ref(&call),
            RustGraphResolutionLimits::DEFAULT,
            control(&cancelled)
        ),
        Err(RustGraphResolutionError::DuplicateDefinition)
    );
    assert_eq!(
        resolve_rust_graph_sites(
            &fixture.definitions,
            &[call.clone(), call],
            RustGraphResolutionLimits::DEFAULT,
            control(&cancelled)
        ),
        Err(RustGraphResolutionError::DuplicateSite)
    );
}

#[test]
fn occurrence_boundaries_reject_non_rust_paths_and_oversized_ordinals() {
    let fixture = bundle(b"fn target() {}", "valid.rs", 1, 1);
    let fact = definition(&fixture.definitions, "target").fact().clone();
    let invalid_path =
        RepositoryPath::try_from_bytes(b"README.md", RepositoryPathLimits::new(128, 8))
            .expect("test path grammar is valid");

    assert_eq!(
        RustGraphDefinitionOccurrence::try_new(
            SourceSlotId::new([1; 32]),
            invalid_path.clone(),
            AnalysisArtifactDigest::new([1; 32]),
            0,
            fact.clone()
        ),
        Err(RustGraphResolutionError::InvalidOccurrence)
    );
    assert_eq!(
        RustGraphDefinitionOccurrence::try_new(
            SourceSlotId::new([1; 32]),
            repository_path("valid.rs"),
            AnalysisArtifactDigest::new([1; 32]),
            MAX_MEMORY_INTEROPERABLE_INTEGER + 1,
            fact
        ),
        Err(RustGraphResolutionError::InvalidOccurrence)
    );
    let site_fixture = bundle(b"fn caller() { missing(); }", "site.rs", 1, 1);
    let raw_site = site(&site_fixture.sites, RustGraphSiteKind::Call, "missing")
        .site()
        .clone();
    assert_eq!(
        RustGraphSiteOccurrence::try_new(
            SourceSlotId::new([1; 32]),
            invalid_path,
            AnalysisArtifactDigest::new([1; 32]),
            raw_site
        ),
        Err(RustGraphResolutionError::InvalidOccurrence)
    );
}

#[test]
fn malformed_source_and_unsupported_unicode_never_create_false_precision() {
    let malformed = bundle(b"fn broken() { missing( }", "malformed.rs", 1, 1);
    let malformed_result = resolve(&malformed.definitions, &malformed.sites);
    assert_eq!(
        malformed_result.coverage().sites(),
        u32::try_from(malformed.sites.len()).expect("fixture count must fit")
    );

    let unicode = bundle(
        "fn café() {} fn caller() { café(); }".as_bytes(),
        "unicode.rs",
        1,
        1,
    );
    if let Some(call) = unicode
        .sites
        .iter()
        .find(|site| site.site().kind() == RustGraphSiteKind::Call)
        .cloned()
    {
        let result = resolve(&unicode.definitions, &[call]);
        assert!(matches!(
            result.outcomes()[0].outcome(),
            RustGraphResolutionOutcome::Unresolved {
                reason: RustGraphUnresolvedReason::UnsupportedQualifiedSyntax
            }
        ));
    }
}
