fn scip_varint(mut value: u64) -> Vec<u8> {
    let mut encoded = Vec::new();
    loop {
        let mut byte = u8::try_from(value & 0x7f).expect("seven bits fit in one byte");
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        encoded.push(byte);
        if value == 0 {
            return encoded;
        }
    }
}

fn scip_field(field: u32, wire: u8, payload: &[u8]) -> Vec<u8> {
    let mut encoded = scip_varint(u64::from((field << 3) | u32::from(wire)));
    if wire == 2 {
        encoded.extend(scip_varint(u64::try_from(payload.len()).expect("fixture size fits")));
    }
    encoded.extend(payload);
    encoded
}

fn scip_lower_hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn synthetic_scip_document() -> Vec<u8> {
    let mut legacy_range = Vec::new();
    for component in [0_u64, 0, 0, 1] {
        legacy_range.extend(scip_varint(component));
    }
    let occurrence = scip_field(1, 2, &legacy_range);
    let mut document = scip_field(1, 2, b"src/lib.rs");
    document.extend(scip_field(2, 2, &occurrence));
    document.extend(scip_field(4, 2, b"rust"));
    document.extend(scip_field(6, 0, &[1]));
    document
}

fn synthetic_scip_document_with_relationships(
    path: &[u8],
    occurrence_symbol: &[u8],
    relationship_targets: &[&[u8]],
    name_start: u64,
    name_end: u64,
) -> Vec<u8> {
    let mut legacy_range = Vec::new();
    for component in [0_u64, name_start, 0, name_end] {
        legacy_range.extend(scip_varint(component));
    }
    let mut occurrence = scip_field(1, 2, &legacy_range);
    occurrence.extend(scip_field(2, 2, occurrence_symbol));
    occurrence.extend(scip_field(3, 0, &[1]));
    let mut document = scip_field(1, 2, path);
    document.extend(scip_field(2, 2, &occurrence));
    if !relationship_targets.is_empty() {
        let mut symbol_information = scip_field(1, 2, occurrence_symbol);
        for target in relationship_targets {
            let mut relationship = scip_field(1, 2, target);
            relationship.extend(scip_field(2, 0, &[1]));
            symbol_information.extend(scip_field(4, 2, &relationship));
        }
        document.extend(scip_field(3, 2, &symbol_information));
    }
    document.extend(scip_field(4, 2, b"rust"));
    document.extend(scip_field(6, 0, &[1]));
    document
}

fn synthetic_scip_document_with_enclosed_reference(
    path: &[u8],
    definition_symbol: &[u8],
    definition_start: u64,
    definition_end: u64,
    target_symbol: &[u8],
    target_start: u64,
    target_end: u64,
) -> Vec<u8> {
    let occurrence = |symbol: &[u8], start: u64, end: u64, definition: bool| {
        let mut range = Vec::new();
        for component in [0_u64, start, 0, end] {
            range.extend(scip_varint(component));
        }
        let mut encoded = scip_field(1, 2, &range);
        encoded.extend(scip_field(2, 2, symbol));
        if definition {
            encoded.extend(scip_field(3, 0, &[1]));
        }
        scip_field(2, 2, &encoded)
    };
    let mut document = scip_field(1, 2, path);
    document.extend(occurrence(
        definition_symbol,
        definition_start,
        definition_end,
        true,
    ));
    document.extend(occurrence(
        target_symbol,
        target_start,
        target_end,
        false,
    ));
    document.extend(scip_field(4, 2, b"rust"));
    document.extend(scip_field(6, 0, &[1]));
    document
}

