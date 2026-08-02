use std::collections::{BTreeSet, VecDeque};

use repowitness_application::RustSourceSnapshotIdentity;
use repowitness_domain::{
    AnalysisSchemaDigest, GitStateDigest, ScipOverlayDigest, ScipRelationshipKinds,
    ScipSymbolRoles, SourceManifestDigest, SourceSlotId, WorktreeStateDigest,
};

use crate::sqlite::{
    ScipEvidenceReadLimits, ScipOccurrenceEvidence, ScipOverlayAvailability, ScipOverlaySummary,
    ScipOverlayImportScope, ScipRelationshipDirection, ScipRelationshipEvidence, ScipSymbolEvidence,
    ScipRelationshipTrace, ScipRelationshipTraceEdge, ScipRelationshipTraceNoRelationships,
    ScipRelationshipTraceReadLimits, ScipRelationshipTraceResult, ScipSymbolEvidenceResult,
    ScipSyntaxSymbolResolution,
};

struct ActiveOverlayRow {
    digest: Vec<u8>,
    documents: i64,
    occurrences: i64,
    relationships: i64,
}

impl OwnedSqliteReader {
    /// Loads the exact completed source member required to admit one SCIP import.
    pub fn scip_import_scope(
        &self,
        view: &PinnedWorkspaceView,
        source_slot: SourceSlotId,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<ScipOverlayImportScope, SqliteStoreError> {
        if !view.members().iter().any(|member| member.source_slot() == source_slot) {
            return Err(SqliteStoreError::InvalidWorkspaceView);
        }
        check_control(&cancelled, deadline)?;
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(
            ReaderCommand::ScipImportScope(Box::new(ScipImportScopeCommand {
                view: view.clone(),
                source_slot,
                cancelled: Arc::clone(&cancelled),
                deadline,
                reply,
            })),
            deadline,
        )?;
        match receive_reply(&receiver, deadline) {
            Ok(scope) => Ok(scope),
            Err(error) => {
                cancelled.store(true, Ordering::Release);
                Err(error)
            }
        }
    }

    /// Reports whether one exact source slot has a complete active SCIP overlay
    /// scoped to the supplied immutable workspace view.
    pub fn scip_overlay_status(
        &self,
        view: &PinnedWorkspaceView,
        source_slot: SourceSlotId,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<ScipOverlayAvailability, SqliteStoreError> {
        if !view.members().iter().any(|member| member.source_slot() == source_slot) {
            return Err(SqliteStoreError::InvalidWorkspaceView);
        }
        check_control(&cancelled, deadline)?;
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(
            ReaderCommand::ScipOverlayStatus(Box::new(ScipOverlayStatusCommand {
                view: view.clone(),
                source_slot,
                cancelled: Arc::clone(&cancelled),
                deadline,
                reply,
            })),
            deadline,
        )?;
        match receive_reply(&receiver, deadline) {
            Ok(status) => Ok(status),
            Err(error) => {
                cancelled.store(true, Ordering::Release);
                Err(error)
            }
        }
    }
}

impl OwnedSqliteReader {
    /// Traces one exact opaque SCIP symbol through one pinned immutable overlay.
    #[allow(
        clippy::too_many_arguments,
        reason = "view, slot, scope, root, traversal profile, limits, and controls are independent trust inputs"
    )]
    pub fn scip_relationship_trace(
        &self,
        view: &PinnedWorkspaceView,
        source_slot: SourceSlotId,
        package_scope: PackageScope,
        root: ScipSymbol,
        direction: ScipRelationshipTraceDirection,
        max_depth: ScipRelationshipTraceDepth,
        max_edges: ScipRelationshipTraceMaxEdges,
        limits: ScipRelationshipTraceReadLimits,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<ScipRelationshipTraceResult, SqliteStoreError> {
        if !view.members().iter().any(|member| member.source_slot() == source_slot)
            || limits.max_edges() != max_edges.get()
        {
            return Err(SqliteStoreError::InvalidScipRelationshipTraceLimits);
        }
        check_control(&cancelled, deadline)?;
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(
            ReaderCommand::ScipRelationshipTrace(Box::new(ScipRelationshipTraceCommand {
                view: view.clone(),
                source_slot,
                package_scope,
                root,
                direction,
                max_depth,
                max_edges,
                limits,
                cancelled: Arc::clone(&cancelled),
                deadline,
                reply,
            })),
            deadline,
        )?;
        match receive_reply(&receiver, deadline) {
            Ok(result) => Ok(result),
            Err(error) => {
                cancelled.store(true, Ordering::Release);
                Err(error)
            }
        }
    }
}

impl OwnedSqliteReader {
    /// Resolves an opaque SCIP symbol only when one selected overlay has exactly one symbol at
    /// the exact indexed syntax identifier span.
    #[allow(
        clippy::too_many_arguments,
        reason = "the complete pinned view, source identity, span, and controls are independent trust inputs"
    )]
    pub fn scip_symbol_at_syntax_span(
        &self,
        view: &PinnedWorkspaceView,
        source_slot: SourceSlotId,
        path: RepositoryPath,
        content: SourceContentDigest,
        name_span: ByteSpan,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<ScipSyntaxSymbolResolution, SqliteStoreError> {
        if !view.members().iter().any(|member| member.source_slot() == source_slot) {
            return Err(SqliteStoreError::InvalidWorkspaceView);
        }
        check_control(&cancelled, deadline)?;
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(
            ReaderCommand::ScipSyntaxSymbol(Box::new(ScipSyntaxSymbolCommand {
                view: view.clone(),
                source_slot,
                path,
                content,
                name_span,
                cancelled: Arc::clone(&cancelled),
                deadline,
                reply,
            })),
            deadline,
        )?;
        match receive_reply(&receiver, deadline) {
            Ok(result) => Ok(result),
            Err(error) => {
                cancelled.store(true, Ordering::Release);
                Err(error)
            }
        }
    }
}

