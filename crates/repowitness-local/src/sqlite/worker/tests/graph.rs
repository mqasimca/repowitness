fn graph_candidate(
    store: &OwnedSqliteIndex,
    repository: RepositoryIdentityDigest,
    suffix: &str,
) -> (GenerationId, crate::sqlite::PreparedRustGraphGeneration) {
    let source = format!(
        "pub fn target_{suffix}() {{}}\n\
         pub fn caller_{suffix}() {{ target_{suffix}(); }}\n"
    );
    graph_candidate_from_source(store, repository, source)
}

fn graph_candidate_from_source(
    store: &OwnedSqliteIndex,
    repository: RepositoryIdentityDigest,
    source: String,
) -> (GenerationId, crate::sqlite::PreparedRustGraphGeneration) {
    graph_candidate_from_source_with_limits(
        store,
        repository,
        source,
        repowitness_analysis::RustGraphResolutionLimits::DEFAULT,
    )
}

fn graph_candidate_from_source_with_limits(
    store: &OwnedSqliteIndex,
    repository: RepositoryIdentityDigest,
    source: String,
    resolution_limits: repowitness_analysis::RustGraphResolutionLimits,
) -> (GenerationId, crate::sqlite::PreparedRustGraphGeneration) {
    graph_candidate_from_source_at_epoch(store, repository, 0, source, resolution_limits)
}

