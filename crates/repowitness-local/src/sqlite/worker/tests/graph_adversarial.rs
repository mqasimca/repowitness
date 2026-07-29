#[test]
fn graph_preparation_control_precedes_input_validation() {
    let empty_resolution = || {
        let active = AtomicBool::new(false);
        repowitness_analysis::resolve_rust_graph_sites(
            &[],
            &[],
            repowitness_analysis::RustGraphResolutionLimits::DEFAULT,
            repowitness_analysis::RustGraphResolutionControl::new(&active, deadline()),
        )
        .expect("empty graph should resolve categorically")
    };
    let workspace =
        ConnectedWorkspaceId::for_single_repository(RepositoryIdentityDigest::new([0x84; 32]));
    let cancelled = AtomicBool::new(true);
    assert_eq!(
        crate::sqlite::prepare_rust_graph_generation(
            workspace,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            empty_resolution(),
            crate::sqlite::RustGraphPreparationControl::new(&cancelled, deadline()),
        ),
        Err(crate::RustGraphPreparationError::Cancelled)
    );
    let active = AtomicBool::new(false);
    assert_eq!(
        crate::sqlite::prepare_rust_graph_generation(
            workspace,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            empty_resolution(),
            crate::sqlite::RustGraphPreparationControl::new(&active, Instant::now()),
        ),
        Err(crate::RustGraphPreparationError::DeadlineExceeded)
    );
}

