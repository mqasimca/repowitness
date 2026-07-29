use std::{
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};

use repowitness_domain::{
    AnalysisArtifactDigest, RepositoryPath, RepositoryPathLimits, SourceSlotId,
};

use super::{
    RUST_GRAPH_RESOLVER_PROFILE_VERSION, RustGraphDefinitionOccurrence, RustGraphResolution,
    RustGraphResolutionControl, RustGraphResolutionEvidence, RustGraphResolutionLimits,
    RustGraphResolutionOutcome, RustGraphSiteOccurrence, RustGraphUnresolvedReason,
    resolve_rust_graph_sites,
};
use crate::{
    RustAnalysisControl, RustAnalysisLimits, RustGraphAnalysisControl, RustGraphAnalysisLimits,
    RustGraphSiteAnalyzer, RustGraphSiteKind, RustSourceAnalyzer,
};

mod adversarial;

struct Bundle {
    definitions: Vec<RustGraphDefinitionOccurrence>,
    sites: Vec<RustGraphSiteOccurrence>,
}

fn deadline() -> Instant {
    Instant::now()
        .checked_add(Duration::from_secs(10))
        .expect("short test deadline must fit")
}

fn control(cancelled: &AtomicBool) -> RustGraphResolutionControl<'_> {
    RustGraphResolutionControl::new(cancelled, deadline())
}

fn repository_path(value: &str) -> RepositoryPath {
    RepositoryPath::try_from_bytes(value.as_bytes(), RepositoryPathLimits::new(4_096, 64))
        .expect("test path must be valid")
}

fn bundle(source: &[u8], path: &str, slot: u8, artifact: u8) -> Bundle {
    let cancelled = AtomicBool::new(false);
    let source_analysis = RustSourceAnalyzer::new()
        .expect("Rust grammar must load")
        .analyze(
            source,
            RustAnalysisLimits::DEFAULT,
            RustAnalysisControl::new(&cancelled, deadline()),
        )
        .expect("declaration fixture must analyze");
    let site_analysis = RustGraphSiteAnalyzer::new()
        .expect("Rust grammar must load")
        .analyze(
            source,
            RustGraphAnalysisLimits::DEFAULT,
            RustGraphAnalysisControl::new(&cancelled, deadline()),
        )
        .expect("site fixture must analyze");
    let source_slot = SourceSlotId::new([slot; 32]);
    let path = repository_path(path);
    let definitions = source_analysis
        .facts()
        .iter()
        .enumerate()
        .map(|(ordinal, fact)| {
            RustGraphDefinitionOccurrence::try_new(
                source_slot,
                path.clone(),
                AnalysisArtifactDigest::new([artifact; 32]),
                u64::try_from(ordinal).expect("test ordinal must fit"),
                fact.clone(),
            )
            .expect("analyzer fact must form an occurrence")
        })
        .collect();
    let sites = site_analysis
        .sites()
        .iter()
        .cloned()
        .map(|site| {
            RustGraphSiteOccurrence::try_new(
                source_slot,
                path.clone(),
                AnalysisArtifactDigest::new([artifact.wrapping_add(128); 32]),
                site,
            )
            .expect("analyzer site must form an occurrence")
        })
        .collect();
    Bundle { definitions, sites }
}

fn definition<'a>(
    definitions: &'a [RustGraphDefinitionOccurrence],
    qualified_name: &str,
) -> &'a RustGraphDefinitionOccurrence {
    definitions
        .iter()
        .find(|definition| definition.fact().qualified_name() == qualified_name)
        .expect("fixture definition must exist")
}

fn site(
    sites: &[RustGraphSiteOccurrence],
    kind: RustGraphSiteKind,
    raw_target: &str,
) -> RustGraphSiteOccurrence {
    sites
        .iter()
        .find(|site| site.site().kind() == kind && site.site().raw_target() == raw_target)
        .cloned()
        .expect("fixture site must exist")
}

fn resolve(
    definitions: &[RustGraphDefinitionOccurrence],
    sites: &[RustGraphSiteOccurrence],
) -> RustGraphResolution {
    let cancelled = AtomicBool::new(false);
    resolve_rust_graph_sites(
        definitions,
        sites,
        RustGraphResolutionLimits::DEFAULT,
        control(&cancelled),
    )
    .expect("fixture must resolve")
}

