use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use repowitness_analysis::RustSymbolKind;
use repowitness_application::{
    CodeSearchLimits, CodeSearchQuery, CodeSearchRequest, ImmutableRustSource, PreparedRustIndex,
    RepositoryDiagnosticsRequest, RustArtifactIdentity, RustIndexLimits,
    RustSourceSnapshotIdentity, SourceLanguage, SymbolGetSelector, code_search,
    hash_rust_source_snapshot, prepare_rust_index, repository_diagnostics,
};
use repowitness_domain::{
    AnalysisSchemaDigest, ByteOffset, ByteSpan, ConfigurationDigest, GitStateDigest,
    ProducerManifestDigest, RepositoryIdentityDigest, RepositoryPath, RepositoryPathLimits,
    ScipRelationshipKinds, ScipSymbol, ScipSymbolRoles, SourceContentDigest, WorktreeStateDigest,
};
use rusqlite::{Connection, params};

use crate::{
    GenerationCoverage, OwnedSqliteIndex, ScipOccurrenceEvidence, ScipRelationshipDirection,
    ScipRelationshipEvidence, ScipRelationshipEvidenceClass,
};

use super::{
    FIXED_SEARCH_HIT_OUTPUT_BYTES, OwnedSqliteReader, ReaderCommand, SearchLimits,
    SqliteStoreError, checked_output_bytes, evidence_output_bytes,
};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);
const PATH_LIMITS: RepositoryPathLimits = RepositoryPathLimits::new(4096, 256);

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new() -> Self {
        let ordinal = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "repowitness-owned-reader-{}-{ordinal}",
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
        .checked_add(Duration::from_secs(5))
        .expect("test deadline should be representable")
}

#[test]
fn search_output_budget_counts_per_occurrence_producer_and_language() {
    let path = RepositoryPath::try_from_bytes(b"src/example.ts", PATH_LIMITS)
        .expect("fixture path should be valid");
    let language = SourceLanguage::TypeScript;
    let kind = RustSymbolKind::Function;
    let name = "execute";
    let qualified_name = "Api::execute";
    let digest_bytes = 3 * 32;
    let ordinal_and_span_bytes = 5 * 8;
    assert_eq!(
        FIXED_SEARCH_HIT_OUTPUT_BYTES,
        digest_bytes + ordinal_and_span_bytes
    );
    let expected = FIXED_SEARCH_HIT_OUTPUT_BYTES
        + path.byte_count().get()
        + u64::try_from(language.as_str().len()).expect("language length fits")
        + u64::try_from(kind.as_str().len()).expect("kind length fits")
        + u64::try_from(name.len()).expect("name length fits")
        + u64::try_from(qualified_name.len()).expect("qualified name length fits");

    assert_eq!(
        checked_output_bytes(0, &path, language, kind, name, qualified_name, expected,),
        Ok(expected)
    );
    assert_eq!(
        checked_output_bytes(0, &path, language, kind, name, qualified_name, expected - 1,),
        Err(SqliteStoreError::SearchOutputLimitExceeded)
    );
}

#[test]
fn scip_evidence_output_budget_counts_wire_encoding() {
    let path = RepositoryPath::try_from_bytes(b"src/\xff.rs", PATH_LIMITS)
        .expect("fixture path should be valid");
    let source = ScipSymbol::try_new("source\"\n".to_owned()).expect("source should validate");
    let target =
        ScipSymbol::try_new("target\\\u{0001}".to_owned()).expect("target should validate");
    let kinds = ScipRelationshipKinds::try_new(true, false, false, false)
        .expect("relationship kinds should validate");
    let relationship = ScipRelationshipEvidence::with_evidence(
        path.clone(),
        SourceContentDigest::new([0; 32]),
        ScipRelationshipDirection::Outgoing,
        source.clone(),
        target.clone(),
        kinds,
        ScipRelationshipEvidenceClass::ProducerDeclared,
    );
    let occurrence = ScipOccurrenceEvidence::new(
        path.clone(),
        SourceContentDigest::new([1; 32]),
        ByteSpan::try_new(ByteOffset::ZERO, ByteOffset::new(1)).expect("span should validate"),
        ScipSymbolRoles::NONE,
    );

    let actual = evidence_output_bytes(&[occurrence], &[relationship])
        .expect("wire output size should be representable");
    let encoded_path = 7 + path.byte_count().get() * 2;
    let source_json = 2 + 6 * source.as_str().len() as u64;
    let target_json = 2 + 6 * target.as_str().len() as u64;
    let expected = 512 + encoded_path + 512 + encoded_path + source_json + target_json;
    assert_eq!(actual, expected);
    assert!(actual > 48 * 2 + path.byte_count().get() * 2);
}