#[allow(
    clippy::too_many_lines,
    reason = "the fixture builds one complete source/index/graph publication input"
)]
fn graph_candidate_from_source_at_epoch(
    store: &OwnedSqliteIndex,
    repository: RepositoryIdentityDigest,
    source_epoch: u64,
    source: String,
    resolution_limits: repowitness_analysis::RustGraphResolutionLimits,
) -> (GenerationId, crate::sqlite::PreparedRustGraphGeneration) {
    let path = RepositoryPath::try_from_bytes(b"src/graph.rs", PATH_LIMITS)
        .expect("graph fixture path should be valid");
    let worktree_state = WorktreeStateDigest::new(Sha256::digest(source.as_bytes()).into());
    let cancelled = AtomicBool::new(false);
    let prepared = prepare_rust_index(
        vec![ImmutableRustSource::new(
            path.clone(),
            source.as_bytes().to_vec().into_boxed_slice(),
        )],
        artifact_identity(),
        RustIndexLimits::default(),
        &cancelled,
        deadline(),
    )
    .expect("graph source index should prepare");
    let file = &prepared.files()[0];
    let content_digest = file.content_digest();
    let declaration_artifact = file.artifact_digest();
    let facts = file.analysis().facts().to_vec();
    let generation = store
        .stage(
            source_epoch,
            RustSourceSnapshotIdentity::new(
                repository,
                GitStateDigest::new([2; 32]),
                worktree_state,
                ConfigurationDigest::new([4; 32]),
                ProducerManifestDigest::new([5; 32]),
                AnalysisSchemaDigest::new([6; 32]),
                7,
            ),
            prepared,
            GenerationCoverage::new(1, 0, 0, 0),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("graph source generation should stage");
    let source_slot = SourceSlotId::for_repository(repository);
    let definitions = facts
        .into_iter()
        .enumerate()
        .map(|(ordinal, fact)| {
            repowitness_analysis::RustGraphDefinitionOccurrence::try_new(
                source_slot,
                path.clone(),
                declaration_artifact,
                u64::try_from(ordinal).expect("fixture ordinal should fit"),
                fact,
            )
            .expect("fixture definition should validate")
        })
        .collect::<Vec<_>>();
    let graph_key = repowitness_application::CanonicalAnalysisArtifactKey::new(
        content_digest,
        ProducerManifestDigest::new([0x51; 32]),
        ConfigurationDigest::new([0x52; 32]),
        AnalysisSchemaDigest::new([0x53; 32]),
        1,
    );
    let graph_artifact = repowitness_application::hash_analysis_artifact_key(&graph_key);
    let mut analyzer =
        repowitness_analysis::RustGraphSiteAnalyzer::new().expect("Rust grammar should load");
    let graph_analysis = analyzer
        .analyze(
            source.as_bytes(),
            repowitness_analysis::RustGraphAnalysisLimits::DEFAULT,
            repowitness_analysis::RustGraphAnalysisControl::new(&cancelled, deadline()),
        )
        .expect("graph sites should analyze");
    let sites = graph_analysis
        .sites()
        .iter()
        .cloned()
        .map(|site| {
            repowitness_analysis::RustGraphSiteOccurrence::try_new(
                source_slot,
                path.clone(),
                graph_artifact,
                site,
            )
            .expect("fixture site should validate")
        })
        .collect::<Vec<_>>();
    let resolution = repowitness_analysis::resolve_rust_graph_sites(
        &definitions,
        &sites,
        resolution_limits,
        repowitness_analysis::RustGraphResolutionControl::new(&cancelled, deadline()),
    )
    .expect("fixture graph should resolve");
    assert!(
        !resolution.outcomes().is_empty(),
        "fixture graph should resolve at least one categorical site"
    );
    let graph = crate::sqlite::prepare_rust_graph_generation(
        ConnectedWorkspaceId::for_single_repository(repository),
        vec![crate::sqlite::RustGraphSource::new(source_slot, generation)],
        vec![(source_slot, path, graph_key, graph_analysis)],
        definitions,
        resolution,
        crate::sqlite::RustGraphPreparationControl::new(&cancelled, deadline()),
    )
    .expect("complete graph projection should prepare");
    (generation, graph)
}

#[test]
fn graph_publication_is_complete_immutable_and_survives_reopen() {
    let directory = TempDirectory::new();
    let repository = RepositoryIdentityDigest::new([0x81; 32]);
    let (store, _) = OwnedSqliteIndex::start(&directory.database(), 123, deadline())
        .expect("store should start");
    store
        .register_workspace(repository, 0, deadline())
        .expect("workspace should register");
    let (generation, graph) = graph_candidate(&store, repository, "complete");
    let expected = (
        graph.definitions().len(),
        graph.resolution().outcomes().len(),
        graph.resolution().coverage().retained_candidates(),
        graph.edge_count(),
    );
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
        .expect("source and graph should activate together");
    store.shutdown(deadline()).expect("store should stop");

    let connection = Connection::open(directory.database()).expect("graph database should reopen");
    let counts: (String, i64, i64, i64, i64) = connection
        .query_row(
            "SELECT publication.lifecycle_state,
                    (SELECT count(*) FROM generation_graph_definitions
                     WHERE generation_id = publication.generation_id),
                    (SELECT count(*) FROM generation_graph_resolutions
                     WHERE generation_id = publication.generation_id),
                    (SELECT count(*) FROM generation_graph_candidates
                     WHERE generation_id = publication.generation_id),
                    (SELECT count(*) FROM generation_graph_edges
                     WHERE generation_id = publication.generation_id)
             FROM generation_graph_publications AS publication
             WHERE publication.generation_id = ?1",
            [generation.get()],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .expect("complete graph should reopen");
    assert_eq!(counts.0, "complete");
    assert_eq!(counts.1, i64::try_from(expected.0).unwrap());
    assert_eq!(counts.2, i64::try_from(expected.1).unwrap());
    assert_eq!(counts.3, i64::try_from(expected.2).unwrap());
    assert_eq!(counts.4, i64::try_from(expected.3).unwrap());
    assert!(
        connection
            .execute(
                "UPDATE generation_graph_edges SET edge_kind = 'reference'
                 WHERE generation_id = ?1",
                [generation.get()],
            )
            .is_err()
    );
}

#[test]
fn cancelled_required_graph_cannot_replace_the_active_generation() {
    let directory = TempDirectory::new();
    let repository = RepositoryIdentityDigest::new([0x82; 32]);
    let (store, _) = OwnedSqliteIndex::start(&directory.database(), 123, deadline())
        .expect("store should start");
    store
        .register_workspace(repository, 0, deadline())
        .expect("workspace should register");
    let first = store
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
            prepared("legacy"),
            GenerationCoverage::new(2, 0, 0, 0),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("legacy generation should stage");
    store
        .activate(first, 0, deadline())
        .expect("legacy generation should activate");
    let (candidate, graph) = graph_candidate(&store, repository, "cancelled");
    assert_eq!(
        store.stage_rust_graph(
            candidate,
            graph,
            Arc::new(AtomicBool::new(true)),
            deadline(),
        ),
        Err(SqliteStoreError::Cancelled)
    );
    assert!(
        store.activate(candidate, 0, deadline()).is_err(),
        "a failed required graph must make the candidate ineligible"
    );
    assert_eq!(
        store
            .active_generation(repository, deadline())
            .expect("prior active generation should remain readable"),
        Some(first)
    );
    store.shutdown(deadline()).expect("store should stop");

    let connection = Connection::open(directory.database()).expect("graph database should reopen");
    let state: (String, i64, i64) = connection
        .query_row(
            "SELECT lifecycle_state,
                    (SELECT count(*) FROM generation_graph_requirements
                     WHERE generation_id = ?1),
                    (SELECT count(*) FROM generation_graph_publications
                     WHERE generation_id = ?1 AND lifecycle_state = 'complete')
             FROM index_generations WHERE generation_id = ?1",
            [candidate.get()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("cancelled candidate should remain inspectable");
    assert_eq!(state, ("cancelled".to_owned(), 1, 0));
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one end-to-end test verifies every native graph read on the same reopened snapshot"
)]
fn graph_reader_is_view_pinned_categorical_bounded_and_reopen_safe() {
    let directory = TempDirectory::new();
    let repository = RepositoryIdentityDigest::new([0x83; 32]);
    let (store, _) = OwnedSqliteIndex::start(&directory.database(), 123, deadline())
        .expect("store should start");
    store
        .register_workspace(repository, 0, deadline())
        .expect("workspace should register");
    let (generation, graph) = graph_candidate(&store, repository, "reader");
    let unique_site = graph
        .resolution()
        .outcomes()
        .iter()
        .find(|resolved| {
            matches!(
                resolved.outcome(),
                repowitness_analysis::RustGraphResolutionOutcome::Unique { .. }
            )
        })
        .expect("fixture should contain one unique relationship");
    let selector = crate::RustGraphSiteSelector::from_identity(unique_site.site());
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
        .expect("source and graph should activate");
    let view = store
        .active_workspace_view(
            ConnectedWorkspaceId::for_single_repository(repository),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("view read should succeed")
        .expect("default view should be active");
    store.shutdown(deadline()).expect("writer should stop");

    let reader =
        OwnedSqliteReader::start(&directory.database(), deadline()).expect("reader should start");
    let repinned = reader
        .pin_workspace_view(
            ConnectedWorkspaceId::for_single_repository(repository),
            Some(view.view().get()),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("published view pin should succeed")
        .expect("published view should exist");
    assert_eq!(repinned, view);
    let status = reader
        .rust_graph_status(
            &view,
            generation,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("complete graph status should load");
    let crate::RustGraphAvailability::Complete(publication) = status else {
        panic!("graph-enabled generation must not look like a legacy generation");
    };
    assert_eq!(publication.generation(), generation);
    assert_eq!(publication.source_count(), 1);

    let limits = crate::RustGraphReadLimits::default();
    let search = reader
        .search_rust_graph_symbols(
            &view,
            generation,
            "target_reader",
            limits,
            None,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("exact graph symbol should be searchable");
    assert_eq!(search.total_matches(), 1);
    assert_eq!(search.definitions().len(), 1);
    assert_eq!(search.definitions()[0].name(), "target_reader");
    let target = search.definitions()[0].clone();
    let caller = reader
        .search_rust_graph_symbols(
            &view,
            generation,
            "caller_reader",
            limits,
            None,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("caller should be searchable")
        .definitions()[0]
        .clone();

    let outbound = reader
        .trace_rust_graph(
            &view,
            generation,
            crate::RustGraphTraceStart::Definition(caller.clone()),
            crate::RustGraphDirection::Outbound,
            crate::RustGraphEdgeKinds::ALL,
            limits,
            None,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("outbound trace should load exact retained relationships");
    assert!(
        outbound
            .edges()
            .iter()
            .any(|edge| edge.source() == &caller && edge.target() == &target)
    );
    assert!(outbound.edges().iter().all(|edge| {
        edge.site().source_slot() == edge.source().source_slot()
            && edge.site().path() == edge.source().path()
    }));
    let repeated = reader
        .trace_rust_graph(
            &view,
            generation,
            crate::RustGraphTraceStart::Definition(caller),
            crate::RustGraphDirection::Outbound,
            crate::RustGraphEdgeKinds::ALL,
            limits,
            None,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("repeated trace should succeed");
    assert_eq!(outbound, repeated);

    let impact = reader
        .analyze_rust_graph_impact(
            &view,
            generation,
            target,
            crate::RustGraphEdgeKinds::ALL,
            limits,
            None,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("inbound impact should load");
    assert!(impact.impacted().iter().any(|item| {
        item.definition().name() == "caller_reader"
            && item.class() == crate::RustGraphImpactClass::DirectlyConnected
            && item.minimum_depth() == 1
    }));

    let evidence = reader
        .rust_graph_evidence(
            &view,
            generation,
            selector,
            limits,
            None,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("exact site lookup should succeed")
        .expect("exact site should exist");
    assert!(matches!(
        evidence.outcome(),
        crate::RustGraphOutcomeRecord::Unique(_)
    ));
    assert_eq!(evidence.candidate_count(), 1);

    let architecture = reader
        .rust_graph_architecture(
            &view,
            generation,
            limits,
            None,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("architecture summary should load");
    assert_eq!(
        architecture
            .definitions_by_kind()
            .iter()
            .map(|(_, count)| count)
            .sum::<u64>(),
        publication.definition_count()
    );
    assert_eq!(
        reader.search_rust_graph_symbols(
            &view,
            generation,
            "target_reader",
            limits,
            None,
            Arc::new(AtomicBool::new(true)),
            deadline(),
        ),
        Err(crate::RustGraphReadError::Cancelled)
    );
    reader.shutdown(deadline()).expect("reader should stop");
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one end-to-end fixture verifies ambiguous persistence, traversal, and impact"
)]
fn graph_reader_preserves_ambiguous_truncated_relationships() {
    let directory = TempDirectory::new();
    let repository = RepositoryIdentityDigest::new([0x84; 32]);
    let (store, _) = OwnedSqliteIndex::start(&directory.database(), 123, deadline())
        .expect("store should start");
    store
        .register_workspace(repository, 0, deadline())
        .expect("workspace should register");
    let source = "mod first { pub fn shared() {} }\n\
                  mod second { pub fn shared() {} }\n\
                  mod third { pub fn shared() {} }\n\
                  pub fn caller() { shared(); }\n"
        .to_owned();
    let resolution_limits = repowitness_analysis::RustGraphResolutionLimits::try_new(
        100_000,
        250_000,
        2,
        500_000,
        128 * 1024 * 1024,
        128 * 1024 * 1024,
    )
    .expect("fixture resolution limits should validate");
    let (generation, graph) =
        graph_candidate_from_source_with_limits(&store, repository, source, resolution_limits);
    let ambiguous = graph
        .resolution()
        .outcomes()
        .iter()
        .find(|resolved| {
            matches!(
                resolved.outcome(),
                repowitness_analysis::RustGraphResolutionOutcome::Ambiguous { candidates }
                    if candidates.len() == 2
            ) && resolved.candidate_count() == 3
                && resolved.candidates_truncated()
        })
        .expect("three same-named definitions should retain two ambiguous candidates");
    let selector = crate::RustGraphSiteSelector::from_identity(ambiguous.site());
    store
        .stage_rust_graph(
            generation,
            graph,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("ambiguous graph should stage");
    store
        .activate(generation, 0, deadline())
        .expect("ambiguous graph should activate");
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
    let limits = crate::RustGraphReadLimits::default();
    let evidence = reader
        .rust_graph_evidence(
            &view,
            generation,
            selector.clone(),
            limits,
            None,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("ambiguous evidence should load")
        .expect("ambiguous site should exist");
    let crate::RustGraphOutcomeRecord::Ambiguous(candidates) = evidence.outcome() else {
        panic!("categorical ambiguity must not be upgraded to a unique edge");
    };
    assert_eq!(evidence.candidate_count(), 3);
    assert!(evidence.candidates_truncated());
    assert_eq!(candidates.len(), 2);
    let first_target = candidates[0].target().clone();

    let trace = reader
        .trace_rust_graph(
            &view,
            generation,
            crate::RustGraphTraceStart::Site(selector),
            crate::RustGraphDirection::Outbound,
            crate::RustGraphEdgeKinds::ALL,
            limits,
            None,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("ambiguous site trace should retain every stored option");
    assert_eq!(trace.edges().len(), 2);
    assert!(trace.edges().iter().all(|edge| {
        matches!(
            edge.cardinality(),
            crate::RustGraphRelationshipCardinality::Ambiguous {
                candidate_count: 3,
                retained_candidates: 2,
                candidates_truncated: true,
            }
        )
    }));

    let impact = reader
        .analyze_rust_graph_impact(
            &view,
            generation,
            first_target,
            crate::RustGraphEdgeKinds::ALL,
            limits,
            None,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("ambiguous inbound impact should load");
    assert!(impact.unknown_coverage());
    assert!(impact.impacted().iter().any(|item| {
        item.definition().name() == "caller"
            && item.class() == crate::RustGraphImpactClass::Possible
    }));

    let input_limited = crate::RustGraphReadLimits::try_new_with_input(
        1,
        64 * 1024,
        8,
        100,
        10_000,
        50_000,
        10_000,
        4 * 1024 * 1024,
    )
    .expect("small input limit should validate");
    assert_eq!(
        reader.analyze_rust_graph_impact(
            &view,
            generation,
            candidates[0].target().clone(),
            crate::RustGraphEdgeKinds::ALL,
            input_limited,
            None,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        ),
        Err(crate::RustGraphReadError::InputLimitExceeded)
    );
    reader.shutdown(deadline()).expect("reader should stop");
}