impl OwnedSqliteReader {
    /// Loads exact package-scoped occurrence and relationship evidence for one
    /// opaque SCIP symbol in a selected immutable overlay.
    #[allow(
        clippy::too_many_arguments,
        reason = "view, slot, package scope, symbol, limits, cancellation, and deadline are independent trust inputs"
    )]
    pub fn scip_symbol_evidence(
        &self,
        view: &PinnedWorkspaceView,
        source_slot: SourceSlotId,
        package_scope: PackageScope,
        symbol: ScipSymbol,
        limits: ScipEvidenceReadLimits,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<ScipSymbolEvidenceResult, SqliteStoreError> {
        if !view.members().iter().any(|member| member.source_slot() == source_slot) {
            return Err(SqliteStoreError::InvalidWorkspaceView);
        }
        check_control(&cancelled, deadline)?;
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(
            ReaderCommand::ScipSymbolEvidence(Box::new(ScipSymbolEvidenceCommand {
                view: view.clone(),
                source_slot,
                package_scope,
                symbol,
                limits,
                cancelled: Arc::clone(&cancelled),
                deadline,
                reply,
            })),
            deadline,
        )?;
        match receive_reply(&receiver, deadline) {
            Ok(result) => Ok(result),
            Err(error) => {
                cancelled.store(true, Ordering::Release);
                Err(error)
            }
        }
    }
}

fn execute_scip_overlay_status_command(
    connection: &mut Connection,
    command: &ScipOverlayStatusCommand,
) -> Result<ScipOverlayAvailability, SqliteStoreError> {
    check_control(&command.cancelled, command.deadline)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Deferred)
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    let row = load_active_overlay_summary(&transaction, &command.view, command.source_slot)?;
    check_control(&command.cancelled, command.deadline)?;
    let Some(row) = row else {
        return Ok(ScipOverlayAvailability::NotProduced);
    };
    let digest = ScipOverlayDigest::try_from_slice(&row.digest)
        .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
    let documents =
        u64::try_from(row.documents).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
    let occurrences =
        u64::try_from(row.occurrences).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
    let relationships =
        u64::try_from(row.relationships).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
    Ok(ScipOverlayAvailability::Complete(ScipOverlaySummary::new(
        digest,
        command.source_slot,
        documents,
        occurrences,
        relationships,
    )))
}

fn execute_scip_import_scope_command(
    connection: &mut Connection,
    command: &ScipImportScopeCommand,
) -> Result<ScipOverlayImportScope, SqliteStoreError> {
    check_control(&command.cancelled, command.deadline)?;
    let member = command
        .view
        .members()
        .iter()
        .find(|member| member.source_slot() == command.source_slot)
        .ok_or(SqliteStoreError::InvalidWorkspaceView)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Deferred)
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    let row = transaction
        .query_row(
            "SELECT snapshot.snapshot_digest, snapshot.manifest_digest,
                    snapshot.repository_identity, snapshot.git_state_digest,
                    snapshot.worktree_state_digest, snapshot.configuration_digest,
                    snapshot.producer_manifest_digest, snapshot.analysis_schema_digest,
                    snapshot.canonicalization_version
             FROM workspace_view_members AS member
             JOIN active_workspace_views AS active
               ON active.connected_workspace_id = member.connected_workspace_id
              AND active.workspace_view_id = member.workspace_view_id
             JOIN index_generations AS generation
               ON generation.workspace_id = member.generation_workspace_id
              AND generation.generation_id = member.generation_id
             JOIN source_snapshots AS snapshot
               ON snapshot.snapshot_digest = generation.snapshot_digest
             WHERE member.workspace_view_id = ?1
               AND member.connected_workspace_id = ?2
               AND member.source_slot_id = ?3
               AND member.source_epoch = ?4
               AND member.generation_id = ?5
               AND snapshot.lifecycle_state = 'complete'",
            params![
                command.view.view().get(),
                command.view.connected_workspace().as_bytes().as_slice(),
                command.source_slot.as_bytes().as_slice(),
                i64::try_from(member.source_epoch().get())
                    .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
                member.generation().get(),
            ],
            |row| {
                Ok((
                    row.get::<_, Vec<u8>>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                    row.get::<_, Vec<u8>>(3)?,
                    row.get::<_, Vec<u8>>(4)?,
                    row.get::<_, Vec<u8>>(5)?,
                    row.get::<_, Vec<u8>>(6)?,
                    row.get::<_, Vec<u8>>(7)?,
                    row.get::<_, i64>(8)?,
                ))
            },
        )
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    let snapshot = SourceSnapshotDigest::try_from_slice(&row.0)
        .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
    let manifest = SourceManifestDigest::try_from_slice(&row.1)
        .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
    let repository = RepositoryIdentityDigest::try_from_slice(&row.2)
        .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
    if repository != member.repository() {
        return Err(SqliteStoreError::IntegrityCheckFailed);
    }
    let identity = RustSourceSnapshotIdentity::new_supported_languages(
        repository,
        GitStateDigest::try_from_slice(&row.3)
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
        WorktreeStateDigest::try_from_slice(&row.4)
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
        ConfigurationDigest::try_from_slice(&row.5)
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
        ProducerManifestDigest::try_from_slice(&row.6)
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
        AnalysisSchemaDigest::try_from_slice(&row.7)
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
        u32::try_from(row.8).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
    );
    check_control(&command.cancelled, command.deadline)?;
    Ok(ScipOverlayImportScope::new(
        command.view.connected_workspace(),
        command.view.view(),
        command.source_slot,
        member.source_epoch(),
        member.generation(),
        snapshot,
        manifest,
        identity,
    ))
}

