use std::sync::atomic::AtomicBool;
use std::time::Duration;

use repowitness_domain::RepositoryPathLimits;

use super::*;

const PATH_LIMITS: RepositoryPathLimits = RepositoryPathLimits::new(1024, 32);

fn path(bytes: &[u8]) -> RepositoryPath {
    RepositoryPath::try_from_bytes(bytes, PATH_LIMITS)
        .expect("fixture repository path must be valid")
}

fn source(path_bytes: &[u8], content: &[u8]) -> ImmutableRustSource {
    ImmutableRustSource::new(path(path_bytes), content.to_vec().into_boxed_slice())
}

fn go_source(path_bytes: &[u8], content: &[u8]) -> ImmutableRustSource {
    ImmutableRustSource::new_go(path(path_bytes), content.to_vec().into_boxed_slice())
}

fn typescript_source(path_bytes: &[u8], content: &[u8]) -> ImmutableRustSource {
    ImmutableRustSource::new_typescript(path(path_bytes), content.to_vec().into_boxed_slice())
}

fn tsx_source(path_bytes: &[u8], content: &[u8]) -> ImmutableRustSource {
    ImmutableRustSource::new_tsx(path(path_bytes), content.to_vec().into_boxed_slice())
}

fn python_source(path_bytes: &[u8], content: &[u8]) -> ImmutableRustSource {
    ImmutableRustSource::new_python(path(path_bytes), content.to_vec().into_boxed_slice())
}

fn identity() -> RustArtifactIdentity {
    RustArtifactIdentity::new(
        ProducerManifestDigest::new([1; 32]),
        ConfigurationDigest::new([2; 32]),
        AnalysisSchemaDigest::new([3; 32]),
        1,
    )
}

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(1)
}

fn source_identities() -> SourceArtifactIdentities {
    SourceArtifactIdentities::new(
        identity(),
        RustArtifactIdentity::new(
            ProducerManifestDigest::new([4; 32]),
            ConfigurationDigest::new([5; 32]),
            AnalysisSchemaDigest::new([6; 32]),
            1,
        ),
        RustArtifactIdentity::new(
            ProducerManifestDigest::new([7; 32]),
            ConfigurationDigest::new([8; 32]),
            AnalysisSchemaDigest::new([9; 32]),
            1,
        ),
        RustArtifactIdentity::new(
            ProducerManifestDigest::new([10; 32]),
            ConfigurationDigest::new([11; 32]),
            AnalysisSchemaDigest::new([12; 32]),
            1,
        ),
        RustArtifactIdentity::new(
            ProducerManifestDigest::new([13; 32]),
            ConfigurationDigest::new([14; 32]),
            AnalysisSchemaDigest::new([15; 32]),
            1,
        ),
    )
}

