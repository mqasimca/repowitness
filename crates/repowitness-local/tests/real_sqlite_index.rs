//! Opt-in end-to-end persistence and retrieval probe for a real Rust repository.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use repowitness_application::{
    CodeSearchLimits, CodeSearchQuery, CodeSearchRequest, PublishRustIndexRequest,
    RepositoryIdentityTextV1, RustArtifactIdentity, RustIndexCoverage, RustSourceSnapshotIdentity,
    code_search, publish_rust_index,
};
use repowitness_domain::{
    AnalysisSchemaDigest, ConfigurationDigest, GitStateDigest, ProducerManifestDigest,
    RepositoryIdentityDigest, WorktreeStateDigest,
};
use repowitness_local::{
    GenerationId, LocalIndexRequest, LocalRustIndexLimits, LocalRustIndexPreparation,
    LocalSymbolGetRequest, LocalSymbolGetResult, LocalSymbolSelectorText, OwnedSqliteIndex,
    OwnedSqliteReader, ProjectionRebuildLimits, RepositoryPathTextByteLimit, RepositoryPathTextV1,
    SearchLimits, SearchResults, get_local_rust_symbol, index_local_rust_repository,
    prepare_local_rust_index,
};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);
const REPOSITORY_ID: &str = concat!(
    "rwi1:h:",
    "5050505050505050505050505050505050505050505050505050505050505050"
);

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new() -> Self {
        let ordinal = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "repowitness-real-sqlite-{}-{ordinal}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("fixture directory should be created");
        Self(path)
    }

    fn database(&self) -> PathBuf {
        self.0.join("index.sqlite3")
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn deadline() -> Instant {
    Instant::now()
        .checked_add(Duration::from_secs(30))
        .expect("test deadline should be representable")
}

fn material_search(
    reader: &OwnedSqliteReader,
    repository: RepositoryIdentityDigest,
    query: &str,
) -> repowitness_application::CodeSearchResult<GenerationId> {
    code_search(
        reader,
        CodeSearchRequest::new(
            repository,
            CodeSearchQuery::try_new(query).expect("selected symbol query should be valid"),
            CodeSearchLimits::default(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        ),
    )
    .expect("shared application search should succeed")
}

fn assert_material_matches_storage(
    material: &repowitness_application::CodeSearchResult<GenerationId>,
    results: &SearchResults,
    generation: GenerationId,
) {
    assert_eq!(
        material.claim().returned_matches(),
        u64::try_from(results.hits().len()).expect("hit count should fit")
    );
    assert_eq!(material.claim().total_matches(), results.total_matches());
    assert_eq!(material.generation(), &generation);
}

fn lower_hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        use std::fmt::Write;
        write!(output, "{byte:02x}").expect("writing to a string cannot fail");
    }
    output
}