fn execute_scip_symbol_evidence_command(
    connection: &mut Connection,
    command: &ScipSymbolEvidenceCommand,
) -> Result<ScipSymbolEvidenceResult, SqliteStoreError> {
    check_control(&command.cancelled, command.deadline)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Deferred)
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    let Some(row) = load_active_overlay_summary(&transaction, &command.view, command.source_slot)?
    else {
        return Ok(ScipSymbolEvidenceResult::NotProduced);
    };
    let overlay = ScipOverlaySummary::new(
        ScipOverlayDigest::try_from_slice(&row.digest)
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
        command.source_slot,
        u64::try_from(row.documents).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
        u64::try_from(row.occurrences).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
        u64::try_from(row.relationships).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
    );
    let (occurrences, occurrences_truncated) = read_scip_occurrences(
        &transaction,
        overlay.digest(),
        &command.package_scope,
        &command.symbol,
        command.limits,
        &command.cancelled,
        command.deadline,
    )?;
    let (relationships, relationships_truncated) = read_scip_relationships(
        &transaction,
        overlay.digest(),
        &command.package_scope,
        &command.symbol,
        command.limits,
        &command.cancelled,
        command.deadline,
    )?;
    check_control(&command.cancelled, command.deadline)?;
    if occurrences.is_empty() && relationships.is_empty() {
        return Ok(ScipSymbolEvidenceResult::NoMatch(overlay));
    }
    let output_bytes = evidence_output_bytes(&occurrences, &relationships)?;
    if output_bytes > command.limits.max_output_bytes() {
        return Err(SqliteStoreError::ScipEvidenceOutputLimitExceeded);
    }
    Ok(ScipSymbolEvidenceResult::Found(ScipSymbolEvidence::new(
        overlay,
        command.package_scope.semantic_digest(),
        occurrences,
        relationships,
        occurrences_truncated,
        relationships_truncated,
        output_bytes,
    )))
}

