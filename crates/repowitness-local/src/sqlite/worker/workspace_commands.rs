/// One already-completed source-slot candidate awaiting workspace-view publication.
///
/// This crate-internal composition value deliberately carries no root path,
/// Git selector, package selector, or configuration text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CompletedWorkspaceSource {
    source_slot: SourceSlotId,
    completed: CompletedSourceSlotIndex<GenerationId>,
}

impl CompletedWorkspaceSource {
    pub(crate) const fn new(
        source_slot: SourceSlotId,
        completed: CompletedSourceSlotIndex<GenerationId>,
    ) -> Self {
        Self {
            source_slot,
            completed,
        }
    }

    const fn source_slot(self) -> SourceSlotId {
        self.source_slot
    }

    const fn source_epoch(self) -> SourceSlotEpoch {
        self.completed.source_epoch()
    }

    const fn generation(self) -> GenerationId {
        self.completed.generation()
    }
}

impl OwnedSqliteIndex {
    /// Publishes one immutable view from already-completed source-slot candidates.
    ///
    /// Selector resolution and source preparation intentionally stay outside
    /// this crate-internal composition seam.
    /// On [`SqliteStoreError::MutationOutcomeUnknown`], read the active
    /// immutable workspace view before retrying.
    pub(crate) fn publish_completed_workspace_view(
        &self,
        connected_workspace: ConnectedWorkspaceId,
        sources: Vec<CompletedWorkspaceSource>,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<WorkspaceViewId, SqliteStoreError> {
        check_workspace_command_control(cancelled.as_ref(), deadline)?;
        let members = completed_workspace_members(&sources)?;
        check_workspace_command_control(cancelled.as_ref(), deadline)?;
        self.publish_workspace_view(connected_workspace, members, cancelled, deadline)
    }

    /// Registers one immutable bounded source-slot membership set.
    ///
    /// On [`SqliteStoreError::MutationOutcomeUnknown`], reopen the store and
    /// inspect durable membership and source-slot state before retrying.
    pub fn connect_workspace(
        &self,
        connected_workspace: ConnectedWorkspaceId,
        source_slots: Vec<WorkspaceSourceSlot>,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<(), SqliteStoreError> {
        check_workspace_command_control(cancelled.as_ref(), deadline)?;
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(
            WriterCommand::ConnectWorkspace(Box::new(ConnectWorkspaceCommand {
                connected_workspace,
                source_slots: source_slots.into_boxed_slice(),
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
            Ok(()) => Ok(()),
            Err(error) => {
                cancelled.store(true, Ordering::Release);
                Err(error)
            }
        }
    }

    /// Captures the durable epoch, current completion, and active receipt.
    pub fn source_slot_state(
        &self,
        connected_workspace: ConnectedWorkspaceId,
        source_slot: SourceSlotId,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<SourceSlotState, SqliteStoreError> {
        check_workspace_command_control(cancelled.as_ref(), deadline)?;
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(
            WriterCommand::SourceSlotState(Box::new(SourceSlotStateCommand {
                connected_workspace,
                source_slot,
                cancelled: Arc::clone(&cancelled),
                deadline,
                reply,
            })),
            deadline,
        )?;
        receive_reply(&receiver, deadline)
    }

    /// Atomically reserves the exact next durable epoch for one source slot.
    ///
    /// On [`SqliteStoreError::MutationOutcomeUnknown`], call
    /// [`Self::source_slot_state`] before retrying the reservation.
    pub fn reserve_source_slot_epoch(
        &self,
        connected_workspace: ConnectedWorkspaceId,
        source_slot: SourceSlotId,
        expected: SourceSlotEpoch,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<SourceSlotEpoch, SqliteStoreError> {
        check_workspace_command_control(cancelled.as_ref(), deadline)?;
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(
            WriterCommand::ReserveSourceSlotEpoch(Box::new(ReserveSourceSlotEpochCommand {
                connected_workspace,
                source_slot,
                expected,
                cancelled: Arc::clone(&cancelled),
                deadline,
                reply,
            })),
            deadline,
        )?;
        receive_mutation_reply(
            &receiver,
            Some(cancelled.as_ref()),
            deadline,
            Some(&self.unresolved_mutation),
        )
    }

    /// Binds one ready generation to a reserved epoch while it remains current.
    ///
    /// On [`SqliteStoreError::MutationOutcomeUnknown`], call
    /// [`Self::source_slot_state`] and compare its completion before retrying.
    pub fn complete_source_slot_epoch(
        &self,
        connected_workspace: ConnectedWorkspaceId,
        source_slot: SourceSlotId,
        source_epoch: SourceSlotEpoch,
        generation: GenerationId,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<(), SqliteStoreError> {
        check_workspace_command_control(cancelled.as_ref(), deadline)?;
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(
            WriterCommand::CompleteSourceSlotEpoch(Box::new(CompleteSourceSlotEpochCommand {
                connected_workspace,
                source_slot,
                source_epoch,
                generation,
                cancelled: Arc::clone(&cancelled),
                deadline,
                reply,
            })),
            deadline,
        )?;
        receive_mutation_reply(
            &receiver,
            Some(cancelled.as_ref()),
            deadline,
            Some(&self.unresolved_mutation),
        )
    }

    /// Atomically publishes one complete immutable connected-workspace view.
    ///
    /// On [`SqliteStoreError::MutationOutcomeUnknown`], call
    /// [`Self::active_workspace_view`] and compare the canonical members before
    /// retrying.
    pub fn publish_workspace_view(
        &self,
        connected_workspace: ConnectedWorkspaceId,
        members: Vec<WorkspaceViewMember>,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<WorkspaceViewId, SqliteStoreError> {
        check_workspace_command_control(cancelled.as_ref(), deadline)?;
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(
            WriterCommand::PublishWorkspaceView(Box::new(PublishWorkspaceViewCommand {
                connected_workspace,
                members: members.into_boxed_slice(),
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
            Ok(view) => Ok(view),
            Err(error) => {
                cancelled.store(true, Ordering::Release);
                Err(error)
            }
        }
    }

    /// Captures the current immutable view and all members in one read snapshot.
    pub fn active_workspace_view(
        &self,
        connected_workspace: ConnectedWorkspaceId,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<Option<PinnedWorkspaceView>, SqliteStoreError> {
        check_workspace_command_control(cancelled.as_ref(), deadline)?;
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(
            WriterCommand::ActiveWorkspaceView {
                connected_workspace,
                cancelled: Arc::clone(&cancelled),
                deadline,
                reply,
            },
            deadline,
        )?;
        match receive_reply(&receiver, deadline) {
            Ok(view) => Ok(view),
            Err(error) => {
                cancelled.store(true, Ordering::Release);
                Err(error)
            }
        }
    }
}

fn completed_workspace_members(
    sources: &[CompletedWorkspaceSource],
) -> Result<Vec<WorkspaceViewMember>, SqliteStoreError> {
    if sources.is_empty() {
        return Err(SqliteStoreError::InvalidWorkspaceView);
    }
    if sources.len() > super::MAX_CONNECTED_WORKSPACE_SOURCE_SLOTS {
        return Err(SqliteStoreError::WorkspaceSourceSlotLimitExceeded);
    }
    let members = sources
        .iter()
        .copied()
        .map(|source| {
            WorkspaceViewMember::at_epoch(
                source.source_slot(),
                source.source_epoch(),
                source.generation(),
            )
        })
        .collect::<Vec<_>>();
    super::workspace::canonical_view_members(&members)
}

fn check_workspace_command_control(
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), SqliteStoreError> {
    if cancelled.load(Ordering::Acquire) {
        Err(SqliteStoreError::Cancelled)
    } else if Instant::now() >= deadline {
        Err(SqliteStoreError::DeadlineExceeded)
    } else {
        Ok(())
    }
}