#[test]
fn all_supported_languages_share_one_manifest_but_not_artifact_identity() {
    let cancelled = AtomicBool::new(false);
    let prepared = prepare_source_index(
        vec![
            go_source(b"cmd/main.go", b"package main\nfunc Execute() {}\n"),
            source(b"src/lib.rs", b"pub fn execute() {}\n"),
            typescript_source(b"web/api.ts", b"export function execute() {}\n"),
            tsx_source(
                b"web/view.tsx",
                b"export function View() { return <main />; }\n",
            ),
            python_source(b"sdk/client.py", b"class Client:\n    pass\n"),
        ],
        source_identities(),
        RustIndexLimits::default(),
        &cancelled,
        deadline(),
    )
    .expect("mixed supported inputs should prepare");

    assert_eq!(prepared.manifest().count().get(), 5);
    assert_eq!(prepared.indexed_go_files(), 1);
    assert_eq!(prepared.indexed_rust_files(), 1);
    assert_eq!(prepared.indexed_typescript_files(), 1);
    assert_eq!(prepared.indexed_tsx_files(), 1);
    assert_eq!(prepared.indexed_python_files(), 1);
    assert_eq!(prepared.analyzed_go_files(), 1);
    assert_eq!(prepared.analyzed_rust_files(), 1);
    assert_eq!(prepared.analyzed_typescript_files(), 1);
    assert_eq!(prepared.analyzed_tsx_files(), 1);
    assert_eq!(prepared.analyzed_python_files(), 1);
    assert_eq!(prepared.reused_files(), 0);
    assert_eq!(prepared.total_known_parser_limitation_nodes(), 0);
    assert_eq!(
        prepared
            .files()
            .iter()
            .map(|file| (file.path().as_bytes(), file.language()))
            .collect::<Vec<_>>(),
        [
            (b"cmd/main.go".as_slice(), SourceLanguage::Go),
            (b"sdk/client.py".as_slice(), SourceLanguage::Python),
            (b"src/lib.rs".as_slice(), SourceLanguage::Rust),
            (b"web/api.ts".as_slice(), SourceLanguage::TypeScript),
            (b"web/view.tsx".as_slice(), SourceLanguage::Tsx),
        ]
    );
    assert!(
        prepared
            .files()
            .iter()
            .all(|file| file.artifact_identity()
                == source_identities().for_language(file.language()))
    );
}

#[test]
fn known_parser_limitations_are_aggregated_without_subtracting_raw_errors() {
    let cancelled = AtomicBool::new(false);
    let synthetic_classified = b"export const statement = true;";
    let sources = || {
        vec![
            typescript_source(b"web/classified.ts", synthetic_classified),
            tsx_source(b"web/classified.tsx", synthetic_classified),
            typescript_source(b"web/malformed.ts", b"export interface Broken { value:"),
        ]
    };
    let identities = source_identities();
    let fixture_sources = sources();
    let classified =
        RustSourceAnalysis::try_from_parts(Vec::new(), 1, 1, 1, RustAnalysisLimits::DEFAULT)
            .expect("classified persisted analysis should be valid");
    let reusable = fixture_sources
        .iter()
        .filter(|source| source.path().as_bytes() != b"web/malformed.ts")
        .map(|source| {
            let identity = identities.for_language(source.language());
            let key = AnalysisArtifactKey::new(
                hash_source_content(source.content()),
                identity.producer_manifest(),
                identity.configuration(),
                identity.schema(),
                identity.canonicalization_version(),
            );
            (hash_analysis_artifact_key(&key), classified.clone())
        })
        .collect::<BTreeMap<_, _>>();
    let prepared = prepare_source_index_with_reuse(
        fixture_sources,
        identities,
        RustIndexLimits::default(),
        &reusable,
        &cancelled,
        deadline(),
    )
    .expect("known and unknown syntax errors should remain explicit");

    assert_eq!(prepared.reused_files(), 2);
    assert_eq!(prepared.analyzed_files(), 1);
    assert_eq!(prepared.total_known_parser_limitation_nodes(), 2);
    assert!(prepared.total_syntax_error_nodes() > 2);
    assert!(prepared.total_known_parser_limitation_nodes() <= prepared.total_syntax_error_nodes());
    for file in prepared.files() {
        assert!(
            file.analysis().known_parser_limitation_nodes() <= file.analysis().syntax_error_nodes()
        );
        if file.path().as_bytes() == b"web/malformed.ts" {
            assert_eq!(file.analysis().known_parser_limitation_nodes(), 0);
            assert!(file.analysis().syntax_error_nodes() > 0);
        } else {
            assert_eq!(file.analysis().known_parser_limitation_nodes(), 1);
            assert_eq!(file.analysis().syntax_error_nodes(), 1);
        }
    }

    let all_reusable = prepared
        .files()
        .iter()
        .map(|file| (file.artifact_digest(), file.analysis().clone()))
        .collect::<BTreeMap<_, _>>();
    let reused = prepare_source_index_with_reuse(
        sources(),
        source_identities(),
        RustIndexLimits::default(),
        &all_reusable,
        &cancelled,
        deadline(),
    )
    .expect("exact reuse should preserve parser coverage");

    assert_eq!(reused.reused_files(), 3);
    assert_eq!(
        reused.total_syntax_error_nodes(),
        prepared.total_syntax_error_nodes()
    );
    assert_eq!(
        reused.total_known_parser_limitation_nodes(),
        prepared.total_known_parser_limitation_nodes()
    );
}