fn execute_scip_relationship_trace_command(
    connection: &mut Connection,
    command: &ScipRelationshipTraceCommand,
) -> Result<ScipRelationshipTraceResult, SqliteStoreError> {
    check_control(&command.cancelled, command.deadline)?;
    let progress_cancelled = Arc::clone(&command.cancelled);
    let deadline = command.deadline;
    connection
        .progress_handler(
            PROGRESS_OPCODES,
            Some(move || {
                progress_cancelled.load(Ordering::Acquire) || Instant::now() >= deadline
            }),
        )
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    let result = execute_scip_relationship_trace_transaction(connection, command);
    connection
        .progress_handler(0, None::<fn() -> bool>)
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    match result {
        Ok(result) => {
            check_control(&command.cancelled, command.deadline)?;
            Ok(result)
        }
        Err(SqliteStoreError::DatabaseOperationFailed) => {
            check_control(&command.cancelled, command.deadline)?;
            Err(SqliteStoreError::DatabaseOperationFailed)
        }
        Err(error) => Err(error),
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "one transaction owns overlay selection and the bounded BFS so no partial trace can escape its immutable view"
)]
fn execute_scip_relationship_trace_transaction(
    connection: &mut Connection,
    command: &ScipRelationshipTraceCommand,
) -> Result<ScipRelationshipTraceResult, SqliteStoreError> {
    if command.limits.max_edges() != command.max_edges.get() {
        return Err(SqliteStoreError::InvalidScipRelationshipTraceLimits);
    }
    check_control(&command.cancelled, command.deadline)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Deferred)
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    let Some(row) = load_active_overlay_summary(&transaction, &command.view, command.source_slot)?
    else {
        return Ok(ScipRelationshipTraceResult::NotProduced);
    };
    let overlay = ScipOverlaySummary::new(
        ScipOverlayDigest::try_from_slice(&row.digest)
            .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
        command.source_slot,
        u64::try_from(row.documents).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
        u64::try_from(row.occurrences).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
        u64::try_from(row.relationships).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
    );
    let package_scope = command.package_scope.semantic_digest();
    let mut queued = VecDeque::from([(command.root.clone(), 0_u8)]);
    let mut visited = BTreeSet::from([command.root.as_str().to_owned()]);
    let mut unexpanded_frontier = BTreeSet::new();
    let mut edges = Vec::new();
    let mut root_had_relationship = false;
    let mut depth_limit_reached = false;
    let mut edge_limit_reached = false;
    let mut symbol_limit_reached = false;
    let mut output_limit_reached = false;
    let mut output_bytes = 0_u64;

    'traversal: while let Some((current, current_depth)) = queued.pop_front() {
        check_control(&command.cancelled, command.deadline)?;
        if current_depth == command.max_depth.get() {
            depth_limit_reached = true;
            unexpanded_frontier.insert(current.as_str().to_owned());
            continue;
        }
        let retained = u16::try_from(edges.len())
            .map_err(|_| SqliteStoreError::CountNotRepresentable)?;
        if retained == command.limits.max_edges() {
            let rows = read_scip_relationship_trace_rows(
                &transaction,
                overlay.digest(),
                &command.package_scope,
                &current,
                command.direction,
                1,
                &command.cancelled,
                command.deadline,
            )?;
            if let Some(row) = rows.first() {
                edge_limit_reached = true;
                unexpanded_frontier.insert(current.as_str().to_owned());
                record_queued_frontier(
                    &mut unexpanded_frontier,
                    &queued,
                    command.max_depth.get(),
                    &mut depth_limit_reached,
                );
                let next_depth = current_depth
                    .checked_add(1)
                    .ok_or(SqliteStoreError::CountNotRepresentable)?;
                let next = trace_next_symbol(row, command.direction);
                record_unexpanded_frontier(
                    &mut unexpanded_frontier,
                    &visited,
                    next.as_str(),
                );
                if next_depth == command.max_depth.get()
                    && !visited.contains(next.as_str())
                {
                    depth_limit_reached = true;
                }
                break;
            }
            continue;
        }
        let query_limit = command
            .limits
            .max_edges()
            .checked_sub(retained)
            .and_then(|remaining| remaining.checked_add(1))
            .ok_or(SqliteStoreError::CountNotRepresentable)?;
        let rows = read_scip_relationship_trace_rows(
            &transaction,
            overlay.digest(),
            &command.package_scope,
            &current,
            command.direction,
            query_limit,
            &command.cancelled,
            command.deadline,
        )?;
        if current_depth == 0 && !rows.is_empty() {
            root_had_relationship = true;
        }
        for row in rows {
            check_control(&command.cancelled, command.deadline)?;
            let next = trace_next_symbol(&row, command.direction);
            let next_key = next.as_str().to_owned();
            let depth = current_depth
                .checked_add(1)
                .ok_or(SqliteStoreError::CountNotRepresentable)?;
            if edges.len() == usize::from(command.limits.max_edges()) {
                edge_limit_reached = true;
                unexpanded_frontier.insert(current.as_str().to_owned());
                record_queued_frontier(
                    &mut unexpanded_frontier,
                    &queued,
                    command.max_depth.get(),
                    &mut depth_limit_reached,
                );
                record_unexpanded_frontier(&mut unexpanded_frontier, &visited, &next_key);
                if depth == command.max_depth.get() && !visited.contains(&next_key) {
                    depth_limit_reached = true;
                }
                break 'traversal;
            }
            let edge_bytes = scip_relationship_trace_edge_output_bytes(&row.relationship)?;
            let next_output_bytes = output_bytes
                .checked_add(edge_bytes)
                .ok_or(SqliteStoreError::CountNotRepresentable)?;
            if next_output_bytes > command.limits.max_output_bytes() {
                unexpanded_frontier.insert(current.as_str().to_owned());
                record_queued_frontier(
                    &mut unexpanded_frontier,
                    &queued,
                    command.max_depth.get(),
                    &mut depth_limit_reached,
                );
                record_unexpanded_frontier(&mut unexpanded_frontier, &visited, &next_key);
                if depth == command.max_depth.get() && !visited.contains(&next_key) {
                    depth_limit_reached = true;
                }
                output_limit_reached = true;
                break 'traversal;
            }
            output_bytes = next_output_bytes;
            let expand = if visited.contains(&next_key) {
                false
            } else if visited.len() == usize::from(command.limits.max_nodes()) {
                symbol_limit_reached = true;
                record_unexpanded_frontier(&mut unexpanded_frontier, &visited, &next_key);
                if depth == command.max_depth.get() {
                    depth_limit_reached = true;
                }
                false
            } else {
                let inserted = visited.insert(next_key);
                if !inserted {
                    return Err(SqliteStoreError::IntegrityCheckFailed);
                }
                true
            };
            edges.push(ScipRelationshipTraceEdge::new(
                row.document_ordinal,
                row.relationship_ordinal,
                depth,
                row.relationship,
            ));
            if expand {
                queued.push_back((next, depth));
            }
        }
    }
    check_control(&command.cancelled, command.deadline)?;
    if !root_had_relationship {
        return Ok(ScipRelationshipTraceResult::NoRelationships(
            ScipRelationshipTraceNoRelationships::new(overlay, package_scope),
        ));
    }
    Ok(ScipRelationshipTraceResult::Found(ScipRelationshipTrace::new(
        overlay,
        package_scope,
        command.direction,
        command.max_depth.get(),
        edges,
        u16::try_from(visited.len()).map_err(|_| SqliteStoreError::CountNotRepresentable)?,
        u16::try_from(unexpanded_frontier.len())
            .map_err(|_| SqliteStoreError::CountNotRepresentable)?,
        depth_limit_reached,
        edge_limit_reached,
        symbol_limit_reached,
        output_limit_reached,
        output_bytes,
    )))
}

