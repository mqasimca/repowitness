//! Cross-layer clean-versus-incremental equivalence over a real Git worktree.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};

use repowitness_analysis::{
    ArtifactKeySemantics, ArtifactPlanAction, RustSourceAnalysis, plan_artifact_reuse,
};
use repowitness_application::{PreparedRustIndex, RustArtifactIdentity};
use repowitness_domain::{
    AnalysisArtifactKey, AnalysisSchemaDigest, ConfigurationDigest, ProducerManifestDigest,
    SourceContentDigest,
};
use repowitness_local::{LocalRustIndexLimits, prepare_local_rust_index};

type RustArtifactKey = AnalysisArtifactKey<
    SourceContentDigest,
    ProducerManifestDigest,
    ConfigurationDigest,
    AnalysisSchemaDigest,
    u32,
>;

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct TestRepository {
    root: PathBuf,
}

impl TestRepository {
    fn new() -> Self {
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "repowitness-incremental-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&root).expect("fixture directory should be created");
        run_git(&root, &["init", "--quiet"]);
        fs::create_dir(root.join("src")).expect("source directory should be created");
        fs::write(root.join("src/a.rs"), b"pub fn stable() -> u32 { 1 }\n")
            .expect("stable source should be written");
        fs::write(root.join("src/b.rs"), b"pub fn changing() -> u32 { 1 }\n")
            .expect("changing source should be written");
        fs::write(root.join("README.md"), b"fixture\n")
            .expect("non-Rust fixture should be written");
        run_git(&root, &["add", "--", "."]);
        run_git(
            &root,
            &[
                "-c",
                "user.name=RepoWitness Test",
                "-c",
                "user.email=repowitness@example.invalid",
                "commit",
                "--quiet",
                "-m",
                "fixture",
            ],
        );
        Self { root }
    }

    fn root(&self) -> &Path {
        &self.root
    }
}

impl Drop for TestRepository {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn run_git(root: &Path, arguments: &[&str]) {
    let status = Command::new("git")
        .args(arguments)
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .status()
        .expect("Git should start for the local integration fixture");
    assert!(status.success(), "Git fixture command should succeed");
}

fn identity() -> RustArtifactIdentity {
    RustArtifactIdentity::new(
        ProducerManifestDigest::new([0x11; 32]),
        ConfigurationDigest::new([0x22; 32]),
        AnalysisSchemaDigest::new([0x33; 32]),
        1,
    )
}

fn artifact_key(digest: SourceContentDigest, identity: RustArtifactIdentity) -> RustArtifactKey {
    AnalysisArtifactKey::new(
        digest,
        identity.producer_manifest(),
        identity.configuration(),
        identity.schema(),
        identity.canonicalization_version(),
    )
}

fn prepare(root: &Path, identity: RustArtifactIdentity) -> PreparedRustIndex {
    let cancelled = AtomicBool::new(false);
    prepare_local_rust_index(root, identity, LocalRustIndexLimits::default(), &cancelled)
        .expect("local Rust preparation should succeed")
        .into_prepared()
}

fn logical_output(index: &PreparedRustIndex) -> Vec<(Vec<u8>, RustSourceAnalysis)> {
    index
        .files()
        .iter()
        .map(|file| (file.path().as_bytes().to_vec(), file.analysis().clone()))
        .collect()
}

#[test]
fn real_discovery_clean_and_incremental_materialization_are_equivalent() {
    let repository = TestRepository::new();
    let identity = identity();
    let first = prepare(repository.root(), identity);

    fs::write(
        repository.root().join("src/b.rs"),
        b"pub fn changing() -> u32 { 2 }\n",
    )
    .expect("tracked source should be changed");
    fs::write(repository.root().join("src/c.rs"), b"pub struct Added;\n")
        .expect("untracked source should be added");
    let clean = prepare(repository.root(), identity);

    let cache = first
        .files()
        .iter()
        .map(|file| {
            (
                artifact_key(file.content_digest(), identity),
                file.analysis().clone(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let reusable = cache.keys().cloned().collect::<BTreeSet<_>>();
    let semantics = ArtifactKeySemantics::new(
        identity.producer_manifest(),
        identity.configuration(),
        identity.schema(),
        identity.canonicalization_version(),
    );
    let plan = plan_artifact_reuse(clean.manifest(), &reusable, |_| semantics, || None)
        .expect("incremental planning should succeed");

    assert_eq!(plan.reuse_count().get(), 1);
    assert_eq!(plan.analysis_count().get(), 2);
    let actions = plan
        .as_slice()
        .iter()
        .map(|entry| (entry.path().as_bytes(), entry.action()))
        .collect::<Vec<_>>();
    assert_eq!(
        actions,
        vec![
            (b"src/a.rs".as_slice(), ArtifactPlanAction::Reuse),
            (b"src/b.rs".as_slice(), ArtifactPlanAction::Analyze),
            (b"src/c.rs".as_slice(), ArtifactPlanAction::Analyze),
        ]
    );

    let incremental = plan
        .as_slice()
        .iter()
        .zip(clean.files())
        .map(|(entry, clean_file)| {
            let analysis = match entry.action() {
                ArtifactPlanAction::Reuse => cache
                    .get(entry.key())
                    .expect("a reusable key should resolve")
                    .clone(),
                ArtifactPlanAction::Analyze => clean_file.analysis().clone(),
            };
            (entry.path().as_bytes().to_vec(), analysis)
        })
        .collect::<Vec<_>>();

    assert_eq!(incremental, logical_output(&clean));
}