fn sole_candidate(resolution: &RustGraphResolution) -> &super::RustGraphResolutionCandidate {
    let [outcome] = resolution.outcomes() else {
        panic!("expected one site outcome");
    };
    match outcome.outcome() {
        RustGraphResolutionOutcome::Unique { candidate } => candidate,
        other => panic!("expected unique outcome, got {other:?}"),
    }
}

#[test]
fn resolver_profile_and_evidence_spellings_are_explicit() {
    assert_eq!(RUST_GRAPH_RESOLVER_PROFILE_VERSION, 1);
    for (evidence, spelling) in [
        (
            RustGraphResolutionEvidence::QualifiedSyntax,
            "qualified_syntax",
        ),
        (RustGraphResolutionEvidence::LexicalSyntax, "lexical_syntax"),
        (RustGraphResolutionEvidence::ImportSyntax, "import_syntax"),
        (
            RustGraphResolutionEvidence::ExactNameHeuristic,
            "exact_name_heuristic",
        ),
    ] {
        assert_eq!(evidence.as_str(), spelling);
    }
    assert_eq!(
        RustGraphUnresolvedReason::DynamicOrMethodCall.as_str(),
        "dynamic_or_method_call"
    );
}

#[test]
fn zero_one_and_many_targets_remain_categorical() {
    let caller = bundle(
        b"fn caller() { target(); missing(); duplicate(); }",
        "caller.rs",
        9,
        9,
    );
    let target = bundle(b"fn target() {}", "target.rs", 9, 1);
    let duplicate_a = bundle(b"fn duplicate() {}", "a.rs", 2, 2);
    let duplicate_b = bundle(b"fn duplicate() {}", "b.rs", 3, 3);

    let target_call = site(&caller.sites, RustGraphSiteKind::Call, "target");
    let missing_call = site(&caller.sites, RustGraphSiteKind::Call, "missing");
    let duplicate_call = site(&caller.sites, RustGraphSiteKind::Call, "duplicate");
    let definitions = vec![
        definition(&target.definitions, "target").clone(),
        definition(&duplicate_a.definitions, "duplicate").clone(),
        definition(&duplicate_b.definitions, "duplicate").clone(),
    ];
    let result = resolve(&definitions, &[target_call, missing_call, duplicate_call]);

    assert_eq!(result.profile_version(), 1);
    assert_eq!(result.coverage().sites(), 3);
    assert_eq!(result.coverage().unique(), 1);
    assert_eq!(result.coverage().unresolved(), 1);
    assert_eq!(result.coverage().ambiguous(), 1);
    assert!(matches!(
        result.outcomes()[0].outcome(),
        RustGraphResolutionOutcome::Unique { candidate }
            if candidate.evidence() == RustGraphResolutionEvidence::LexicalSyntax
    ));
    assert!(matches!(
        result.outcomes()[1].outcome(),
        RustGraphResolutionOutcome::Unresolved {
            reason: RustGraphUnresolvedReason::NoCandidate
        }
    ));
    assert!(matches!(
        result.outcomes()[2].outcome(),
        RustGraphResolutionOutcome::Ambiguous { candidates }
            if candidates.len() == 2
                && candidates.iter().all(|candidate| {
                    candidate.evidence() == RustGraphResolutionEvidence::ExactNameHeuristic
                })
    ));
}

#[test]
fn nearest_lexical_declaration_shadows_the_outer_name() {
    let fixture = bundle(
        b"
fn item() {}
fn outer() {
    fn item() {}
    item();
}
",
        "shadow.rs",
        1,
        1,
    );
    let call = site(&fixture.sites, RustGraphSiteKind::Call, "item");
    let expected = definition(&fixture.definitions, "outer::item").identity();
    let result = resolve(&fixture.definitions, &[call]);
    let candidate = sole_candidate(&result);

    assert_eq!(candidate.target(), &expected);
    assert_eq!(
        candidate.evidence(),
        RustGraphResolutionEvidence::LexicalSyntax
    );
}