fn trace_next_symbol(
    row: &ScipRelationshipTraceRow,
    direction: ScipRelationshipTraceDirection,
) -> ScipSymbol {
    match direction {
        ScipRelationshipTraceDirection::Outgoing => row.relationship.target().clone(),
        ScipRelationshipTraceDirection::Incoming => row.relationship.source().clone(),
    }
}

fn record_unexpanded_frontier(
    unexpanded_frontier: &mut BTreeSet<String>,
    visited: &BTreeSet<String>,
    symbol: &str,
) {
    if !visited.contains(symbol) {
        unexpanded_frontier.insert(symbol.to_owned());
    }
}

fn record_queued_frontier(
    unexpanded_frontier: &mut BTreeSet<String>,
    queued: &VecDeque<(ScipSymbol, u8)>,
    max_depth: u8,
    depth_limit_reached: &mut bool,
) {
    for (symbol, depth) in queued {
        unexpanded_frontier.insert(symbol.as_str().to_owned());
        if *depth == max_depth {
            *depth_limit_reached = true;
        }
    }
}

fn execute_scip_syntax_symbol_command(
    connection: &mut Connection,
    command: &ScipSyntaxSymbolCommand,
) -> Result<ScipSyntaxSymbolResolution, SqliteStoreError> {
    check_control(&command.cancelled, command.deadline)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Deferred)
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    let Some(overlay) = load_active_overlay_summary(&transaction, &command.view, command.source_slot)?
    else {
        return Ok(ScipSyntaxSymbolResolution::NotProduced);
    };
    let start = i64::try_from(command.name_span.start().get())
        .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
    let end = i64::try_from(command.name_span.end().get())
        .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
    let mut statement = transaction
        .prepare(
            "SELECT DISTINCT occurrence.symbol
             FROM scip_overlay_occurrences AS occurrence
             JOIN scip_overlay_documents AS document
               ON document.overlay_digest = occurrence.overlay_digest
              AND document.document_ordinal = occurrence.document_ordinal
             WHERE occurrence.overlay_digest = ?1
               AND document.repository_path = ?2
               AND document.content_digest = ?3
               AND occurrence.start_byte = ?4
               AND occurrence.end_byte = ?5
               AND occurrence.symbol IS NOT NULL
             ORDER BY occurrence.symbol
             LIMIT 2",
        )
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    let rows = statement
        .query_map(
            params![
                overlay.digest,
                command.path.as_bytes(),
                command.content.as_bytes(),
                start,
                end,
            ],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    let mut symbols = Vec::with_capacity(2);
    for row in rows {
        check_control(&command.cancelled, command.deadline)?;
        let symbol = row.map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        let symbol = String::from_utf8(symbol).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
        symbols.push(ScipSymbol::try_new(symbol).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?);
    }
    check_control(&command.cancelled, command.deadline)?;
    match symbols.as_slice() {
        [] => Ok(ScipSyntaxSymbolResolution::NoExactMatch),
        [symbol] => Ok(ScipSyntaxSymbolResolution::Exact(symbol.clone())),
        [..] => Ok(ScipSyntaxSymbolResolution::Ambiguous),
    }
}