#[test]
fn identical_bytes_cannot_cross_language_artifact_or_reuse_boundaries() {
    let cancelled = AtomicBool::new(false);
    let bytes = b"fn shared() {}\n";
    let clean = prepare_source_index(
        vec![
            go_source(b"same.go", bytes),
            source(b"same.rs", bytes),
            typescript_source(b"same.ts", bytes),
            tsx_source(b"same.tsx", bytes),
            python_source(b"same.py", bytes),
        ],
        source_identities(),
        RustIndexLimits::default(),
        &cancelled,
        deadline(),
    )
    .expect("mixed identical bytes should prepare independently");
    assert!(
        clean
            .files()
            .windows(2)
            .all(|pair| pair[0].content_digest() == pair[1].content_digest())
    );
    let artifact_digests = clean
        .files()
        .iter()
        .map(PreparedRustFile::artifact_digest)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(artifact_digests.len(), 5);

    let typescript = clean
        .files()
        .iter()
        .find(|file| file.language() == SourceLanguage::TypeScript)
        .expect("TypeScript fixture should exist");
    let only_typescript =
        BTreeMap::from([(typescript.artifact_digest(), typescript.analysis().clone())]);
    let incremental = prepare_source_index_with_reuse(
        vec![
            source(b"same.rs", bytes),
            go_source(b"same.go", bytes),
            tsx_source(b"same.tsx", bytes),
            typescript_source(b"same.ts", bytes),
            python_source(b"same.py", bytes),
        ],
        source_identities(),
        RustIndexLimits::default(),
        &only_typescript,
        &cancelled,
        deadline(),
    )
    .expect("only the exact language artifact should be reused");

    assert_eq!(incremental.reused_typescript_files(), 1);
    assert_eq!(incremental.analyzed_go_files(), 1);
    assert_eq!(incremental.analyzed_rust_files(), 1);
    assert_eq!(incremental.analyzed_tsx_files(), 1);
    assert_eq!(incremental.analyzed_python_files(), 1);
    assert_eq!(incremental.reused_go_files(), 0);
    assert_eq!(incremental.reused_rust_files(), 0);
    assert_eq!(incremental.reused_tsx_files(), 0);
    assert_eq!(incremental.reused_python_files(), 0);
    assert_eq!(incremental.analyzed_typescript_files(), 0);
}

#[test]
fn rust_only_entry_points_reject_go_inputs() {
    let cancelled = AtomicBool::new(false);
    let error = prepare_rust_index(
        vec![go_source(b"main.go", b"package main\n")],
        identity(),
        RustIndexLimits::default(),
        &cancelled,
        deadline(),
    )
    .expect_err("Rust-only compatibility API must not share its identity with Go");

    assert!(matches!(
        error,
        RustIndexPreparationError::UnexpectedLanguage
    ));
}

#[test]
fn source_language_must_match_the_exact_repository_extension() {
    let cancelled = AtomicBool::new(false);
    for mismatched in [
        go_source(b"wrong.rs", b"package wrong\n"),
        source(b"wrong.go", b"fn wrong() {}\n"),
        source(b"upper.RS", b"fn wrong() {}\n"),
        typescript_source(b"wrong.tsx", b"export const wrong = 1;\n"),
        tsx_source(b"wrong.ts", b"export const wrong = <main />;\n"),
        typescript_source(b"upper.TS", b"export const wrong = 1;\n"),
        python_source(b"wrong.rs", b"def wrong():\n    pass\n"),
        python_source(b"upper.PY", b"def wrong():\n    pass\n"),
    ] {
        let error = prepare_source_index(
            vec![mismatched],
            source_identities(),
            RustIndexLimits::default(),
            &cancelled,
            deadline(),
        )
        .expect_err("language/path disagreement must fail before snapshot construction");
        assert!(matches!(
            error,
            RustIndexPreparationError::LanguagePathMismatch
        ));
    }
}