#[test]
fn duplicate_names_across_paths_and_slots_are_never_first_match_wins() {
    let site_bundle = bundle(b"fn caller() { duplicate(); }", "caller.rs", 9, 9);
    let first = bundle(b"fn duplicate() {}", "z.rs", 2, 2);
    let second = bundle(b"fn duplicate() {}", "a.rs", 2, 3);
    let third = bundle(b"fn duplicate() {}", "other.rs", 3, 4);
    let call = site(&site_bundle.sites, RustGraphSiteKind::Call, "duplicate");
    let definitions = vec![
        definition(&first.definitions, "duplicate").clone(),
        definition(&second.definitions, "duplicate").clone(),
        definition(&third.definitions, "duplicate").clone(),
    ];
    let mut reversed = definitions.clone();
    reversed.reverse();

    let forward = resolve(&definitions, std::slice::from_ref(&call));
    let backward = resolve(&reversed, &[call]);

    assert_eq!(forward, backward);
    assert!(matches!(
        forward.outcomes()[0].outcome(),
        RustGraphResolutionOutcome::Ambiguous { candidates } if candidates.len() == 3
    ));
}

#[test]
fn input_permutations_cannot_change_resolution_output() {
    let caller = bundle(
        b"fn caller() { target(); duplicate(); missing(); }",
        "caller.rs",
        9,
        9,
    );
    let target = bundle(b"fn target() {}", "target.rs", 9, 1);
    let duplicate_a = bundle(b"fn duplicate() {}", "a.rs", 2, 2);
    let duplicate_b = bundle(b"fn duplicate() {}", "b.rs", 3, 3);

    let mut definitions = caller.definitions;
    definitions.extend(target.definitions);
    definitions.extend(duplicate_a.definitions);
    definitions.extend(duplicate_b.definitions);
    let sites = caller.sites;
    let expected = resolve(&definitions, &sites);

    for definition_shift in 0..definitions.len() {
        for site_shift in 0..sites.len() {
            let mut permuted_definitions = definitions.clone();
            permuted_definitions.rotate_left(definition_shift);
            if definition_shift % 2 == 1 {
                permuted_definitions.reverse();
            }

            let mut permuted_sites = sites.clone();
            permuted_sites.rotate_left(site_shift);
            if site_shift % 2 == 1 {
                permuted_sites.reverse();
            }

            assert_eq!(
                resolve(&permuted_definitions, &permuted_sites),
                expected,
                "definition shift {definition_shift}, site shift {site_shift}"
            );
        }
    }
}

#[test]
fn exact_simple_imports_and_aliases_resolve_with_import_evidence() {
    let fixture = bundle(
        b"
mod support {
    pub struct Item;
    pub fn run() {}
}
use crate::support::Item as Alias;
use crate::support::run as execute;
fn caller(value: Alias) {
    execute();
    let _: Alias = value;
}
",
        "imports.rs",
        1,
        1,
    );
    let import = site(
        &fixture.sites,
        RustGraphSiteKind::Import,
        "crate::support::Item as Alias",
    );
    let function_import = site(
        &fixture.sites,
        RustGraphSiteKind::Import,
        "crate::support::run as execute",
    );
    let alias = site(&fixture.sites, RustGraphSiteKind::Reference, "Alias");
    let call = site(&fixture.sites, RustGraphSiteKind::Call, "execute");
    let result = resolve(
        &fixture.definitions,
        &[import, function_import, alias, call],
    );

    assert_eq!(result.coverage().unique(), 4, "{result:?}");
    for outcome in result.outcomes() {
        assert!(matches!(
            outcome.outcome(),
            RustGraphResolutionOutcome::Unique { candidate }
                if candidate.evidence() == RustGraphResolutionEvidence::ImportSyntax
        ));
    }
}