fn load_active_overlay_summary(
    transaction: &Transaction<'_>,
    view: &PinnedWorkspaceView,
    source_slot: SourceSlotId,
) -> Result<Option<ActiveOverlayRow>, SqliteStoreError> {
    transaction
        .query_row(
            "SELECT receipt.overlay_digest, receipt.document_count,
                    receipt.occurrence_count, receipt.relationship_count
             FROM active_scip_overlays AS active
             JOIN scip_overlay_receipts AS receipt
               ON receipt.overlay_digest = active.overlay_digest
             JOIN workspace_view_members AS member
               ON member.workspace_view_id = receipt.workspace_view_id
              AND member.source_slot_id = receipt.source_slot_id
             WHERE active.connected_workspace_id = ?1
               AND active.source_slot_id = ?2
               AND active.workspace_view_id = ?3
               AND receipt.connected_workspace_id = ?1
               AND receipt.workspace_view_id = ?3
               AND receipt.source_slot_id = ?2
               AND receipt.lifecycle_state = 'complete'
               AND member.source_epoch = receipt.source_epoch
               AND member.generation_workspace_id = receipt.generation_workspace_id
               AND member.generation_id = receipt.generation_id",
            params![
                view.connected_workspace().as_bytes().as_slice(),
                source_slot.as_bytes().as_slice(),
                view.view().get(),
            ],
            |row| {
                Ok(ActiveOverlayRow {
                    digest: row.get(0)?,
                    documents: row.get(1)?,
                    occurrences: row.get(2)?,
                    relationships: row.get(3)?,
                })
            },
        )
        .optional()
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)
}