#[test]
fn selected_languages_require_distinct_artifact_identities() {
    let cancelled = AtomicBool::new(false);
    let shared = identity();
    let identities = SourceArtifactIdentities::new(shared, shared, shared, shared, shared);
    let error = prepare_source_index(
        vec![
            source(b"same.rs", b"fn shared() {}\n"),
            go_source(b"same.go", b"fn shared() {}\n"),
        ],
        identities,
        RustIndexLimits::default(),
        &cancelled,
        deadline(),
    )
    .expect_err("two selected languages must not share artifact identity");

    assert!(matches!(
        error,
        RustIndexPreparationError::LanguageArtifactIdentityCollision
    ));
}

#[test]
fn unordered_inputs_produce_one_canonical_complete_index() {
    let cancelled = AtomicBool::new(false);
    let prepared = prepare_rust_index(
        vec![
            source(b"src/b.rs", b"fn b() {}\n"),
            source(b"src/a.rs", b"struct A;\n"),
        ],
        identity(),
        RustIndexLimits::default(),
        &cancelled,
        deadline(),
    )
    .expect("valid immutable Rust inputs must prepare");

    assert_eq!(
        prepared
            .files()
            .iter()
            .map(|file| file.path().as_bytes())
            .collect::<Vec<_>>(),
        [b"src/a.rs".as_slice(), b"src/b.rs".as_slice()]
    );
    assert_eq!(prepared.manifest().count().get(), 2);
    assert_eq!(prepared.total_source_bytes(), 20);
    assert_eq!(prepared.total_facts(), 2);
    assert_eq!(prepared.total_syntax_error_nodes(), 0);
    assert_eq!(prepared.total_known_parser_limitation_nodes(), 0);
    assert_eq!(
        prepared.manifest_digest(),
        hash_source_manifest(prepared.manifest())
    );
    assert!(prepared.files().iter().all(|file| file.artifact_digest()
        == hash_analysis_artifact_key(&AnalysisArtifactKey::new(
            file.content_digest(),
            identity().producer_manifest(),
            identity().configuration(),
            identity().schema(),
            identity().canonicalization_version(),
        ))));
}

#[test]
fn input_order_does_not_change_observable_output() {
    let cancelled = AtomicBool::new(false);
    let forward = prepare_rust_index(
        vec![
            source(b"a.rs", b"fn a() {}\n"),
            source(b"b.rs", b"fn b() {}\n"),
        ],
        identity(),
        RustIndexLimits::default(),
        &cancelled,
        deadline(),
    )
    .expect("forward input must prepare");
    let reverse = prepare_rust_index(
        vec![
            source(b"b.rs", b"fn b() {}\n"),
            source(b"a.rs", b"fn a() {}\n"),
        ],
        identity(),
        RustIndexLimits::default(),
        &cancelled,
        deadline(),
    )
    .expect("reverse input must prepare");

    assert_eq!(forward, reverse);
}

