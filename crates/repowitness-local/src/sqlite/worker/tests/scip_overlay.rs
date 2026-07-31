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

fn synthetic_scip_document_with_evidence(
    path: &[u8],
    occurrence_symbol: &[u8],
    relationship_target: Option<&[u8]>,
) -> Vec<u8> {
    let mut legacy_range = Vec::new();
    for component in [0_u64, 0, 0, 1] {
        legacy_range.extend(scip_varint(component));
    }
    let mut occurrence = scip_field(1, 2, &legacy_range);
    occurrence.extend(scip_field(2, 2, occurrence_symbol));
    occurrence.extend(scip_field(3, 0, &[1]));
    let mut document = scip_field(1, 2, path);
    document.extend(scip_field(2, 2, &occurrence));
    if let Some(target) = relationship_target {
        let mut relationship = scip_field(1, 2, target);
        relationship.extend(scip_field(2, 0, &[1]));
        let mut symbol_information = scip_field(1, 2, occurrence_symbol);
        symbol_information.extend(scip_field(4, 2, &relationship));
        document.extend(scip_field(3, 2, &symbol_information));
    }
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
    let target_symbol = b"scip-rust pkg 1 Target#";
    let lib_raw = synthetic_scip_document_with_evidence(
        b"src/lib.rs",
        source_symbol,
        Some(target_symbol),
    );
    let model_raw = synthetic_scip_document_with_evidence(b"src/model.rs", target_symbol, None);
    let fixture_index = prepared("scip_evidence");
    let lib_document = repowitness_analysis::decode_scip_overlay_document(
        &lib_raw,
        fixture_index.manifest(),
        PATH_LIMITS,
        b"pub fn stable_scip_evidence() {}\n",
        &AtomicBool::new(false),
        deadline(),
    )
    .expect("source evidence document should decode");
    let model_document = repowitness_analysis::decode_scip_overlay_document(
        &model_raw,
        fixture_index.manifest(),
        PATH_LIMITS,
        b"pub struct Model;\n",
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
    assert_eq!(source_evidence.relationships().len(), 1);
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
    assert_eq!(target_evidence.relationships().len(), 1);
    assert_eq!(target_evidence.occurrences()[0].path().as_bytes(), b"src/model.rs");
    assert_eq!(
        target_evidence.relationships()[0].direction(),
        crate::ScipRelationshipDirection::Incoming
    );
    let excluded_scope = PackageScope::try_explicit_root_bytes([b"src/model.rs"], PATH_LIMITS)
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
    reader.shutdown(deadline()).expect("reader should stop");

    let repository_identity = RepositoryIdentityTextV1::encode(repository).into_string();
    let local_result = crate::read_local_scip_evidence(
        crate::LocalScipEvidenceReadRequest::new(
            &directory.database(),
            &repository_identity,
            PackageScope::whole_repository(),
            ScipSymbol::try_new(String::from_utf8(source_symbol.to_vec()).expect("UTF-8"))
                .expect("source symbol should validate"),
        )
        .with_exact_view(view.view().get())
        .expect("exact view should validate"),
        Arc::new(AtomicBool::new(false)),
    )
    .expect("local facade should preserve the exact evidence result");
    assert!(matches!(
        local_result.output(),
        crate::ScipSymbolEvidenceResult::Found(evidence)
            if evidence.occurrences().len() == 1 && evidence.relationships().len() == 1
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
    let remaining: (i64, i64, i64) = connection
        .query_row(
            "SELECT
                 (SELECT count(*) FROM scip_overlay_receipts
                  WHERE overlay_digest = ?1),
                 (SELECT count(*) FROM active_scip_overlays
                  WHERE overlay_digest = ?1),
                 (SELECT count(*) FROM retention_scip_overlay_garbage)",
            [overlay_digest.as_bytes().as_slice()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("retention result should be readable");
    assert_eq!(remaining, (0, 0, 0));
    drop(connection);
    store.shutdown(deadline()).expect("store should stop");
}