fn read_scip_occurrences(
    transaction: &Transaction<'_>,
    digest: ScipOverlayDigest,
    package_scope: &PackageScope,
    symbol: &ScipSymbol,
    limits: ScipEvidenceReadLimits,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(Vec<ScipOccurrenceEvidence>, bool), SqliteStoreError> {
    let mut sql = String::from(
        "SELECT document.repository_path, document.content_digest, occurrence.roles,
                occurrence.start_byte, occurrence.end_byte
         FROM scip_overlay_occurrences AS occurrence
         JOIN scip_overlay_documents AS document
           ON document.overlay_digest = occurrence.overlay_digest
          AND document.document_ordinal = occurrence.document_ordinal
         WHERE occurrence.overlay_digest = ? AND occurrence.symbol = ?",
    );
    let mut parameters = vec![
        rusqlite::types::Value::Blob(digest.as_bytes().to_vec()),
        rusqlite::types::Value::Blob(symbol.as_str().as_bytes().to_vec()),
    ];
    append_package_scope_predicate(&mut sql, &mut parameters, package_scope);
    sql.push_str(" ORDER BY document.repository_path, occurrence.occurrence_ordinal LIMIT ?");
    parameters.push(rusqlite::types::Value::Integer(
        i64::from(limits.max_occurrences()) + 1,
    ));
    let mut statement = transaction
        .prepare(&sql)
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    let rows = statement
        .query_map(rusqlite::params_from_iter(parameters.iter()), |row| {
            Ok((
                row.get::<_, Vec<u8>>(0)?,
                row.get::<_, Vec<u8>>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    let mut result = Vec::new();
    let mut truncated = false;
    for row in rows {
        check_control(cancelled, deadline)?;
        if result.len() == usize::from(limits.max_occurrences()) {
            truncated = true;
            break;
        }
        let (path, content, roles, start, end) =
            row.map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        result.push(ScipOccurrenceEvidence::new(
            RepositoryPath::try_from_bytes(&path, PERSISTED_PATH_LIMITS)
                .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
            SourceContentDigest::try_from_slice(&content)
                .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
            decode_span(start, end)?,
            ScipSymbolRoles::new(u32::try_from(roles).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?),
        ));
    }
    Ok((result, truncated))
}

fn read_scip_relationships(
    transaction: &Transaction<'_>,
    digest: ScipOverlayDigest,
    package_scope: &PackageScope,
    symbol: &ScipSymbol,
    limits: ScipEvidenceReadLimits,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(Vec<ScipRelationshipEvidence>, bool), SqliteStoreError> {
    let mut sql = String::from(
        "SELECT document.repository_path, document.content_digest,
                relationship.source_symbol, relationship.target_symbol,
                relationship.kinds,
                CASE WHEN relationship.source_symbol = ? THEN 0 ELSE 1 END
         FROM scip_overlay_relationships AS relationship
         JOIN scip_overlay_documents AS document
           ON document.overlay_digest = relationship.overlay_digest
          AND document.document_ordinal = relationship.document_ordinal
         WHERE relationship.overlay_digest = ?
           AND (relationship.source_symbol = ? OR relationship.target_symbol = ?)",
    );
    let symbol_value = rusqlite::types::Value::Blob(symbol.as_str().as_bytes().to_vec());
    let mut parameters = vec![
        symbol_value.clone(),
        rusqlite::types::Value::Blob(digest.as_bytes().to_vec()),
        symbol_value.clone(),
        symbol_value,
    ];
    append_package_scope_predicate(&mut sql, &mut parameters, package_scope);
    sql.push_str(" ORDER BY document.repository_path, relationship.relationship_ordinal LIMIT ?");
    parameters.push(rusqlite::types::Value::Integer(
        i64::from(limits.max_relationships()) + 1,
    ));
    let mut statement = transaction
        .prepare(&sql)
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    let rows = statement
        .query_map(rusqlite::params_from_iter(parameters.iter()), |row| {
            Ok((
                row.get::<_, Vec<u8>>(0)?,
                row.get::<_, Vec<u8>>(1)?,
                row.get::<_, Vec<u8>>(2)?,
                row.get::<_, Vec<u8>>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
            ))
        })
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    let mut result = Vec::new();
    let mut truncated = false;
    for row in rows {
        check_control(cancelled, deadline)?;
        if result.len() == usize::from(limits.max_relationships()) {
            truncated = true;
            break;
        }
        let (path, content, source, target, kinds, direction) =
            row.map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        result.push(ScipRelationshipEvidence::new(
            RepositoryPath::try_from_bytes(&path, PERSISTED_PATH_LIMITS)
                .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
            SourceContentDigest::try_from_slice(&content)
                .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
            if direction == 0 {
                ScipRelationshipDirection::Outgoing
            } else if direction == 1 {
                ScipRelationshipDirection::Incoming
            } else {
                return Err(SqliteStoreError::IntegrityCheckFailed);
            },
            decode_symbol(source)?,
            decode_symbol(target)?,
            decode_relationship_kinds(kinds)?,
        ));
    }
    Ok((result, truncated))
}

struct ScipRelationshipTraceRow {
    document_ordinal: u32,
    relationship_ordinal: u32,
    relationship: ScipRelationshipEvidence,
}

#[allow(
    clippy::too_many_arguments,
    reason = "transaction, immutable scope, traversal controls, and cancellation/deadline are independent query inputs"
)]
fn read_scip_relationship_trace_rows(
    transaction: &Transaction<'_>,
    digest: ScipOverlayDigest,
    package_scope: &PackageScope,
    symbol: &ScipSymbol,
    direction: ScipRelationshipTraceDirection,
    row_limit: u16,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<Vec<ScipRelationshipTraceRow>, SqliteStoreError> {
    let (predicate, relationship_direction) = match direction {
        ScipRelationshipTraceDirection::Outgoing => {
            ("relationship.source_symbol = ?", ScipRelationshipDirection::Outgoing)
        }
        ScipRelationshipTraceDirection::Incoming => {
            ("relationship.target_symbol = ?", ScipRelationshipDirection::Incoming)
        }
    };
    let mut sql = format!(
        "SELECT document.repository_path, document.content_digest,
                relationship.document_ordinal, relationship.relationship_ordinal,
                relationship.source_symbol, relationship.target_symbol, relationship.kinds
         FROM scip_overlay_relationships AS relationship
         JOIN scip_overlay_documents AS document
           ON document.overlay_digest = relationship.overlay_digest
          AND document.document_ordinal = relationship.document_ordinal
         WHERE relationship.overlay_digest = ?
           AND {predicate}"
    );
    let mut parameters = vec![
        rusqlite::types::Value::Blob(digest.as_bytes().to_vec()),
        rusqlite::types::Value::Blob(symbol.as_str().as_bytes().to_vec()),
    ];
    append_package_scope_predicate(&mut sql, &mut parameters, package_scope);
    sql.push_str(
        " ORDER BY relationship.document_ordinal, relationship.relationship_ordinal LIMIT ?",
    );
    parameters.push(rusqlite::types::Value::Integer(i64::from(row_limit)));
    let mut statement = transaction
        .prepare(&sql)
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    let rows = statement
        .query_map(rusqlite::params_from_iter(parameters.iter()), |row| {
            Ok((
                row.get::<_, Vec<u8>>(0)?,
                row.get::<_, Vec<u8>>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, Vec<u8>>(4)?,
                row.get::<_, Vec<u8>>(5)?,
                row.get::<_, i64>(6)?,
            ))
        })
        .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
    let mut result = Vec::with_capacity(usize::from(row_limit));
    for row in rows {
        check_control(cancelled, deadline)?;
        let (path, content, document_ordinal, relationship_ordinal, source, target, kinds) =
            row.map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        result.push(ScipRelationshipTraceRow {
            document_ordinal: u32::try_from(document_ordinal)
                .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
            relationship_ordinal: u32::try_from(relationship_ordinal)
                .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
            relationship: ScipRelationshipEvidence::new(
                RepositoryPath::try_from_bytes(&path, PERSISTED_PATH_LIMITS)
                    .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
                SourceContentDigest::try_from_slice(&content)
                    .map_err(|_| SqliteStoreError::IntegrityCheckFailed)?,
                relationship_direction,
                decode_symbol(source)?,
                decode_symbol(target)?,
                decode_relationship_kinds(kinds)?,
            ),
        });
    }
    Ok(result)
}

fn append_package_scope_predicate(
    sql: &mut String,
    parameters: &mut Vec<rusqlite::types::Value>,
    package_scope: &PackageScope,
) {
    let Some(roots) = package_scope.explicit_roots() else { return };
    sql.push_str(" AND (");
    for (ordinal, root) in roots.iter().enumerate() {
        if ordinal != 0 { sql.push_str(" OR "); }
        sql.push_str("(document.repository_path = ? OR (substr(document.repository_path, 1, ?) = ? AND length(document.repository_path) > ? AND substr(document.repository_path, ? + 1, 1) = X'2F'))");
        let root_bytes = root.as_bytes().to_vec();
        let root_length = i64::try_from(root_bytes.len()).expect("bounded repository path fits SQLite");
        parameters.push(rusqlite::types::Value::Blob(root_bytes.clone()));
        parameters.push(rusqlite::types::Value::Integer(root_length));
        parameters.push(rusqlite::types::Value::Blob(root_bytes));
        parameters.push(rusqlite::types::Value::Integer(root_length));
        parameters.push(rusqlite::types::Value::Integer(root_length));
    }
    sql.push(')');
}

fn decode_span(start: i64, end: i64) -> Result<ByteSpan, SqliteStoreError> {
    let start = u64::try_from(start).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
    let end = u64::try_from(end).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
    ByteSpan::try_new(ByteOffset::new(start), ByteOffset::new(end))
        .map_err(|_| SqliteStoreError::IntegrityCheckFailed)
}

fn decode_symbol(value: Vec<u8>) -> Result<ScipSymbol, SqliteStoreError> {
    ScipSymbol::try_new(String::from_utf8(value).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?)
        .map_err(|_| SqliteStoreError::IntegrityCheckFailed)
}

fn decode_relationship_kinds(value: i64) -> Result<ScipRelationshipKinds, SqliteStoreError> {
    let value = u8::try_from(value).map_err(|_| SqliteStoreError::IntegrityCheckFailed)?;
    if value & !0x0f != 0 { return Err(SqliteStoreError::IntegrityCheckFailed); }
    ScipRelationshipKinds::try_new(
        value & 1 != 0,
        value & 2 != 0,
        value & 4 != 0,
        value & 8 != 0,
    ).map_err(|_| SqliteStoreError::IntegrityCheckFailed)
}

fn evidence_output_bytes(
    occurrences: &[ScipOccurrenceEvidence],
    relationships: &[ScipRelationshipEvidence],
) -> Result<u64, SqliteStoreError> {
    let mut total = 0_u64;
    for occurrence in occurrences {
        total = total.checked_add(48)
            .and_then(|value| value.checked_add(occurrence.path().byte_count().get()))
            .ok_or(SqliteStoreError::CountNotRepresentable)?;
    }
    for relationship in relationships {
        let source = u64::try_from(relationship.source().as_str().len())
            .map_err(|_| SqliteStoreError::CountNotRepresentable)?;
        let target = u64::try_from(relationship.target().as_str().len())
            .map_err(|_| SqliteStoreError::CountNotRepresentable)?;
        let symbols = source
            .checked_add(target)
            .ok_or(SqliteStoreError::CountNotRepresentable)?;
        total = total.checked_add(48)
            .and_then(|value| value.checked_add(relationship.path().byte_count().get()))
            .and_then(|value| value.checked_add(symbols))
            .ok_or(SqliteStoreError::CountNotRepresentable)?;
    }
    Ok(total)
}

fn scip_relationship_trace_edge_output_bytes(
    relationship: &ScipRelationshipEvidence,
) -> Result<u64, SqliteStoreError> {
    // This bounds the JSON representation used by both CLI and MCP, rather
    // than the raw SQLite values. JSON may escape each input byte as `\\u00XX`.
    // The path is emitted in the byte-preserving `rwp1:h:` hexadecimal form.
    const FIXED_EDGE_JSON_BYTES: u64 = 512;
    const PATH_TEXT_PREFIX_BYTES: u64 = 7;

    let path = relationship
        .path()
        .byte_count()
        .get()
        .checked_mul(2)
        .and_then(|value| value.checked_add(PATH_TEXT_PREFIX_BYTES))
        .ok_or(SqliteStoreError::CountNotRepresentable)?;
    let source = json_string_upper_bound(relationship.source().as_str().len())?;
    let target = json_string_upper_bound(relationship.target().as_str().len())?;
    FIXED_EDGE_JSON_BYTES
        .checked_add(path)
        .and_then(|value| value.checked_add(source))
        .and_then(|value| value.checked_add(target))
        .ok_or(SqliteStoreError::CountNotRepresentable)
}

fn json_string_upper_bound(value_bytes: usize) -> Result<u64, SqliteStoreError> {
    const JSON_STRING_DELIMITERS_BYTES: u64 = 2;
    const MAX_JSON_BYTES_PER_INPUT_BYTE: u64 = 6;

    let value_bytes =
        u64::try_from(value_bytes).map_err(|_| SqliteStoreError::CountNotRepresentable)?;
    value_bytes
        .checked_mul(MAX_JSON_BYTES_PER_INPUT_BYTE)
        .and_then(|value| value.checked_add(JSON_STRING_DELIMITERS_BYTES))
        .ok_or(SqliteStoreError::CountNotRepresentable)
}