#[cfg(unix)]
#[test]
fn sqlite_owners_resolve_parent_symlinks_but_reject_the_database_symlink() {
    use std::os::unix::fs::symlink;

    let directory = TempDirectory::new();
    let aliases = TempDirectory::new();
    let linked_parent = aliases.0.join("linked-parent");
    symlink(&directory.0, &linked_parent).expect("parent symlink should be created");
    let database = linked_parent.join("index.sqlite3");

    let (writer, _) = OwnedSqliteIndex::start(&database, 123, deadline())
        .expect("writer should resolve the parent symlink");
    writer.shutdown(deadline()).expect("writer should stop");

    let reader = OwnedSqliteReader::start(&database, deadline())
        .expect("reader should resolve the parent symlink");
    reader.shutdown(deadline()).expect("reader should stop");

    let database_alias = aliases.0.join("database-alias.sqlite3");
    symlink(directory.database(), &database_alias).expect("database symlink should be created");
    let writer_result = OwnedSqliteIndex::start(&database_alias, 123, deadline());
    assert_eq!(writer_result.err(), Some(SqliteStoreError::OpenFailed));
    let reader_result = OwnedSqliteReader::start(&database_alias, deadline());
    assert_eq!(reader_result.err(), Some(SqliteStoreError::OpenFailed));
}

#[test]
fn saturated_drop_detaches_instead_of_waiting_without_shutdown() {
    let (commands, _receiver) = mpsc::sync_channel(1);
    let (reply, _reply_receiver) = mpsc::sync_channel(1);
    commands
        .send(ReaderCommand::Shutdown { reply })
        .expect("fixture queue should accept one command");
    let worker = thread::spawn(|| thread::sleep(Duration::from_millis(500)));
    let reader = OwnedSqliteReader {
        commands,
        worker: Some(worker),
    };

    let started = Instant::now();
    drop(reader);

    assert!(started.elapsed() < Duration::from_millis(250));
}

fn identity() -> RustSourceSnapshotIdentity {
    RustSourceSnapshotIdentity::new(
        RepositoryIdentityDigest::new([1; 32]),
        GitStateDigest::new([2; 32]),
        WorktreeStateDigest::new([3; 32]),
        ConfigurationDigest::new([4; 32]),
        ProducerManifestDigest::new([5; 32]),
        AnalysisSchemaDigest::new([6; 32]),
        7,
    )
}

fn artifact_identity() -> RustArtifactIdentity {
    RustArtifactIdentity::new(
        identity().producer_manifest(),
        identity().configuration(),
        identity().analysis_schema(),
        identity().canonicalization_version(),
    )
}

fn prepared(version: u8) -> PreparedRustIndex {
    let first = if version == 1 {
        b"pub fn old_generation_only() {}\npub fn shared_token() {}\n".as_slice()
    } else {
        b"pub fn new_generation_only() {}\npub fn shared_token() {}\n".as_slice()
    };
    let cancelled = AtomicBool::new(false);
    prepare_rust_index(
        vec![
            ImmutableRustSource::new(
                RepositoryPath::try_from_bytes(b"src/a.rs", PATH_LIMITS)
                    .expect("fixture path should be valid"),
                first.to_vec().into_boxed_slice(),
            ),
            ImmutableRustSource::new(
                RepositoryPath::try_from_bytes(b"src/b.rs", PATH_LIMITS)
                    .expect("fixture path should be valid"),
                b"pub fn shared_token() {}\n".to_vec().into_boxed_slice(),
            ),
        ],
        artifact_identity(),
        RustIndexLimits::default(),
        &cancelled,
        deadline(),
    )
    .expect("fixture index should prepare")
}