#[test]
fn imports_with_the_same_alias_do_not_leak_across_module_scopes() {
    let fixture = bundle(
        b"
mod first {
    pub struct One;
    use crate::first::One as Alias;
    fn consume(_: Alias) {}
}
mod second {
    pub struct Two;
    use crate::second::Two as Alias;
    fn consume(_: Alias) {}
}
",
        "scoped-imports.rs",
        1,
        1,
    );
    let imports = fixture
        .sites
        .iter()
        .filter(|site| site.site().kind() == RustGraphSiteKind::Import)
        .cloned();
    let first_alias = fixture
        .sites
        .iter()
        .find(|site| {
            site.site().kind() == RustGraphSiteKind::Reference
                && site.site().raw_target() == "Alias"
                && site
                    .site()
                    .enclosing_definition()
                    .is_some_and(|enclosing| enclosing.qualified_name() == "first::consume")
        })
        .cloned()
        .expect("first module alias reference must exist");
    let selected = imports.chain([first_alias]).collect::<Vec<_>>();
    let expected = definition(&fixture.definitions, "first::One").identity();
    let result = resolve(&fixture.definitions, &selected);
    let alias_outcome = result
        .outcomes()
        .iter()
        .find(|outcome| outcome.site().kind() == RustGraphSiteKind::Reference)
        .expect("reference outcome must exist");

    assert!(matches!(
        alias_outcome.outcome(),
        RustGraphResolutionOutcome::Unique { candidate }
            if candidate.target() == &expected
                && candidate.evidence() == RustGraphResolutionEvidence::ImportSyntax
    ));
}

#[test]
fn qualified_free_call_with_terminal_turbofish_is_supported() {
    let fixture = bundle(
        b"
mod service {
    pub fn run<T>() {}
}
fn caller() {
    crate::service::run::<u8>();
}
",
        "qualified.rs",
        1,
        1,
    );
    let call = site(
        &fixture.sites,
        RustGraphSiteKind::Call,
        "crate::service::run::<u8>",
    );
    let expected = definition(&fixture.definitions, "service::run").identity();
    let result = resolve(&fixture.definitions, &[call]);
    let candidate = sole_candidate(&result);

    assert_eq!(candidate.target(), &expected);
    assert_eq!(
        candidate.evidence(),
        RustGraphResolutionEvidence::QualifiedSyntax
    );
}

#[test]
fn glob_relative_macro_test_and_dynamic_sites_abstain_explicitly() {
    let fixture = bundle(
        b"
mod support { pub fn run() {} }
use crate::support::*;
struct Worker;
impl Worker { fn run(&self) {} }
fn caller(worker: Worker) {
    worker.run();
    Worker::run(&worker);
    trace!(worker);
}
#[test]
fn verifies() {}
",
        "unsupported.rs",
        1,
        1,
    );
    let sites = vec![
        site(
            &fixture.sites,
            RustGraphSiteKind::Import,
            "crate::support::*",
        ),
        site(&fixture.sites, RustGraphSiteKind::Call, "worker.run"),
        site(&fixture.sites, RustGraphSiteKind::Call, "Worker::run"),
        site(&fixture.sites, RustGraphSiteKind::MacroCall, "trace"),
        site(&fixture.sites, RustGraphSiteKind::TestMarker, "test"),
    ];
    let result = resolve(&fixture.definitions, &sites);
    let reasons = result
        .outcomes()
        .iter()
        .map(|outcome| match outcome.outcome() {
            RustGraphResolutionOutcome::Unresolved { reason } => *reason,
            other => panic!("unsupported site resolved unexpectedly: {other:?}"),
        })
        .collect::<Vec<_>>();

    assert_eq!(
        reasons,
        vec![
            RustGraphUnresolvedReason::UnsupportedImportShape,
            RustGraphUnresolvedReason::DynamicOrMethodCall,
            RustGraphUnresolvedReason::DynamicOrMethodCall,
            RustGraphUnresolvedReason::UnsupportedSiteKind,
            RustGraphUnresolvedReason::UnsupportedSiteKind,
        ]
    );
    assert_eq!(result.coverage().unsupported(), 5);
}

#[test]
fn public_debug_output_redacts_paths_symbols_artifacts_and_deadlines() {
    let fixture = bundle(
        b"fn sensitive_target() {} fn caller() { sensitive_target(); }",
        "private/secret.rs",
        0xA5,
        0xC3,
    );
    let call = site(&fixture.sites, RustGraphSiteKind::Call, "sensitive_target");
    let result = resolve(&fixture.definitions, &[call]);
    let rendered = format!("{result:?}");

    assert!(!rendered.contains("sensitive_target"));
    assert!(!rendered.contains("private"));
    assert!(!rendered.contains("secret.rs"));
    assert!(!rendered.contains("A5"));
    assert!(!rendered.contains("C3"));

    let cancelled = AtomicBool::new(false);
    let rendered_control = format!("{:?}", control(&cancelled));
    assert!(rendered_control.contains("<monotonic>"));
}