#[test]
fn exact_reuse_matches_clean_output_and_semantic_changes_analyze_only_affected_files() {
    let cancelled = AtomicBool::new(false);
    let clean = prepare_rust_index(
        vec![
            source(b"a.rs", b"fn alpha() {}\n"),
            source(b"b.rs", b"struct Beta;\n"),
        ],
        identity(),
        RustIndexLimits::default(),
        &cancelled,
        deadline(),
    )
    .expect("clean preparation must succeed");
    assert_eq!(clean.reused_files(), 0);
    assert_eq!(clean.analyzed_files(), 2);
    let reusable = clean
        .files()
        .iter()
        .map(|file| (file.artifact_digest(), file.analysis().clone()))
        .collect::<BTreeMap<_, _>>();

    let incremental = prepare_rust_index_with_reuse(
        vec![
            source(b"b.rs", b"struct Beta;\n"),
            source(b"a.rs", b"fn alpha() {}\n"),
        ],
        identity(),
        RustIndexLimits::default(),
        &reusable,
        &cancelled,
        deadline(),
    )
    .expect("exact reusable artifacts must prepare");
    assert_eq!(incremental.manifest(), clean.manifest());
    assert_eq!(incremental.files(), clean.files());
    assert_eq!(incremental.total_facts(), clean.total_facts());
    assert_eq!(
        incremental.total_known_parser_limitation_nodes(),
        clean.total_known_parser_limitation_nodes()
    );
    assert_eq!(incremental.reused_files(), 2);
    assert_eq!(incremental.analyzed_files(), 0);

    let changed = prepare_rust_index_with_reuse(
        vec![
            source(b"a.rs", b"fn alpha() {}\n"),
            source(b"b.rs", b"struct Changed;\n"),
        ],
        identity(),
        RustIndexLimits::default(),
        &reusable,
        &cancelled,
        deadline(),
    )
    .expect("one changed input must prepare");
    assert_eq!(changed.reused_files(), 1);
    assert_eq!(changed.analyzed_files(), 1);

    let changed_identity = RustArtifactIdentity::new(
        ProducerManifestDigest::new([9; 32]),
        identity().configuration(),
        identity().schema(),
        identity().canonicalization_version(),
    );
    let invalidated = prepare_rust_index_with_reuse(
        vec![
            source(b"a.rs", b"fn alpha() {}\n"),
            source(b"b.rs", b"struct Beta;\n"),
        ],
        changed_identity,
        RustIndexLimits::default(),
        &reusable,
        &cancelled,
        deadline(),
    )
    .expect("identity changes must fall back to clean analysis");
    assert_eq!(invalidated.reused_files(), 0);
    assert_eq!(invalidated.analyzed_files(), 2);
}

#[test]
fn reusable_analysis_must_match_the_exact_current_source() {
    let cancelled = AtomicBool::new(false);
    let other = prepare_rust_index(
        vec![source(b"other.rs", b"fn beta() {}\n")],
        identity(),
        RustIndexLimits::default(),
        &cancelled,
        deadline(),
    )
    .expect("fixture analysis must prepare");
    let current_content = b"fn alpha() {}\n";
    let current_key = AnalysisArtifactKey::new(
        hash_source_content(current_content),
        identity().producer_manifest(),
        identity().configuration(),
        identity().schema(),
        identity().canonicalization_version(),
    );
    let reusable = BTreeMap::from([(
        hash_analysis_artifact_key(&current_key),
        other.files()[0].analysis().clone(),
    )]);

    assert!(matches!(
        prepare_rust_index_with_reuse(
            vec![source(b"current.rs", current_content)],
            identity(),
            RustIndexLimits::default(),
            &reusable,
            &cancelled,
            deadline(),
        ),
        Err(RustIndexPreparationError::Analysis {
            source: RustAnalysisError::InvalidAnalysisArtifact,
            ..
        })
    ));
}

