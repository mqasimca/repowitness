impl OwnedSqliteIndex {
    /// Persists and activates one complete SCIP overlay for an exact published
    /// workspace-view source slot.
    ///
    /// The owned writer validates the supplied identity against the view member,
    /// stages every document/fact row, completes the receipt, and switches the
    /// slot pointer in one transaction. A failure leaves the prior pointer
    /// readable. On [`SqliteStoreError::MutationOutcomeUnknown`], reopen and
    /// inspect the exact overlay digest before retrying.
    pub fn stage_scip_overlay(
        &self,
        connected_workspace: ConnectedWorkspaceId,
        workspace_view: WorkspaceViewId,
        source_slot: SourceSlotId,
        prepared: PreparedScipOverlay,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<repowitness_domain::ScipOverlayDigest, SqliteStoreError> {
        self.send_scip_overlay(
            connected_workspace,
            workspace_view,
            source_slot,
            prepared,
            false,
            cancelled,
            deadline,
        )
    }

    /// Publishes only if the selected view remains the current active view at
    /// the writer's transaction fence.
    #[allow(
        clippy::too_many_arguments,
        reason = "exact view scope, immutable payload, activity policy, and control remain explicit"
    )]
    pub(crate) fn stage_current_scip_overlay(
        &self,
        connected_workspace: ConnectedWorkspaceId,
        workspace_view: WorkspaceViewId,
        source_slot: SourceSlotId,
        prepared: PreparedScipOverlay,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<repowitness_domain::ScipOverlayDigest, SqliteStoreError> {
        self.send_scip_overlay(
            connected_workspace,
            workspace_view,
            source_slot,
            prepared,
            true,
            cancelled,
            deadline,
        )
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "exact view scope, immutable payload, activity policy, and control remain explicit"
    )]
    fn send_scip_overlay(
        &self,
        connected_workspace: ConnectedWorkspaceId,
        workspace_view: WorkspaceViewId,
        source_slot: SourceSlotId,
        prepared: PreparedScipOverlay,
        require_active_view: bool,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<repowitness_domain::ScipOverlayDigest, SqliteStoreError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(
            WriterCommand::StageScipOverlay(Box::new(StageScipOverlayCommand {
                connected_workspace,
                workspace_view,
                source_slot,
                require_active_view,
                prepared,
                cancelled: Arc::clone(&cancelled),
                deadline,
                reply,
            })),
            deadline,
        )?;
        match receive_mutation_reply(
            &receiver,
            Some(cancelled.as_ref()),
            deadline,
            Some(&self.unresolved_mutation),
        ) {
            Ok(digest) => Ok(digest),
            Err(error) => {
                cancelled.store(true, Ordering::Release);
                Err(error)
            }
        }
    }
}