#[test]
fn legacy_generation_reports_not_produced_and_deadlines_fail_closed() {
    let directory = TempDirectory::new();
    let repository = RepositoryIdentityDigest::new([0x85; 32]);
    let (store, _) = OwnedSqliteIndex::start(&directory.database(), 123, deadline())
        .expect("store should start");
    store
        .register_workspace(repository, 0, deadline())
        .expect("workspace should register");
    let generation = store
        .stage(
            0,
            RustSourceSnapshotIdentity::new(
                repository,
                GitStateDigest::new([2; 32]),
                WorktreeStateDigest::new([3; 32]),
                ConfigurationDigest::new([4; 32]),
                ProducerManifestDigest::new([5; 32]),
                AnalysisSchemaDigest::new([6; 32]),
                7,
            ),
            prepared("legacy graph status"),
            GenerationCoverage::new(3, 0, 0, 0),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("legacy generation should stage");
    store
        .activate(generation, 0, deadline())
        .expect("legacy generation should activate");
    let view = store
        .active_workspace_view(
            ConnectedWorkspaceId::for_single_repository(repository),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("view read should succeed")
        .expect("view should be active");
    store.shutdown(deadline()).expect("writer should stop");

    let reader =
        OwnedSqliteReader::start(&directory.database(), deadline()).expect("reader should start");
    assert_eq!(
        reader
            .rust_graph_status(
                &view,
                generation,
                Arc::new(AtomicBool::new(false)),
                deadline(),
            )
            .expect("legacy graph status should be categorical"),
        crate::RustGraphAvailability::NotProduced { generation }
    );
    assert_eq!(
        reader.search_rust_graph_symbols(
            &view,
            generation,
            "legacy",
            crate::RustGraphReadLimits::default(),
            None,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        ),
        Err(crate::RustGraphReadError::GraphNotProduced)
    );
    assert_eq!(
        reader.rust_graph_status(
            &view,
            generation,
            Arc::new(AtomicBool::new(false)),
            Instant::now(),
        ),
        Err(crate::RustGraphReadError::DeadlineExceeded)
    );
    reader.shutdown(deadline()).expect("reader should stop");
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one fixture proves immutable old-view reads after a replacement activation"
)]
fn graph_reads_remain_pinned_across_generation_replacement() {
    let directory = TempDirectory::new();
    let repository = RepositoryIdentityDigest::new([0x86; 32]);
    let (store, _) = OwnedSqliteIndex::start(&directory.database(), 123, deadline())
        .expect("store should start");
    store
        .register_workspace(repository, 0, deadline())
        .expect("workspace should register");
    let (first, first_graph) = graph_candidate(&store, repository, "first");
    store
        .stage_rust_graph(
            first,
            first_graph,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("first graph should stage");
    store
        .activate(first, 0, deadline())
        .expect("first graph should activate");
    let first_view = store
        .active_workspace_view(
            ConnectedWorkspaceId::for_single_repository(repository),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("first view should load")
        .expect("first view should exist");

    store
        .advance_source_epoch(repository, 0, 1, deadline())
        .expect("replacement source epoch should advance");
    let second_source =
        "pub fn target_second() {}\npub fn caller_second() { target_second(); }\n".to_owned();
    let (second, second_graph) = graph_candidate_from_source_at_epoch(
        &store,
        repository,
        1,
        second_source,
        repowitness_analysis::RustGraphResolutionLimits::DEFAULT,
    );
    store
        .stage_rust_graph(
            second,
            second_graph,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("second graph should stage");
    store
        .activate(second, 1, deadline())
        .expect("second graph should activate");
    let second_view = store
        .active_workspace_view(
            ConnectedWorkspaceId::for_single_repository(repository),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("second view should load")
        .expect("second view should exist");
    store.shutdown(deadline()).expect("writer should stop");

    let reader =
        OwnedSqliteReader::start(&directory.database(), deadline()).expect("reader should start");
    let limits = crate::RustGraphReadLimits::default();
    assert_eq!(
        reader
            .search_rust_graph_symbols(
                &first_view,
                first,
                "target_first",
                limits,
                None,
                Arc::new(AtomicBool::new(false)),
                deadline(),
            )
            .expect("old immutable graph should remain readable")
            .total_matches(),
        1
    );
    assert_eq!(
        reader
            .search_rust_graph_symbols(
                &second_view,
                second,
                "target_second",
                limits,
                None,
                Arc::new(AtomicBool::new(false)),
                deadline(),
            )
            .expect("replacement graph should be readable")
            .total_matches(),
        1
    );
    assert_eq!(
        reader.rust_graph_status(
            &second_view,
            first,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        ),
        Err(crate::RustGraphReadError::GenerationUnavailable)
    );
    reader.shutdown(deadline()).expect("reader should stop");
}

#[test]
fn graph_receipt_corruption_is_rejected_after_reader_start() {
    let directory = TempDirectory::new();
    let repository = RepositoryIdentityDigest::new([0x87; 32]);
    let (store, _) = OwnedSqliteIndex::start(&directory.database(), 123, deadline())
        .expect("store should start");
    store
        .register_workspace(repository, 0, deadline())
        .expect("workspace should register");
    let (generation, graph) = graph_candidate(&store, repository, "corrupt");
    store
        .stage_rust_graph(
            generation,
            graph,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("graph should stage");
    store
        .activate(generation, 0, deadline())
        .expect("graph should activate");
    let view = store
        .active_workspace_view(
            ConnectedWorkspaceId::for_single_repository(repository),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("view should load")
        .expect("view should exist");
    store.shutdown(deadline()).expect("writer should stop");

    let reader =
        OwnedSqliteReader::start(&directory.database(), deadline()).expect("reader should start");
    {
        let connection =
            Connection::open(directory.database()).expect("corruption connection should open");
        connection
            .execute_batch(
                "DROP TRIGGER generation_graph_publications_no_semantic_update;
                 UPDATE generation_graph_publications
                    SET definition_count = definition_count + 1;",
            )
            .expect("test should create an inconsistent receipt");
    }
    assert_eq!(
        reader.rust_graph_status(
            &view,
            generation,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        ),
        Err(crate::RustGraphReadError::CorruptGraph)
    );
    reader.shutdown(deadline()).expect("reader should stop");
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one fixture verifies independent traversal bounds on the same immutable graph"
)]
fn graph_trace_exposes_independent_bounds_and_fail_closed_errors() {
    let directory = TempDirectory::new();
    let repository = RepositoryIdentityDigest::new([0x88; 32]);
    let (store, _) = OwnedSqliteIndex::start(&directory.database(), 123, deadline())
        .expect("store should start");
    store
        .register_workspace(repository, 0, deadline())
        .expect("workspace should register");
    let source = "pub fn final_target() {}\n\
                  pub fn middle() { final_target(); }\n\
                  pub fn entry() { middle(); }\n"
        .to_owned();
    let (generation, graph) = graph_candidate_from_source(&store, repository, source);
    store
        .stage_rust_graph(
            generation,
            graph,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("chain graph should stage");
    store
        .activate(generation, 0, deadline())
        .expect("chain graph should activate");
    let view = store
        .active_workspace_view(
            ConnectedWorkspaceId::for_single_repository(repository),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("view should load")
        .expect("view should exist");
    store.shutdown(deadline()).expect("writer should stop");
    let reader =
        OwnedSqliteReader::start(&directory.database(), deadline()).expect("reader should start");
    let entry = reader
        .search_rust_graph_symbols(
            &view,
            generation,
            "entry",
            crate::RustGraphReadLimits::default(),
            None,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("entry should be searchable")
        .definitions()[0]
        .clone();
    let depth_limited =
        crate::RustGraphReadLimits::try_new(1, 100, 10_000, 50_000, 10_000, 4 * 1024 * 1024)
            .expect("depth limit should validate");
    let trace = reader
        .trace_rust_graph(
            &view,
            generation,
            crate::RustGraphTraceStart::Definition(entry.clone()),
            crate::RustGraphDirection::Outbound,
            crate::RustGraphEdgeKinds::ALL,
            depth_limited,
            None,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("depth-limited trace should return a bounded prefix");
    assert!(trace.truncation().depth());
    assert!(!trace.truncation().results());
    assert_eq!(trace.maximum_completed_depth(), 1);

    let result_limited =
        crate::RustGraphReadLimits::try_new(8, 1, 10_000, 50_000, 10_000, 4 * 1024 * 1024)
            .expect("result limit should validate");
    let trace = reader
        .trace_rust_graph(
            &view,
            generation,
            crate::RustGraphTraceStart::Definition(entry.clone()),
            crate::RustGraphDirection::Outbound,
            crate::RustGraphEdgeKinds::ALL,
            result_limited,
            None,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("result-limited trace should return a bounded prefix");
    assert!(trace.truncation().results());
    assert!(!trace.truncation().visited_edges());

    let tiny_output = crate::RustGraphReadLimits::try_new(8, 100, 10_000, 50_000, 10_000, 1)
        .expect("positive output limit should validate");
    assert_eq!(
        reader.trace_rust_graph(
            &view,
            generation,
            crate::RustGraphTraceStart::Definition(entry.clone()),
            crate::RustGraphDirection::Outbound,
            crate::RustGraphEdgeKinds::ALL,
            tiny_output,
            None,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        ),
        Err(crate::RustGraphReadError::OutputLimitExceeded)
    );
    assert_eq!(
        reader.trace_rust_graph(
            &view,
            generation,
            crate::RustGraphTraceStart::Definition(entry),
            crate::RustGraphDirection::Outbound,
            crate::RustGraphEdgeKinds::ALL,
            crate::RustGraphReadLimits::default(),
            None,
            Arc::new(AtomicBool::new(true)),
            deadline(),
        ),
        Err(crate::RustGraphReadError::Cancelled)
    );
    reader.shutdown(deadline()).expect("reader should stop");
}