#[test]
fn duplicates_and_aggregate_limits_fail_before_partial_output() {
    let cancelled = AtomicBool::new(false);
    assert!(matches!(
        prepare_rust_index(
            vec![
                source(b"same.rs", b"fn a() {}"),
                source(b"same.rs", b"fn b() {}")
            ],
            identity(),
            RustIndexLimits::default(),
            &cancelled,
            deadline(),
        ),
        Err(RustIndexPreparationError::DuplicateRepositoryPath)
    ));

    let file_limited = RustIndexLimits::try_new(1, 1024, 100, RustAnalysisLimits::default())
        .expect("fixture limits must be valid");
    assert!(matches!(
        prepare_rust_index(
            vec![source(b"a.rs", b""), source(b"b.rs", b"")],
            identity(),
            file_limited,
            &cancelled,
            deadline(),
        ),
        Err(RustIndexPreparationError::FileLimitExceeded { limit: 1 })
    ));

    let byte_limited = RustIndexLimits::try_new(1, 2, 100, RustAnalysisLimits::default())
        .expect("fixture limits must be valid");
    assert!(matches!(
        prepare_rust_index(
            vec![source(b"a.rs", b"abc")],
            identity(),
            byte_limited,
            &cancelled,
            deadline(),
        ),
        Err(RustIndexPreparationError::SourceByteLimitExceeded { limit: 2 })
    ));

    let fact_limited = RustIndexLimits::try_new(1, 1024, 0, RustAnalysisLimits::default())
        .expect("fixture limits must be valid");
    assert!(matches!(
        prepare_rust_index(
            vec![source(b"a.rs", b"fn a() {}")],
            identity(),
            fact_limited,
            &cancelled,
            deadline(),
        ),
        Err(RustIndexPreparationError::FactLimitExceeded { limit: 0 })
    ));
}

#[test]
fn cancellation_deadline_and_syntax_errors_are_explicit() {
    let cancelled = AtomicBool::new(true);
    assert!(matches!(
        prepare_rust_index(
            vec![source(b"a.rs", b"fn a() {}")],
            identity(),
            RustIndexLimits::default(),
            &cancelled,
            deadline(),
        ),
        Err(RustIndexPreparationError::Cancelled)
    ));
    let not_cancelled = AtomicBool::new(false);
    assert!(matches!(
        prepare_rust_index(
            vec![source(b"a.rs", b"fn a() {}")],
            identity(),
            RustIndexLimits::default(),
            &not_cancelled,
            Instant::now(),
        ),
        Err(RustIndexPreparationError::DeadlineExceeded)
    ));

    let prepared = prepare_rust_index(
        vec![source(b"broken.rs", b"fn broken( { struct Kept;")],
        identity(),
        RustIndexLimits::default(),
        &not_cancelled,
        deadline(),
    )
    .expect("syntax errors must remain a successful explicit analysis outcome");
    assert!(prepared.total_syntax_error_nodes() > 0);
    assert!(prepared.files()[0].analysis().has_syntax_errors());
}

#[test]
fn limit_and_error_diagnostics_are_stable_and_redacted() {
    assert_eq!(
        RustIndexLimits::try_new(
            MAX_RUST_INDEX_FILES + 1,
            1,
            1,
            RustAnalysisLimits::default()
        ),
        Err(RustIndexLimitError::FileLimitTooLarge)
    );
    assert_eq!(
        RustIndexLimits::try_new(
            1,
            MAX_RUST_INDEX_SOURCE_BYTES + 1,
            1,
            RustAnalysisLimits::default()
        ),
        Err(RustIndexLimitError::SourceByteLimitTooLarge)
    );
    assert_eq!(
        RustIndexLimits::try_new(
            1,
            1,
            MAX_RUST_INDEX_FACTS + 1,
            RustAnalysisLimits::default()
        ),
        Err(RustIndexLimitError::FactLimitTooLarge)
    );

    let private = source(b"private-name.rs", b"private source contents");
    let diagnostic = format!("{private:?}");
    assert!(!diagnostic.contains("private-name"));
    assert!(!diagnostic.contains("private source"));

    let cancelled = AtomicBool::new(false);
    let prepared = prepare_rust_index(
        vec![source(b"private-name.rs", b"fn private_symbol_name() {}")],
        identity(),
        RustIndexLimits::default(),
        &cancelled,
        deadline(),
    )
    .expect("redaction fixture must prepare");
    let diagnostic = format!("{prepared:?} {:?}", prepared.files()[0]);
    assert!(!diagnostic.contains("private-name"));
    assert!(!diagnostic.contains("private_symbol_name"));
}