fn retrieve_first_real_symbol(
    repository: &Path,
    database: &Path,
    repository_identity: RepositoryIdentityDigest,
    results: &SearchResults,
) -> LocalSymbolGetResult {
    let hit = results
        .hits()
        .first()
        .expect("real search should have a hit");
    let repository_text = RepositoryIdentityTextV1::encode(repository_identity);
    let snapshot = lower_hex(results.snapshot().as_bytes());
    let path =
        RepositoryPathTextV1::encode(hit.path(), RepositoryPathTextByteLimit::new(2_097_160))
            .expect("persisted real path should have canonical text");
    let content = lower_hex(hit.content_digest().as_bytes());
    let artifact = lower_hex(hit.artifact_digest().as_bytes());
    let selector = LocalSymbolSelectorText::new(
        &snapshot,
        results.generation().get(),
        path.as_str(),
        &content,
        &artifact,
        hit.fact_ordinal(),
    );
    get_local_rust_symbol(
        LocalSymbolGetRequest::new(repository, database, repository_text.as_str(), selector),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("one exact real declaration should be retrievable")
}

fn configured_repository() -> PathBuf {
    let configured = PathBuf::from(
        std::env::var_os("REPOWITNESS_REAL_REPOSITORY")
            .expect("REPOWITNESS_REAL_REPOSITORY must identify a Git worktree"),
    );
    if configured.is_absolute() {
        configured
    } else {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("the local crate should have a workspace root")
            .join(configured)
    }
}

fn selected_real_query(preparation: &LocalRustIndexPreparation) -> String {
    preparation
        .prepared()
        .files()
        .iter()
        .flat_map(|file| file.analysis().facts())
        .find(|fact| fact.name().len() <= 64)
        .expect("configured repository should contain at least one Rust declaration")
        .name()
        .to_owned()
}

fn assert_real_retrieval(
    results: &SearchResults,
    retrieved: &LocalSymbolGetResult,
    rebuilt: &LocalSymbolGetResult,
) {
    let symbol = retrieved
        .claim()
        .symbol()
        .expect("the exact real occurrence should resolve");
    assert_eq!(symbol.occurrence().name(), results.hits()[0].name());
    assert!(!symbol.declaration().is_empty());
    assert_eq!(
        rebuilt
            .claim()
            .symbol()
            .expect("projection rebuild must preserve exact retrieval")
            .declaration(),
        symbol.declaration()
    );
}

fn artifact_identity() -> RustArtifactIdentity {
    RustArtifactIdentity::new(
        ProducerManifestDigest::new([0x51; 32]),
        ConfigurationDigest::new([0x52; 32]),
        AnalysisSchemaDigest::new([0x53; 32]),
        1,
    )
}

fn snapshot_identity(
    git_state: GitStateDigest,
    worktree_state: WorktreeStateDigest,
) -> RustSourceSnapshotIdentity {
    let artifact = artifact_identity();
    RustSourceSnapshotIdentity::new(
        RepositoryIdentityDigest::new([0x50; 32]),
        git_state,
        worktree_state,
        artifact.configuration(),
        artifact.producer_manifest(),
        artifact.schema(),
        artifact.canonicalization_version(),
    )
}

#[test]
#[ignore = "requires REPOWITNESS_REAL_REPOSITORY, Git, and a readable Rust worktree"]
fn configured_repository_persists_activates_and_searches_every_prepared_rust_fact() {
    let repository = configured_repository();
    let cancelled = AtomicBool::new(false);
    let preparation = prepare_local_rust_index(
        &repository,
        artifact_identity(),
        LocalRustIndexLimits::default(),
        &cancelled,
    )
    .expect("real repository should prepare");
    let searched = preparation.selected_rust_files();
    let skipped = preparation.skipped_non_rust_paths();
    let syntax_errors = preparation.prepared().total_syntax_error_nodes();
    let query = selected_real_query(&preparation);
    let expected_facts = preparation.prepared().total_facts();
    let snapshot_identity =
        snapshot_identity(preparation.git_state(), preparation.worktree_state());
    let prepared = preparation.into_prepared();

    let directory = TempDirectory::new();
    let (writer, startup) = OwnedSqliteIndex::start(&directory.database(), 0, deadline())
        .expect("owned writer should start");
    writer
        .register_workspace(snapshot_identity.repository(), 0, deadline())
        .expect("workspace should register");
    let publication = publish_rust_index(
        &writer,
        PublishRustIndexRequest::new(
            0,
            snapshot_identity,
            prepared,
            RustIndexCoverage::new(searched, skipped, syntax_errors, 0),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        ),
    )
    .expect("prepared repository should persist and activate");
    let generation = publication.generation();
    let reader =
        OwnedSqliteReader::start(&directory.database(), deadline()).expect("reader should start");
    let results = reader
        .search(
            snapshot_identity.repository(),
            &query,
            SearchLimits::default(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("one real symbol query should succeed");
    let material = material_search(&reader, snapshot_identity.repository(), &query);
    let retrieved = retrieve_first_real_symbol(
        &repository,
        &directory.database(),
        snapshot_identity.repository(),
        &results,
    );
    let rebuild = writer
        .rebuild_search_projection(
            ProjectionRebuildLimits::default(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("real search projection should rebuild");
    let rebuilt_results = reader
        .search(
            snapshot_identity.repository(),
            &query,
            SearchLimits::default(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("the rebuilt real projection should remain searchable");
    let rebuilt_material = material_search(&reader, snapshot_identity.repository(), &query);
    let rebuilt_retrieved = retrieve_first_real_symbol(
        &repository,
        &directory.database(),
        snapshot_identity.repository(),
        &rebuilt_results,
    );

    assert_eq!(startup.recovered_generations(), 0);
    assert_eq!(results.generation(), generation);
    assert!(!results.hits().is_empty());
    assert!(results.hits().len() <= usize::from(SearchLimits::default().max_results()));
    assert!(expected_facts >= u64::try_from(results.hits().len()).expect("hit count should fit"));
    assert_material_matches_storage(&material, &results, generation);
    assert_eq!(rebuild.previous_slot(), 0);
    assert_eq!(rebuild.active_slot(), 1);
    assert_eq!(rebuild.rebuilt_rows(), expected_facts);
    assert_eq!(rebuilt_results, results);
    assert_eq!(rebuilt_material, material);
    assert_real_retrieval(&results, &retrieved, &rebuilt_retrieved);

    reader.shutdown(deadline()).expect("reader should stop");
    writer.shutdown(deadline()).expect("writer should stop");
}

#[test]
#[ignore = "requires REPOWITNESS_REAL_REPOSITORY, Git, and a readable Rust worktree"]
fn configured_repository_reuses_every_unchanged_production_artifact() {
    let repository = configured_repository();
    let directory = TempDirectory::new();
    let database = directory.database();
    let request = LocalIndexRequest::new(&repository, &database, REPOSITORY_ID, 0);

    let first = index_local_rust_repository(request, Arc::new(AtomicBool::new(false)))
        .expect("clean real generation should activate");
    assert_eq!(
        [
            first.reused_rust_files(),
            first.reused_go_files(),
            first.reused_typescript_files(),
            first.reused_tsx_files(),
            first.reused_python_files(),
        ],
        [0; 5]
    );
    assert_eq!(
        [
            first.analyzed_rust_files(),
            first.analyzed_go_files(),
            first.analyzed_typescript_files(),
            first.analyzed_tsx_files(),
            first.analyzed_python_files(),
        ],
        [
            first.indexed_rust_files(),
            first.indexed_go_files(),
            first.indexed_typescript_files(),
            first.indexed_tsx_files(),
            first.indexed_python_files(),
        ]
    );

    let second = index_local_rust_repository(request, Arc::new(AtomicBool::new(false)))
        .expect("unchanged real generation should activate");
    assert_eq!(
        [
            second.reused_rust_files(),
            second.reused_go_files(),
            second.reused_typescript_files(),
            second.reused_tsx_files(),
            second.reused_python_files(),
        ],
        [
            second.indexed_rust_files(),
            second.indexed_go_files(),
            second.indexed_typescript_files(),
            second.indexed_tsx_files(),
            second.indexed_python_files(),
        ]
    );
    assert_eq!(
        [
            second.analyzed_rust_files(),
            second.analyzed_go_files(),
            second.analyzed_typescript_files(),
            second.analyzed_tsx_files(),
            second.analyzed_python_files(),
        ],
        [0; 5]
    );
    assert_eq!(second.total_facts(), first.total_facts());
    assert_eq!(second.total_source_bytes(), first.total_source_bytes());
}