fn overlay_scope(view: &crate::PinnedWorkspaceView, source_slot: SourceSlotId) -> ScipOverlayScopeIdentity {
    let member = view
        .members()
        .iter()
        .find(|member| member.source_slot() == source_slot)
        .expect("fixture view member");
    ScipOverlayScopeIdentity::new(
        view.connected_workspace(),
        view.view().get(),
        source_slot,
        member.source_epoch(),
        member.generation().get(),
    )
    .expect("positive fixture scope")
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one end-to-end synthetic overlay fixture keeps source, SCIP, publication, and trace assertions together"
)]
fn enclosed_reference_projection_supports_a_bounded_caller_trace() {
    let directory = TempDirectory::new();
    let (store, _) = OwnedSqliteIndex::start(&directory.database(), 123, deadline())
        .expect("owned store should start");
    let repository = RepositoryIdentityDigest::new([1; 32]);
    store
        .register_workspace(repository, 0, deadline())
        .expect("repository should register");
    let source = b"pub fn caller() { target(); }\n";
    let prepared_index = repowitness_application::prepare_rust_index(
        vec![ImmutableRustSource::new(
            RepositoryPath::try_from_bytes(b"src/lib.rs", PATH_LIMITS)
                .expect("fixture path should be valid"),
            source.to_vec().into_boxed_slice(),
        )],
        artifact_identity(),
        RustIndexLimits::default(),
        &AtomicBool::new(false),
        deadline(),
    )
    .expect("source should prepare");
    let source_identity = snapshot_identity();
    let manifest_digest = prepared_index.manifest_digest();
    let prepared_manifest = prepared_index.manifest().clone();
    let generation = store
        .stage(
            0,
            source_identity,
            prepared_index,
            GenerationCoverage::new(1, 0, 0, 0),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("source generation should stage");
    store
        .activate(generation, 0, deadline())
        .expect("source generation should activate");
    let view = store
        .active_workspace_view(
            ConnectedWorkspaceId::for_single_repository(repository),
            workspace_control(),
            deadline(),
        )
        .expect("active view should load")
        .expect("active view should exist");
    let source_slot = view.members()[0].source_slot();
    let definition = b"scip-rust pkg 1 Caller.";
    let target = b"scip-rust pkg 1 Target.";
    let raw = synthetic_scip_document_with_enclosed_reference(
        b"src/lib.rs",
        definition,
        7,
        13,
        target,
        18,
        24,
    );
    let document = repowitness_analysis::decode_scip_overlay_document(
        &raw,
        &prepared_manifest,
        PATH_LIMITS,
        source,
        &AtomicBool::new(false),
        deadline(),
    )
    .expect("SCIP fixture should decode");
    let identity = ScipOverlayIdentityInput::new(
        overlay_scope(&view, source_slot),
        hash_source_snapshot(source_identity, manifest_digest),
        manifest_digest,
        ConfigurationDigest::new([4; 32]),
        ProducerManifestDigest::new([5; 32]),
        reviewed_scip_schema_digest(),
        bounded_scip_importer_digest(),
        hash_scip_input(&raw),
    );
    let overlay = PreparedScipOverlay::try_new(identity, vec![document.clone()])
        .expect("overlay should prepare");
    store
        .stage_scip_overlay(
            view.connected_workspace(),
            view.view(),
            source_slot,
            overlay,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("overlay should publish");
    let connection = Connection::open(directory.database()).expect("database should reopen");
    let removed = connection
        .execute(
            "DELETE FROM scip_enclosed_reference_edges
             WHERE overlay_digest IN (SELECT overlay_digest FROM active_scip_overlays)",
            [],
        )
        .expect("derived edge should be removable for replay coverage");
    assert_eq!(removed, 1);
    drop(connection);
    let replay = PreparedScipOverlay::try_new(identity, vec![document])
        .expect("replayed overlay should prepare");
    store
        .stage_scip_overlay(
            view.connected_workspace(),
            view.view(),
            source_slot,
            replay,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("complete overlay replay should restore derived evidence");
    let reader = OwnedSqliteReader::start(&directory.database(), deadline())
        .expect("reader should start");
    let trace = reader
        .scip_relationship_trace(
            &view,
            source_slot,
            PackageScope::whole_repository(),
            ScipSymbol::try_new(String::from_utf8(target.to_vec()).expect("target UTF-8"))
                .expect("target symbol should validate"),
            repowitness_application::ScipRelationshipTraceDirection::Incoming,
            repowitness_application::ScipRelationshipTraceDepth::try_new(1)
                .expect("depth should validate"),
            repowitness_application::ScipRelationshipTraceMaxEdges::try_new(4)
                .expect("edges should validate"),
            crate::sqlite::ScipRelationshipTraceReadLimits::try_new(4, 4, 1_048_576)
                .expect("trace limits should validate"),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("caller trace should load");
    let crate::ScipRelationshipTraceResult::Found(trace) = trace else {
        panic!("enclosed target should have one caller");
    };
    assert_eq!(trace.edges().len(), 1);
    assert_eq!(
        trace.edges()[0].relationship().evidence(),
        crate::ScipRelationshipEvidenceClass::EnclosedReference
    );
    reader.shutdown(deadline()).expect("reader should stop");
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one end-to-end atomic overlay fixture keeps publication, idempotency, and cancellation evidence together"
)]
fn scip_overlay_writer_is_atomic_idempotent_and_cancellation_preserves_pointer() {
    let directory = TempDirectory::new();
    let (store, _) = OwnedSqliteIndex::start(&directory.database(), 123, deadline())
        .expect("owned store should start");
    let repository = RepositoryIdentityDigest::new([1; 32]);
    store
        .register_workspace(repository, 0, deadline())
        .expect("repository should register");

    let source = b"pub fn stable_scip() {}\n";
    let prepared_index = prepared("scip");
    let prepared_manifest = prepared_index.manifest_digest();
    let raw_document = synthetic_scip_document();
    let cancelled = AtomicBool::new(false);
    let document = repowitness_analysis::decode_scip_overlay_document(
        &raw_document,
        prepared_index.manifest(),
        PATH_LIMITS,
        source,
        &cancelled,
        deadline(),
    )
    .expect("synthetic document should decode against exact source");
    assert_eq!(document.occurrences().len(), 1);
    assert!(document.occurrences()[0].symbol().is_none());
    let source_identity = snapshot_identity();
    let generation = store
        .stage(
            0,
            source_identity,
            prepared_index,
            GenerationCoverage::new(2, 0, 0, 0),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("source generation should stage");
    store
        .activate(generation, 0, deadline())
        .expect("source generation should activate");
    let view = store
        .active_workspace_view(
            ConnectedWorkspaceId::for_single_repository(repository),
            workspace_control(),
            deadline(),
        )
        .expect("active view should load")
        .expect("single repository view should publish");
    let source_slot = view.members()[0].source_slot();
    let workspace = view.connected_workspace();
    let reader =
        OwnedSqliteReader::start(&directory.database(), deadline()).expect("reader should start");
    let no_overlay_trace = reader
        .scip_relationship_trace(
            &view,
            source_slot,
            PackageScope::whole_repository(),
            ScipSymbol::try_new("scip-rust pkg 1 Unproduced.".to_owned())
                .expect("fixture symbol should validate"),
            repowitness_application::ScipRelationshipTraceDirection::Outgoing,
            repowitness_application::ScipRelationshipTraceDepth::try_new(1)
                .expect("depth should validate"),
            repowitness_application::ScipRelationshipTraceMaxEdges::try_new(1)
                .expect("edge limit should validate"),
            crate::sqlite::ScipRelationshipTraceReadLimits::try_new(1, 2, 1_048_576)
                .expect("trace limits should validate"),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("no-overlay trace should load categorically");
    assert!(matches!(
        no_overlay_trace,
        crate::ScipRelationshipTraceResult::NotProduced
    ));
    reader.shutdown(deadline()).expect("reader should stop");
    let overlay_identity = ScipOverlayIdentityInput::new(
        overlay_scope(&view, source_slot),
        hash_source_snapshot(source_identity, prepared_manifest),
        prepared_manifest,
        ConfigurationDigest::new([4; 32]),
        ProducerManifestDigest::new([5; 32]),
        reviewed_scip_schema_digest(),
        bounded_scip_importer_digest(),
        hash_scip_input(&raw_document),
    );
    let overlay = PreparedScipOverlay::try_new(overlay_identity, vec![document.clone()])
        .expect("decoded overlay should prepare");
    let replay_overlay = PreparedScipOverlay::try_new(overlay_identity, vec![document])
        .expect("decoded overlay should prepare");

    let digest = store
        .stage_scip_overlay(
            workspace,
            view.view(),
            source_slot,
            overlay,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("exact overlay should stage and activate");
    assert_eq!(
        store
            .stage_scip_overlay(
                workspace,
                view.view(),
                source_slot,
                replay_overlay,
                Arc::new(AtomicBool::new(false)),
                deadline(),
            )
            .expect("exact complete overlay should replay"),
        digest
    );
    let connection = Connection::open(directory.database()).expect("database should be readable");
    let stored: (String, i64, i64, i64, Option<Vec<u8>>) = connection
        .query_row(
            "SELECT receipt.lifecycle_state, receipt.document_count,
                    receipt.occurrence_count, receipt.relationship_count,
                    occurrence.symbol
             FROM active_scip_overlays AS active
             JOIN scip_overlay_receipts AS receipt
               ON receipt.overlay_digest = active.overlay_digest
             JOIN scip_overlay_occurrences AS occurrence
               ON occurrence.overlay_digest = receipt.overlay_digest",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
        )
        .expect("complete active overlay should be readable");
    assert_eq!(stored, ("complete".to_owned(), 1, 1, 0, None));
    drop(connection);
    let reader =
        OwnedSqliteReader::start(&directory.database(), deadline()).expect("reader should start");
    let status = reader
        .scip_overlay_status(
            &view,
            source_slot,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("exact view-scoped status should load");
    assert_eq!(
        status,
        crate::ScipOverlayAvailability::Complete(crate::ScipOverlaySummary::new(
            digest,
            source_slot,
            1,
            1,
            0,
        ))
    );
    drop(reader);

    let cancellation_index = prepared("scip");
    let second_raw_document = synthetic_scip_document();
    let second_document = repowitness_analysis::decode_scip_overlay_document(
        &second_raw_document,
        cancellation_index.manifest(),
        PATH_LIMITS,
        source,
        &AtomicBool::new(false),
        deadline(),
    )
    .expect("cancellation fixture document should decode");
    let cancelled_overlay = PreparedScipOverlay::try_new(
        ScipOverlayIdentityInput::new(
            overlay_scope(&view, source_slot),
            hash_source_snapshot(snapshot_identity(), cancellation_index.manifest_digest()),
            cancellation_index.manifest_digest(),
            ConfigurationDigest::new([4; 32]),
            ProducerManifestDigest::new([5; 32]),
            reviewed_scip_schema_digest(),
            bounded_scip_importer_digest(),
            hash_scip_input(b"different-scip-input"),
        ),
        vec![second_document],
    )
    .expect("changed input should prepare");
    let cancelled = Arc::new(AtomicBool::new(true));
    assert_eq!(
        store.stage_scip_overlay(
            workspace,
            view.view(),
            source_slot,
            cancelled_overlay,
            cancelled,
            deadline(),
        ),
        Err(SqliteStoreError::Cancelled)
    );
    let connection = Connection::open(directory.database()).expect("database should reopen");
    let active_digest: Vec<u8> = connection
        .query_row(
            "SELECT overlay_digest FROM active_scip_overlays",
            [],
            |row| row.get(0),
        )
        .expect("prior pointer should remain readable");
    assert_eq!(active_digest, digest.as_bytes());
}

#[test]
#[allow(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    reason = "the source-slot isolation fixture retains every exact package-scope and pinned-view assertion together"
)]
fn scip_symbol_evidence_is_pinned_package_scoped_and_cross_file() {
    let directory = TempDirectory::new();
    let (store, _) = OwnedSqliteIndex::start(&directory.database(), 123, deadline())
        .expect("owned store should start");
    let repository = RepositoryIdentityDigest::new([0xA4; 32]);
    store
        .register_workspace(repository, 0, deadline())
        .expect("repository should register");
    let source_identity = workspace_snapshot_identity(repository, 0x91);
    let prepared_index = prepared("scip_evidence");
    let generation = store
        .stage(
            0,
            source_identity,
            prepared_index,
            GenerationCoverage::new(2, 0, 0, 0),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("source generation should stage");
    store
        .activate(generation, 0, deadline())
        .expect("source generation should activate");
    let view = store
        .active_workspace_view(
            ConnectedWorkspaceId::for_single_repository(repository),
            workspace_control(),
            deadline(),
        )
        .expect("workspace view should load")
        .expect("workspace view should publish");
    let source_slot = view.members()[0].source_slot();
    let workspace = view.connected_workspace();

    let source_symbol = b"scip-rust pkg 1 Source.";
    let target_symbol = format!("scip-rust pkg 1 Target{}#", "\u{0001}".repeat(64)).into_bytes();
    let second_target_symbol = b"scip-rust pkg 1 Other#";
    let lib_raw = synthetic_scip_document_with_relationships(
        b"src/lib.rs",
        source_symbol,
        &[target_symbol.as_slice(), second_target_symbol],
        7,
        27,
    );
    let model_raw = synthetic_scip_document_with_relationships(
        b"src/model.rs",
        target_symbol.as_slice(),
        &[source_symbol],
        11,
        16,
    );
    let fixture_index = prepared("scip_evidence");
    let lib_source = b"pub fn stable_scip_evidence() {}\n";
    let model_source = b"pub struct Model;\n";
    let lib_document = repowitness_analysis::decode_scip_overlay_document(
        &lib_raw,
        fixture_index.manifest(),
        PATH_LIMITS,
        lib_source,
        &AtomicBool::new(false),
        deadline(),
    )
    .expect("source evidence document should decode");
    let model_document = repowitness_analysis::decode_scip_overlay_document(
        &model_raw,
        fixture_index.manifest(),
        PATH_LIMITS,
        model_source,
        &AtomicBool::new(false),
        deadline(),
    )
    .expect("target evidence document should decode");
    let overlay = PreparedScipOverlay::try_new(
        ScipOverlayIdentityInput::new(
            overlay_scope(&view, source_slot),
            hash_source_snapshot(source_identity, fixture_index.manifest_digest()),
            fixture_index.manifest_digest(),
            ConfigurationDigest::new([4; 32]),
            ProducerManifestDigest::new([5; 32]),
            reviewed_scip_schema_digest(),
            bounded_scip_importer_digest(),
            hash_scip_input(b"two-document-evidence"),
        ),
        vec![lib_document, model_document],
    )
    .expect("two-document evidence should prepare");
    store
        .stage_scip_overlay(
            workspace,
            view.view(),
            source_slot,
            overlay,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("overlay should activate");
    let reader =
        OwnedSqliteReader::start(&directory.database(), deadline()).expect("reader should start");
    let source = ScipSymbol::try_new(String::from_utf8(source_symbol.to_vec()).expect("UTF-8"))
        .expect("source symbol should validate");
    let target = ScipSymbol::try_new(String::from_utf8(target_symbol.to_vec()).expect("UTF-8"))
        .expect("target symbol should validate");
    let source_result = reader
        .scip_symbol_evidence(
            &view,
            source_slot,
            PackageScope::whole_repository(),
            source,
            crate::ScipEvidenceReadLimits::default(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("source evidence should load");
    let crate::ScipSymbolEvidenceResult::Found(source_evidence) = source_result else {
        panic!("source symbol should have exact evidence");
    };
    assert_eq!(source_evidence.occurrences().len(), 1);
    assert_eq!(source_evidence.relationships().len(), 3);
    assert_eq!(source_evidence.occurrences()[0].path().as_bytes(), b"src/lib.rs");
    assert_eq!(
        source_evidence.relationships()[0].direction(),
        crate::ScipRelationshipDirection::Outgoing
    );
    let target_result = reader
        .scip_symbol_evidence(
            &view,
            source_slot,
            PackageScope::whole_repository(),
            target,
            crate::ScipEvidenceReadLimits::default(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("target evidence should load");
    let crate::ScipSymbolEvidenceResult::Found(target_evidence) = target_result else {
        panic!("target symbol should have exact cross-file evidence");
    };
    assert_eq!(target_evidence.occurrences().len(), 1);
    assert_eq!(target_evidence.relationships().len(), 2);
    assert_eq!(target_evidence.occurrences()[0].path().as_bytes(), b"src/model.rs");
    assert_eq!(
        target_evidence.relationships()[0].direction(),
        crate::ScipRelationshipDirection::Incoming
    );
    let excluded_scope = PackageScope::try_explicit_root_bytes([b"src/unmatched.rs"], PATH_LIMITS)
        .expect("exact package root should validate");
    let excluded = reader
        .scip_symbol_evidence(
            &view,
            source_slot,
            excluded_scope,
            ScipSymbol::try_new(String::from_utf8(source_symbol.to_vec()).expect("UTF-8"))
                .expect("source symbol should validate"),
            crate::ScipEvidenceReadLimits::default(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
    )
    .expect("scoped evidence should load");
    assert!(matches!(excluded, crate::ScipSymbolEvidenceResult::NoMatch(_)));

    let outgoing_trace = reader
        .scip_relationship_trace(
            &view,
            source_slot,
            PackageScope::whole_repository(),
            ScipSymbol::try_new(String::from_utf8(source_symbol.to_vec()).expect("UTF-8"))
                .expect("source symbol should validate"),
            repowitness_application::ScipRelationshipTraceDirection::Outgoing,
            repowitness_application::ScipRelationshipTraceDepth::try_new(1)
                .expect("depth should validate"),
            repowitness_application::ScipRelationshipTraceMaxEdges::try_new(1)
                .expect("edge limit should validate"),
            crate::sqlite::ScipRelationshipTraceReadLimits::try_new(1, 2, 1_048_576)
                .expect("trace limits should validate"),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("outgoing producer trace should load");
    let crate::ScipRelationshipTraceResult::Found(outgoing_trace) = outgoing_trace else {
        panic!("source should begin one producer-declared trace");
    };
    assert_eq!(outgoing_trace.edges().len(), 1);
    assert_eq!(outgoing_trace.edges()[0].document_ordinal(), 0);
    assert_eq!(outgoing_trace.edges()[0].relationship_ordinal(), 0);
    assert_eq!(outgoing_trace.edges()[0].depth(), 1);
    assert_eq!(
        outgoing_trace.edges()[0].relationship().target().as_str(),
        String::from_utf8(target_symbol.to_vec()).expect("UTF-8")
    );
    assert_eq!(outgoing_trace.visited_symbols(), 2);
    assert_eq!(outgoing_trace.unexpanded_frontier_symbols(), 3);
    assert!(outgoing_trace.depth_limit_reached());
    assert!(outgoing_trace.edge_limit_reached());

    let depth_limited_trace = reader
        .scip_relationship_trace(
            &view,
            source_slot,
            PackageScope::whole_repository(),
            ScipSymbol::try_new(String::from_utf8(source_symbol.to_vec()).expect("UTF-8"))
                .expect("source symbol should validate"),
            repowitness_application::ScipRelationshipTraceDirection::Outgoing,
            repowitness_application::ScipRelationshipTraceDepth::try_new(1)
                .expect("depth should validate"),
            repowitness_application::ScipRelationshipTraceMaxEdges::try_new(256)
                .expect("edge limit should validate"),
            crate::sqlite::ScipRelationshipTraceReadLimits::default(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("depth-limited producer trace should load");
    let crate::ScipRelationshipTraceResult::Found(depth_limited_trace) = depth_limited_trace
    else {
        panic!("source should begin one depth-limited producer trace");
    };
    assert_eq!(depth_limited_trace.edges().len(), 2);
    assert_eq!(depth_limited_trace.visited_symbols(), 3);
    assert_eq!(depth_limited_trace.unexpanded_frontier_symbols(), 2);
    assert!(depth_limited_trace.depth_limit_reached());
    assert!(!depth_limited_trace.edge_limit_reached());

    let cycle_trace = reader
        .scip_relationship_trace(
            &view,
            source_slot,
            PackageScope::whole_repository(),
            ScipSymbol::try_new(String::from_utf8(source_symbol.to_vec()).expect("UTF-8"))
                .expect("source symbol should validate"),
            repowitness_application::ScipRelationshipTraceDirection::Outgoing,
            repowitness_application::ScipRelationshipTraceDepth::try_new(2)
                .expect("depth should validate"),
            repowitness_application::ScipRelationshipTraceMaxEdges::try_new(256)
                .expect("edge limit should validate"),
            crate::sqlite::ScipRelationshipTraceReadLimits::default(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("cyclic producer trace should load");
    let crate::ScipRelationshipTraceResult::Found(cycle_trace) = cycle_trace else {
        panic!("source should begin one cyclic producer trace");
    };
    assert_eq!(cycle_trace.edges().len(), 3);
    assert_eq!(cycle_trace.visited_symbols(), 3);
    assert_eq!(cycle_trace.edges()[2].depth(), 2);
    assert_eq!(
        cycle_trace.edges()[2].relationship().target().as_str(),
        String::from_utf8(source_symbol.to_vec()).expect("UTF-8")
    );

    let edge_limited_cycle = reader
        .scip_relationship_trace(
            &view,
            source_slot,
            PackageScope::whole_repository(),
            ScipSymbol::try_new(String::from_utf8(source_symbol.to_vec()).expect("UTF-8"))
                .expect("source symbol should validate"),
            repowitness_application::ScipRelationshipTraceDirection::Outgoing,
            repowitness_application::ScipRelationshipTraceDepth::try_new(2)
                .expect("depth should validate"),
            repowitness_application::ScipRelationshipTraceMaxEdges::try_new(2)
                .expect("edge limit should validate"),
            crate::sqlite::ScipRelationshipTraceReadLimits::try_new(2, 3, 1_048_576)
                .expect("trace limits should validate"),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("edge-limited cyclic trace should load");
    let crate::ScipRelationshipTraceResult::Found(edge_limited_cycle) = edge_limited_cycle
    else {
        panic!("source should begin one edge-limited cyclic trace");
    };
    assert_eq!(edge_limited_cycle.edges().len(), 2);
    assert_eq!(edge_limited_cycle.visited_symbols(), 3);
    assert_eq!(edge_limited_cycle.unexpanded_frontier_symbols(), 2);
    assert!(edge_limited_cycle.edge_limit_reached());

    let symbol_limited_trace = reader
        .scip_relationship_trace(
            &view,
            source_slot,
            PackageScope::whole_repository(),
            ScipSymbol::try_new(String::from_utf8(source_symbol.to_vec()).expect("UTF-8"))
                .expect("source symbol should validate"),
            repowitness_application::ScipRelationshipTraceDirection::Outgoing,
            repowitness_application::ScipRelationshipTraceDepth::try_new(1)
                .expect("depth should validate"),
            repowitness_application::ScipRelationshipTraceMaxEdges::try_new(256)
                .expect("edge limit should validate"),
            crate::sqlite::ScipRelationshipTraceReadLimits::try_new(256, 1, 1_048_576)
                .expect("trace limits should validate"),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("symbol-limited producer trace should load");
    let crate::ScipRelationshipTraceResult::Found(symbol_limited_trace) = symbol_limited_trace
    else {
        panic!("source should begin one symbol-limited producer trace");
    };
    assert_eq!(symbol_limited_trace.edges().len(), 2);
    assert_eq!(symbol_limited_trace.visited_symbols(), 1);
    assert_eq!(symbol_limited_trace.unexpanded_frontier_symbols(), 2);
    assert!(symbol_limited_trace.depth_limit_reached());
    assert!(symbol_limited_trace.symbol_limit_reached());

    let output_limited_trace = reader
        .scip_relationship_trace(
            &view,
            source_slot,
            PackageScope::whole_repository(),
            ScipSymbol::try_new(String::from_utf8(source_symbol.to_vec()).expect("UTF-8"))
                .expect("source symbol should validate"),
            repowitness_application::ScipRelationshipTraceDirection::Outgoing,
            repowitness_application::ScipRelationshipTraceDepth::try_new(1)
                .expect("depth should validate"),
            repowitness_application::ScipRelationshipTraceMaxEdges::try_new(256)
                .expect("edge limit should validate"),
            crate::sqlite::ScipRelationshipTraceReadLimits::try_new(256, 257, 500)
                .expect("trace limits should validate"),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("output-limited producer trace should load");
    let crate::ScipRelationshipTraceResult::Found(output_limited_trace) = output_limited_trace
    else {
        panic!("source should begin one output-limited producer trace");
    };
    assert!(output_limited_trace.edges().is_empty());
    assert_eq!(output_limited_trace.output_bytes(), 0);
    assert_eq!(output_limited_trace.unexpanded_frontier_symbols(), 2);
    assert!(output_limited_trace.depth_limit_reached());
    assert!(output_limited_trace.output_limit_reached());

    let exact_cap_trace = reader
        .scip_relationship_trace(
            &view,
            source_slot,
            PackageScope::whole_repository(),
            ScipSymbol::try_new(String::from_utf8(source_symbol.to_vec()).expect("UTF-8"))
                .expect("source symbol should validate"),
            repowitness_application::ScipRelationshipTraceDirection::Outgoing,
            repowitness_application::ScipRelationshipTraceDepth::try_new(2)
                .expect("depth should validate"),
            repowitness_application::ScipRelationshipTraceMaxEdges::try_new(3)
                .expect("edge limit should validate"),
            crate::sqlite::ScipRelationshipTraceReadLimits::try_new(3, 4, 1_048_576)
                .expect("trace limits should validate"),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("exactly full incoming trace should load");
    let crate::ScipRelationshipTraceResult::Found(exact_cap_trace) = exact_cap_trace else {
        panic!("source should begin one exactly full outgoing trace");
    };
    assert_eq!(exact_cap_trace.edges().len(), 3);
    assert!(!exact_cap_trace.edge_limit_reached());
    assert!(!exact_cap_trace.depth_limit_reached());

    let incoming_trace = reader
        .scip_relationship_trace(
            &view,
            source_slot,
            PackageScope::whole_repository(),
            ScipSymbol::try_new(String::from_utf8(target_symbol.to_vec()).expect("UTF-8"))
                .expect("target symbol should validate"),
            repowitness_application::ScipRelationshipTraceDirection::Incoming,
            repowitness_application::ScipRelationshipTraceDepth::try_new(2)
                .expect("depth should validate"),
            repowitness_application::ScipRelationshipTraceMaxEdges::try_new(256)
                .expect("edge limit should validate"),
            crate::sqlite::ScipRelationshipTraceReadLimits::default(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("incoming producer trace should load");
    let crate::ScipRelationshipTraceResult::Found(incoming_trace) = incoming_trace else {
        panic!("target should begin one incoming producer trace");
    };
    assert_eq!(incoming_trace.edges().len(), 2);
    assert_eq!(
        incoming_trace.edges()[0].relationship().source().as_str(),
        String::from_utf8(source_symbol.to_vec()).expect("UTF-8")
    );
    assert!(!incoming_trace.depth_limit_reached());

    let no_relationships = reader
        .scip_relationship_trace(
            &view,
            source_slot,
            PackageScope::whole_repository(),
            ScipSymbol::try_new("scip-rust pkg 1 Unrelated.".to_owned())
                .expect("unrelated symbol should validate"),
            repowitness_application::ScipRelationshipTraceDirection::Outgoing,
            repowitness_application::ScipRelationshipTraceDepth::try_new(1)
                .expect("depth should validate"),
            repowitness_application::ScipRelationshipTraceMaxEdges::try_new(256)
                .expect("edge limit should validate"),
            crate::sqlite::ScipRelationshipTraceReadLimits::default(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("no-relationship result should load");
    assert!(matches!(
        no_relationships,
        crate::ScipRelationshipTraceResult::NoRelationships(_)
    ));

    let scoped_no_relationships = reader
        .scip_relationship_trace(
            &view,
            source_slot,
            PackageScope::try_explicit_root_bytes([b"src/model.rs"], PATH_LIMITS)
                .expect("exact package scope should validate"),
            ScipSymbol::try_new(String::from_utf8(source_symbol.to_vec()).expect("UTF-8"))
                .expect("source symbol should validate"),
            repowitness_application::ScipRelationshipTraceDirection::Outgoing,
            repowitness_application::ScipRelationshipTraceDepth::try_new(1)
                .expect("depth should validate"),
            repowitness_application::ScipRelationshipTraceMaxEdges::try_new(256)
                .expect("edge limit should validate"),
            crate::sqlite::ScipRelationshipTraceReadLimits::default(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("scoped no-relationship result should load");
    assert!(matches!(
        scoped_no_relationships,
        crate::ScipRelationshipTraceResult::NoRelationships(_)
    ));
    reader.shutdown(deadline()).expect("reader should stop");

    let repository_identity = RepositoryIdentityTextV1::encode(repository).into_string();
    let searched = crate::search_local_symbols(
        crate::LocalSymbolSearchRequest::new(
            &directory.database(),
            &repository_identity,
            "stable_scip_evidence",
            repowitness_application::SymbolSearchNameMatch::Exact,
        ),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("typed declaration search should return the source receipt");
    assert_eq!(searched.connected_workspace(), workspace);
    assert_eq!(searched.workspace_view(), view.view().get());
    assert_eq!(searched.source_slot(), source_slot);
    let receipt = searched
        .evidence()
        .as_slice()
        .first()
        .expect("exact declaration search should return one receipt");
    let repowitness_domain::EvidenceLocation::SymbolOccurrence(occurrence) =
        receipt.identity().location()
    else {
        panic!("typed declaration search must return a symbol occurrence");
    };
    assert_eq!(receipt.identity().path().as_bytes(), b"src/lib.rs");
    let snapshot_sha256 = scip_lower_hex(searched.snapshot().as_bytes());
    let artifact_sha256 = scip_lower_hex(occurrence.artifact_digest().as_bytes());
    let lib_content_sha256 = scip_lower_hex(receipt.identity().content_digest().as_bytes());
    let fact_ordinal = occurrence.fact_ordinal();
    let name_span = occurrence.name_span();
    let resolved = crate::resolve_local_scip_symbol(
        crate::LocalScipSymbolResolveRequest::new(
            &directory.database(),
            &repository_identity,
            crate::LocalScipSymbolResolveSelectorText::new(
                &snapshot_sha256,
                searched.generation().get(),
                "rwp1:h:7372632F6C69622E7273",
                &lib_content_sha256,
                &artifact_sha256,
                fact_ordinal,
                (name_span.start().get(), name_span.end().get()),
            ),
        )
        .with_exact_view(view.view().get())
        .expect("exact view should validate"),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("local exact syntax resolution should preserve the selected overlay symbol");
    assert!(matches!(
        resolved.output(),
        crate::ScipSyntaxSymbolResolution::Exact(symbol)
            if symbol.as_str() == std::str::from_utf8(source_symbol).expect("fixture symbol is UTF-8")
    ));
    let crate::ScipSyntaxSymbolResolution::Exact(resolved_symbol) = resolved.into_output() else {
        panic!("the exact syntax receipt must resolve to one provider symbol");
    };
    let local_result = crate::read_local_scip_evidence(
        crate::LocalScipEvidenceReadRequest::new(
            &directory.database(),
            &repository_identity,
            PackageScope::whole_repository(),
            resolved_symbol,
        )
        .with_exact_view(view.view().get())
        .expect("exact view should validate"),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("resolved provider symbol should retrieve the same exact evidence");
    assert!(matches!(
        local_result.output(),
        crate::ScipSymbolEvidenceResult::Found(evidence)
            if evidence.occurrences().len() == 1 && evidence.relationships().len() == 3
    ));
    store.shutdown(deadline()).expect("store should stop");
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "the retention fixture retains its source-root, overlay, sweep, and reopen assertions together"
)]
fn retention_sweeps_an_expired_scip_overlay_with_its_source_generation() {
    let directory = TempDirectory::new();
    let (store, _) = OwnedSqliteIndex::start(&directory.database(), 123, deadline())
        .expect("owned store should start");
    let repository = RepositoryIdentityDigest::new([0xB4; 32]);
    store
        .register_workspace(repository, 0, deadline())
        .expect("repository should register");

    let first_source_identity = workspace_snapshot_identity(repository, 0x81);
    let first_index = prepared("scip_retention_first");
    let first_generation = store
        .stage(
            0,
            first_source_identity,
            first_index,
            GenerationCoverage::new(2, 0, 0, 0),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("first generation should stage");
    store
        .activate(first_generation, 0, deadline())
        .expect("first generation should activate");
    let first_view = store
        .active_workspace_view(
            ConnectedWorkspaceId::for_single_repository(repository),
            workspace_control(),
            deadline(),
        )
        .expect("first workspace view should load")
        .expect("first workspace view should publish");
    let source_slot = first_view.members()[0].source_slot();
    let workspace = first_view.connected_workspace();

    for (epoch, salt, suffix) in [
        (1_u64, 0x82_u8, "scip_retention_second"),
        (2_u64, 0x83_u8, "scip_retention_third"),
    ] {
        store
            .advance_source_epoch(repository, epoch - 1, epoch, deadline())
            .expect("source epoch should advance");
        let generation = store
            .stage(
                epoch,
                workspace_snapshot_identity(repository, salt),
                prepared(suffix),
                GenerationCoverage::new(2, 0, 0, 0),
                Arc::new(AtomicBool::new(false)),
                deadline(),
            )
            .expect("successor generation should stage");
        store
            .activate(generation, epoch, deadline())
            .expect("successor generation should activate");
    }

    let policy = GenerationRetentionPolicy::try_new(
        1,
        RetentionLimits::default(),
        RetentionPins::default(),
    )
    .expect("retention policy should validate");
    let plan_before_overlay = store
        .plan_generation_retention(RetentionPlanRequest::new(
            policy.clone(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        ))
        .expect("pre-overlay retention plan should complete");

    let raw_document = synthetic_scip_document();
    let source = b"pub fn stable_scip_retention_first() {}\n";
    let overlay_index = prepared("scip_retention_first");
    let document = repowitness_analysis::decode_scip_overlay_document(
        &raw_document,
        overlay_index.manifest(),
        PATH_LIMITS,
        source,
        &AtomicBool::new(false),
        deadline(),
    )
    .expect("first-generation SCIP document should decode");
    let overlay_identity = ScipOverlayIdentityInput::new(
        overlay_scope(&first_view, source_slot),
        hash_source_snapshot(first_source_identity, overlay_index.manifest_digest()),
        overlay_index.manifest_digest(),
        ConfigurationDigest::new([4; 32]),
        ProducerManifestDigest::new([5; 32]),
        reviewed_scip_schema_digest(),
        bounded_scip_importer_digest(),
        hash_scip_input(&raw_document),
    );
    let current_only_overlay = PreparedScipOverlay::try_new(overlay_identity, vec![document.clone()])
        .expect("first-generation overlay should prepare");
    assert_eq!(
        store.stage_current_scip_overlay(
            workspace,
            first_view.view(),
            source_slot,
            current_only_overlay,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        ),
        Err(SqliteStoreError::InvalidWorkspaceView),
        "a contained local import must reject a view that was superseded before its writer fence"
    );
    let overlay = PreparedScipOverlay::try_new(overlay_identity, vec![document])
        .expect("first-generation overlay should prepare");
    let overlay_digest = store
        .stage_scip_overlay(
            workspace,
            first_view.view(),
            source_slot,
            overlay,
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )
        .expect("published historical view should accept exact overlay evidence");
    let connection = Connection::open(directory.database()).expect("database should reopen");
    connection
        .execute(
            "INSERT INTO scip_enclosed_reference_edges(
                overlay_digest, document_ordinal, relationship_ordinal,
                source_symbol, target_symbol, kinds
             ) VALUES (?1, 0, 0, ?2, ?3, 1)",
            params![
                overlay_digest.as_bytes().as_slice(),
                b"scip-rust pkg 1 Caller.".as_slice(),
                b"scip-rust pkg 1 Target.".as_slice(),
            ],
        )
        .expect("derived relationship should be insertable for retention coverage");
    drop(connection);
    assert_eq!(
        store.apply_generation_retention(RetentionApplyRequest::new(
            policy.clone(),
            plan_before_overlay.plan_digest(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        )),
        Err(SqliteStoreError::RetentionPlanStale),
        "a completed overlay must invalidate a previously computed retention plan"
    );
    let plan = store
        .plan_generation_retention(RetentionPlanRequest::new(
            policy.clone(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        ))
        .expect("retention plan should include expired generations");
    assert!(plan.candidate_generations().contains(&first_generation));
    let outcome = store
        .apply_generation_retention(RetentionApplyRequest::new(
            policy,
            plan.plan_digest(),
            Arc::new(AtomicBool::new(false)),
            deadline(),
        ))
        .expect("retention should sweep the expired overlay atomically");
    assert!(outcome.generation_count() >= 1);

    let connection = Connection::open(directory.database()).expect("database should reopen");
    let remaining: (i64, i64, i64, i64) = connection
        .query_row(
            "SELECT
                 (SELECT count(*) FROM scip_overlay_receipts
                  WHERE overlay_digest = ?1),
                 (SELECT count(*) FROM active_scip_overlays
                  WHERE overlay_digest = ?1),
                 (SELECT count(*) FROM scip_enclosed_reference_edges
                  WHERE overlay_digest = ?1),
                 (SELECT count(*) FROM retention_scip_overlay_garbage)",
            [overlay_digest.as_bytes().as_slice()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("retention result should be readable");
    assert_eq!(remaining, (0, 0, 0, 0));
    drop(connection);
    store.shutdown(deadline()).expect("store should stop");
}