fn publish(
    writer: &OwnedSqliteIndex,
    epoch: u64,
    prepared: PreparedRustIndex,
) -> super::GenerationId {
    let generation = writer
        .stage(
            epoch,
            identity(),
            prepared,
            GenerationCoverage::new(2, 0, 0, 0),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("generation should stage");
    writer
        .activate(generation, epoch, deadline())
        .expect("generation should activate");
    generation
}

#[test]
fn reusable_artifacts_are_exact_bounded_and_cancelled_without_partial_output() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let prepared = prepared(1);
    let expected = prepared
        .files()
        .iter()
        .map(|file| (file.artifact_digest(), file.analysis().clone()))
        .collect::<BTreeMap<_, _>>();
    let requested = expected.keys().copied().collect::<Vec<_>>();
    let (writer, _) =
        OwnedSqliteIndex::start(&database, 123, deadline()).expect("writer should start");
    writer
        .register_workspace(identity().repository(), 0, deadline())
        .expect("workspace should register");
    publish(&writer, 0, prepared);
    writer.shutdown(deadline()).expect("writer should stop");

    let reader = OwnedSqliteReader::start(&database, deadline()).expect("reader should start");
    let actual = reader
        .load_reusable_artifacts(
            &requested,
            artifact_identity(),
            RustIndexLimits::default(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("exact artifacts should load");
    assert_eq!(actual, expected);

    assert_eq!(
        reader.load_reusable_artifacts(
            &requested,
            artifact_identity(),
            RustIndexLimits::default(),
            Arc::new(AtomicBool::new(true)),
            deadline(),
        ),
        Err(SqliteStoreError::Cancelled)
    );
    assert_eq!(
        reader.load_reusable_artifacts(
            &requested,
            artifact_identity(),
            RustIndexLimits::default(),
            Arc::new(AtomicBool::new(false)),
            Instant::now(),
        ),
        Err(SqliteStoreError::DeadlineExceeded)
    );

    let duplicate = requested
        .first()
        .copied()
        .map(|digest| [digest, digest])
        .expect("fixture must contain an artifact");
    assert_eq!(
        reader.load_reusable_artifacts(
            &duplicate,
            artifact_identity(),
            RustIndexLimits::default(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        ),
        Err(SqliteStoreError::IntegrityCheckFailed)
    );
    reader.shutdown(deadline()).expect("reader should stop");
}

#[test]
fn reusable_artifact_payload_corruption_fails_closed() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let prepared = prepared(1);
    let requested = prepared
        .files()
        .iter()
        .map(|file| file.artifact_digest())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let corrupted = requested[0];
    let (writer, _) =
        OwnedSqliteIndex::start(&database, 123, deadline()).expect("writer should start");
    writer
        .register_workspace(identity().repository(), 0, deadline())
        .expect("workspace should register");
    publish(&writer, 0, prepared);
    writer.shutdown(deadline()).expect("writer should stop");

    let connection = Connection::open(&database).expect("fixture database should open");
    connection
        .execute_batch("DROP TRIGGER artifact_facts_no_update")
        .expect("fixture immutability trigger should be removed");
    connection
        .execute(
            "UPDATE artifact_facts SET name = 'corrupt'
                 WHERE artifact_digest = ?1 AND ordinal = 0",
            params![corrupted.as_bytes().as_slice()],
        )
        .expect("fixture fact should be corrupted");
    drop(connection);

    let reader = OwnedSqliteReader::start(&database, deadline()).expect("reader should start");
    assert_eq!(
        reader.load_reusable_artifacts(
            &requested,
            artifact_identity(),
            RustIndexLimits::default(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        ),
        Err(SqliteStoreError::IntegrityCheckFailed)
    );
    reader.shutdown(deadline()).expect("reader should stop");
}

#[test]
fn reusable_artifact_metadata_is_bounded_before_allocation() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let prepared = prepared(1);
    let requested = prepared
        .files()
        .iter()
        .map(|file| file.artifact_digest())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let corrupted = requested[0];
    let (writer, _) =
        OwnedSqliteIndex::start(&database, 123, deadline()).expect("writer should start");
    writer
        .register_workspace(identity().repository(), 0, deadline())
        .expect("workspace should register");
    publish(&writer, 0, prepared);
    writer.shutdown(deadline()).expect("writer should stop");

    let connection = Connection::open(&database).expect("fixture database should open");
    connection
        .execute_batch("DROP TRIGGER analysis_artifacts_no_semantic_update")
        .expect("fixture immutability trigger should be removed");
    connection
        .execute(
            "UPDATE analysis_artifacts SET fact_count = 2147483647
                 WHERE artifact_digest = ?1",
            params![corrupted.as_bytes().as_slice()],
        )
        .expect("fixture fact count should be corrupted");
    drop(connection);

    let reader = OwnedSqliteReader::start(&database, deadline()).expect("reader should start");
    assert_eq!(
        reader.load_reusable_artifacts(
            &requested,
            artifact_identity(),
            RustIndexLimits::default(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        ),
        Err(SqliteStoreError::IntegrityCheckFailed)
    );
    reader.shutdown(deadline()).expect("reader should stop");
}

#[test]
fn persisted_language_and_repository_path_must_agree() {
    let directory = TempDirectory::new();
    let database = directory.database();
    let (writer, _) =
        OwnedSqliteIndex::start(&database, 123, deadline()).expect("writer should start");
    writer
        .register_workspace(identity().repository(), 0, deadline())
        .expect("workspace should register");
    publish(&writer, 0, prepared(1));
    writer.shutdown(deadline()).expect("writer should stop");

    let connection = Connection::open(&database).expect("fixture database should open");
    connection
        .execute_batch("DROP TRIGGER analysis_artifacts_no_semantic_update")
        .expect("fixture immutability trigger should be removed");
    connection
        .execute("UPDATE analysis_artifacts SET language = 'go'", [])
        .expect("fixture artifact language should be corrupted");
    drop(connection);

    let reader = OwnedSqliteReader::start(&database, deadline()).expect("reader should start");
    assert_eq!(
        reader
            .search(
                identity().repository(),
                "shared_token",
                SearchLimits::default(),
                Arc::new(AtomicBool::new(false)),
                deadline(),
            )
            .expect_err("path/language mismatch must fail closed"),
        SqliteStoreError::IntegrityCheckFailed
    );
    reader.shutdown(deadline()).expect("reader should stop");
}

#[test]
fn reader_pins_active_generation_and_orders_equal_hits_by_exact_path() {
    let directory = TempDirectory::new();
    let (writer, _) = OwnedSqliteIndex::start(&directory.database(), 123, deadline())
        .expect("writer should start");
    writer
        .register_workspace(identity().repository(), 0, deadline())
        .expect("workspace should register");
    let first_prepared = prepared(1);
    let first_snapshot = hash_rust_source_snapshot(identity(), first_prepared.manifest_digest());
    let first = publish(&writer, 0, first_prepared);
    let reader =
        OwnedSqliteReader::start(&directory.database(), deadline()).expect("reader should start");

    let results = reader
        .search(
            identity().repository(),
            "shared_token",
            SearchLimits::default(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("active generation should be searchable");
    assert_eq!(results.generation(), first);
    assert_eq!(results.snapshot(), first_snapshot);
    assert_eq!(results.producer_manifest(), identity().producer_manifest());
    assert_eq!(
        results.index_coverage(),
        GenerationCoverage::new(2, 0, 0, 0)
    );
    assert_eq!(results.hits().len(), 2);
    assert_eq!(results.total_matches(), 2);
    assert_eq!(results.hits()[0].path().as_bytes(), b"src/a.rs");
    assert_eq!(results.hits()[1].path().as_bytes(), b"src/b.rs");

    let material = code_search(
        &reader,
        CodeSearchRequest::new(
            identity().repository(),
            CodeSearchQuery::try_new("shared_token").expect("query should be valid"),
            CodeSearchLimits::try_new(1, 64 * 1024).expect("limits should be valid"),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        ),
    )
    .expect("shared application search should succeed");
    assert_eq!(material.claim().returned_matches(), 1);
    assert_eq!(material.claim().total_matches(), 2);
    assert_eq!(material.coverage().truncated().get(), 1);
    assert_eq!(material.snapshot(), &first_snapshot);
    assert_eq!(material.generation(), &first);

    writer
        .advance_source_epoch(identity().repository(), 0, 1, deadline())
        .expect("source epoch should advance");
    let second = publish(&writer, 1, prepared(2));
    let current = reader
        .search(
            identity().repository(),
            "new_generation_only",
            SearchLimits::default(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("new active generation should be searchable");
    assert_eq!(current.generation(), second);
    assert_eq!(current.hits().len(), 1);
    let old = reader
        .search(
            identity().repository(),
            "old_generation_only",
            SearchLimits::default(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("retained generations should not leak into search");
    assert_eq!(old.generation(), second);
    assert!(old.hits().is_empty());

    reader.shutdown(deadline()).expect("reader should stop");
    writer.shutdown(deadline()).expect("writer should stop");
}

#[test]
fn exact_lookup_requires_active_context_and_missing_occurrences_remain_explicit() {
    let directory = TempDirectory::new();
    let (writer, _) = OwnedSqliteIndex::start(&directory.database(), 123, deadline())
        .expect("writer should start");
    writer
        .register_workspace(identity().repository(), 0, deadline())
        .expect("workspace should register");
    let first_prepared = prepared(1);
    let first_snapshot = hash_rust_source_snapshot(identity(), first_prepared.manifest_digest());
    let first = publish(&writer, 0, first_prepared);
    let reader =
        OwnedSqliteReader::start(&directory.database(), deadline()).expect("reader should start");
    let search = reader
        .search(
            identity().repository(),
            "shared_token",
            SearchLimits::default(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("fixture occurrence should be searchable");
    let first_hit = &search.hits()[0];
    let selector = SymbolGetSelector::new(
        first_hit.path().clone(),
        first_hit.content_digest(),
        first_hit.artifact_digest(),
        first_hit.fact_ordinal(),
    );
    let exact = reader
        .get_symbol(
            identity().repository(),
            first_snapshot,
            first,
            selector.clone(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("exact active occurrence should resolve");
    assert_eq!(exact.snapshot(), first_snapshot);
    assert_eq!(exact.generation(), first);
    assert_eq!(exact.producer_manifest(), identity().producer_manifest());
    assert_eq!(
        exact
            .hit()
            .expect("exact occurrence should exist")
            .qualified_name(),
        "shared_token"
    );

    writer
        .advance_source_epoch(identity().repository(), 0, 1, deadline())
        .expect("source epoch should advance");
    let second_prepared = prepared(2);
    let second_snapshot = hash_rust_source_snapshot(identity(), second_prepared.manifest_digest());
    let second = publish(&writer, 1, second_prepared);
    assert_eq!(
        reader
            .get_symbol(
                identity().repository(),
                first_snapshot,
                first,
                selector,
                Arc::new(AtomicBool::new(false)),
                deadline(),
            )
            .expect_err("stale source context must not silently retarget"),
        SqliteStoreError::GenerationUnavailable
    );
    let missing = reader
        .get_symbol(
            identity().repository(),
            second_snapshot,
            second,
            SymbolGetSelector::new(
                RepositoryPath::try_from_bytes(b"src/a.rs", PATH_LIMITS)
                    .expect("fixture path should be valid"),
                repowitness_domain::SourceContentDigest::new([9; 32]),
                repowitness_domain::AnalysisArtifactDigest::new([9; 32]),
                999,
            ),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("missing exact occurrence should be a bounded result");
    assert!(missing.hit().is_none());

    reader.shutdown(deadline()).expect("reader should stop");
    writer.shutdown(deadline()).expect("writer should stop");
}

#[test]
fn hostile_syntax_is_literal_and_query_result_and_control_bounds_fail_closed() {
    let directory = TempDirectory::new();
    let (writer, _) = OwnedSqliteIndex::start(&directory.database(), 123, deadline())
        .expect("writer should start");
    writer
        .register_workspace(identity().repository(), 0, deadline())
        .expect("workspace should register");
    publish(&writer, 0, prepared(1));
    let reader =
        OwnedSqliteReader::start(&directory.database(), deadline()).expect("reader should start");

    let hostile = reader
        .search(
            identity().repository(),
            "shared_token OR old_generation_only*",
            SearchLimits::default(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("hostile-looking syntax should remain a literal query");
    assert!(hostile.hits().is_empty());
    assert_eq!(
        SearchLimits::try_new(0, 1).expect_err("zero results should fail"),
        SqliteStoreError::InvalidSearchLimits
    );
    assert_eq!(
        reader
            .search(
                identity().repository(),
                "",
                SearchLimits::default(),
                Arc::new(AtomicBool::new(false)),
                deadline(),
            )
            .expect_err("empty query should fail"),
        SqliteStoreError::InvalidSearchQuery
    );
    assert_eq!(
        reader
            .search(
                identity().repository(),
                "shared_token",
                SearchLimits::try_new(10, 1).expect("tiny output bound is valid"),
                Arc::new(AtomicBool::new(false)),
                deadline(),
            )
            .expect_err("output should exceed one byte"),
        SqliteStoreError::SearchOutputLimitExceeded
    );
    assert_eq!(
        reader
            .search(
                identity().repository(),
                "shared_token",
                SearchLimits::default(),
                Arc::new(AtomicBool::new(true)),
                deadline(),
            )
            .expect_err("pre-cancelled search should fail"),
        SqliteStoreError::Cancelled
    );

    reader.shutdown(deadline()).expect("reader should stop");
    writer.shutdown(deadline()).expect("writer should stop");
}

include!("tests/artifact_parser_diagnostics.rs");
