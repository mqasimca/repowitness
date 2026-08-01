enum WriterCommand {
    Register {
        repository: RepositoryIdentityDigest,
        initial_source_epoch: u64,
        deadline: Instant,
        reply: Reply<()>,
    },
    EnsureWorkspace {
        repository: RepositoryIdentityDigest,
        initial_source_epoch: u64,
        deadline: Instant,
        reply: Reply<SourceSlotEpoch>,
    },
    AdvanceEpoch {
        repository: RepositoryIdentityDigest,
        expected: u64,
        next: u64,
        deadline: Instant,
        reply: Reply<()>,
    },
    Stage(Box<StageCommand>),
    StageSourceSlot(Box<StageSourceSlotCommand>),
    StageGraph(Box<StageGraphCommand>),
    StageScipOverlay(Box<StageScipOverlayCommand>),
    ImportMemory(Box<MemoryImportCommand>),
    SyncTeamMemory(Box<MemoryImportCommand>),
    ImportObservedMemoryHistory(Box<ObservedMemoryHistoryCommand>),
    AppendMemoryCorrespondenceReview(Box<AppendMemoryCorrespondenceReviewCommand>),
    AppendTaskCheckpoint(Box<TaskCheckpointCommand>),
    AppendTaskVerification(Box<TaskVerificationCommand>),
    AppendPersonalMemory(Box<PersonalMemoryCommand>),
    TaskStatus(Box<TaskStatusCommand>),
    LoadMemorySource {
        repository: RepositoryIdentityDigest,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
        reply: Reply<MemoryProjectionSource>,
    },
    LoadMemoryJournal(Box<LoadMemoryJournalCommand>),
    LoadRustMemoryCandidates(Box<LoadRustMemoryCandidatesCommand>),
    LoadMemoryCorrespondenceReviews(Box<LoadMemoryCorrespondenceReviewsCommand>),
    PublishMemoryProjection(Box<PublishMemoryProjectionCommand>),
    Activate {
        generation: GenerationId,
        expected_source_epoch: u64,
        deadline: Instant,
        reply: Reply<()>,
    },
    ActiveGeneration {
        repository: RepositoryIdentityDigest,
        deadline: Instant,
        reply: Reply<Option<GenerationId>>,
    },
    ConnectWorkspace(Box<ConnectWorkspaceCommand>),
    SourceSlotState(Box<SourceSlotStateCommand>),
    ReserveSourceSlotEpoch(Box<ReserveSourceSlotEpochCommand>),
    CompleteSourceSlotEpoch(Box<CompleteSourceSlotEpochCommand>),
    PublishWorkspaceView(Box<PublishWorkspaceViewCommand>),
    ActiveWorkspaceView {
        connected_workspace: ConnectedWorkspaceId,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
        reply: Reply<Option<PinnedWorkspaceView>>,
    },
    RebuildProjection(Box<RebuildProjectionCommand>),
    PlanRetention(Box<PlanRetentionCommand>),
    ApplyRetention(Box<ApplyRetentionCommand>),
    Checkpoint {
        deadline: Instant,
        reply: Reply<CheckpointOutcome>,
    },
    Shutdown {
        reply: Reply<()>,
    },
}

impl WriterCommand {
    #[allow(
        clippy::match_like_matches_macro,
        reason = "an exhaustive match forces every future writer command to declare whether it mutates"
    )]
    fn is_mutating(&self) -> bool {
        match self {
            Self::Register { .. }
            | Self::EnsureWorkspace { .. }
            | Self::AdvanceEpoch { .. }
            | Self::Stage(_)
            | Self::StageSourceSlot(_)
            | Self::StageGraph(_)
            | Self::StageScipOverlay(_)
            | Self::ImportMemory(_)
            | Self::SyncTeamMemory(_)
            | Self::ImportObservedMemoryHistory(_)
            | Self::AppendMemoryCorrespondenceReview(_)
            | Self::AppendTaskCheckpoint(_)
            | Self::AppendTaskVerification(_)
            | Self::AppendPersonalMemory(_)
            | Self::PublishMemoryProjection(_)
            | Self::Activate { .. }
            | Self::ConnectWorkspace(_)
            | Self::ReserveSourceSlotEpoch(_)
            | Self::CompleteSourceSlotEpoch(_)
            | Self::PublishWorkspaceView(_)
            | Self::RebuildProjection(_)
            | Self::ApplyRetention(_)
            | Self::Checkpoint { .. } => true,
            Self::LoadMemorySource { .. }
            | Self::TaskStatus(_)
            | Self::LoadMemoryJournal(_)
            | Self::LoadRustMemoryCandidates(_)
            | Self::LoadMemoryCorrespondenceReviews(_)
            | Self::ActiveGeneration { .. }
            | Self::SourceSlotState(_)
            | Self::ActiveWorkspaceView { .. }
            | Self::PlanRetention(_)
            | Self::Shutdown { .. } => false,
        }
    }

    fn reject_unresolved_mutation(self) {
        match self {
            Self::Register { reply, .. } => reject_unresolved_reply(reply),
            Self::EnsureWorkspace { reply, .. } => reject_unresolved_reply(reply),
            Self::AdvanceEpoch { reply, .. } => reject_unresolved_reply(reply),
            Self::Stage(command) => reject_unresolved_reply(command.reply),
            Self::StageSourceSlot(command) => reject_unresolved_reply(command.reply),
            Self::StageGraph(command) => reject_unresolved_reply(command.reply),
            Self::StageScipOverlay(command) => reject_unresolved_reply(command.reply),
            Self::ImportMemory(command) => reject_unresolved_reply(command.reply),
            Self::SyncTeamMemory(command) => reject_unresolved_reply(command.reply),
            Self::ImportObservedMemoryHistory(command) => reject_unresolved_reply(command.reply),
            Self::AppendMemoryCorrespondenceReview(command) => {
                reject_unresolved_reply(command.reply);
            }
            Self::AppendTaskCheckpoint(command) => reject_unresolved_reply(command.reply),
            Self::AppendTaskVerification(command) => reject_unresolved_reply(command.reply),
            Self::AppendPersonalMemory(command) => reject_unresolved_reply(command.reply),
            Self::PublishMemoryProjection(command) => reject_unresolved_reply(command.reply),
            Self::Activate { reply, .. } => reject_unresolved_reply(reply),
            Self::ConnectWorkspace(command) => reject_unresolved_reply(command.reply),
            Self::ReserveSourceSlotEpoch(command) => reject_unresolved_reply(command.reply),
            Self::CompleteSourceSlotEpoch(command) => reject_unresolved_reply(command.reply),
            Self::PublishWorkspaceView(command) => reject_unresolved_reply(command.reply),
            Self::RebuildProjection(command) => reject_unresolved_reply(command.reply),
            Self::ApplyRetention(command) => reject_unresolved_reply(command.reply),
            Self::Checkpoint { reply, .. } => reject_unresolved_reply(reply),
            Self::LoadMemorySource { .. }
            | Self::TaskStatus(_)
            | Self::LoadMemoryJournal(_)
            | Self::LoadRustMemoryCandidates(_)
            | Self::LoadMemoryCorrespondenceReviews(_)
            | Self::ActiveGeneration { .. }
            | Self::SourceSlotState(_)
            | Self::ActiveWorkspaceView { .. }
            | Self::PlanRetention(_)
            | Self::Shutdown { .. } => {}
        }
    }
}

fn reject_unresolved_reply<T>(reply: Reply<T>) {
    let _ = reply.try_send(Err(SqliteStoreError::MutationOutcomeUnknown));
}

struct StageCommand {
    source_epoch: u64,
    identity: RustSourceSnapshotIdentity,
    prepared: PreparedRustIndex,
    coverage: RustIndexCoverage,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    reply: Reply<GenerationId>,
}

struct StageSourceSlotCommand {
    connected_workspace: ConnectedWorkspaceId,
    source_slot: SourceSlotId,
    reserved_epoch: SourceSlotEpoch,
    identity: RustSourceSnapshotIdentity,
    prepared: PreparedRustIndex,
    coverage: RustIndexCoverage,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    reply: Reply<GenerationId>,
}

struct StageGraphCommand {
    generation: GenerationId,
    prepared: PreparedRustGraphGeneration,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    reply: Reply<()>,
}

struct StageScipOverlayCommand {
    connected_workspace: ConnectedWorkspaceId,
    workspace_view: WorkspaceViewId,
    source_slot: SourceSlotId,
    require_active_view: bool,
    prepared: PreparedScipOverlay,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    reply: Reply<repowitness_domain::ScipOverlayDigest>,
}

struct MemoryImportCommand {
    prepared: PreparedMemoryImport,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    reply: Reply<MemoryImportReceipt>,
}

struct ObservedMemoryHistoryCommand {
    repository: RepositoryIdentityDigest,
    prepared: Box<[PreparedMemoryImport]>,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    reply: Reply<Box<[MemoryImportReceipt]>>,
}

struct AppendMemoryCorrespondenceReviewCommand {
    prepared: PreparedMemoryCorrespondenceReview,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    reply: Reply<MemoryCorrespondenceReviewReceipt>,
}

struct LoadMemoryJournalCommand {
    repository: RepositoryIdentityDigest,
    limits: MemoryProjectionLoadLimits,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    reply: Reply<LoadedMemoryJournal>,
}

struct LoadRustMemoryCandidatesCommand {
    source: MemoryProjectionSource,
    evidence: RustSymbolMemoryEvidence,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    reply: Reply<LoadedRustCandidateSet>,
}

struct LoadMemoryCorrespondenceReviewsCommand {
    source: MemoryProjectionSource,
    record_id: repowitness_domain::MemoryRecordId,
    revision: repowitness_domain::CanonicalMemoryDigest,
    evidence_ordinal: u8,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    reply: Reply<LoadedCorrespondenceReviews>,
}

struct PublishMemoryProjectionCommand {
    prepared: PreparedMemoryProjection,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    reply: Reply<MemoryProjectionPublication>,
}

struct RebuildProjectionCommand {
    limits: ProjectionRebuildLimits,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    reply: Reply<ProjectionRebuildOutcome>,
}

struct ConnectWorkspaceCommand {
    connected_workspace: ConnectedWorkspaceId,
    source_slots: Box<[WorkspaceSourceSlot]>,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    reply: Reply<()>,
}

struct SourceSlotStateCommand {
    connected_workspace: ConnectedWorkspaceId,
    source_slot: SourceSlotId,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    reply: Reply<SourceSlotState>,
}

struct ReserveSourceSlotEpochCommand {
    connected_workspace: ConnectedWorkspaceId,
    source_slot: SourceSlotId,
    expected: SourceSlotEpoch,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    reply: Reply<SourceSlotEpoch>,
}

struct CompleteSourceSlotEpochCommand {
    connected_workspace: ConnectedWorkspaceId,
    source_slot: SourceSlotId,
    source_epoch: SourceSlotEpoch,
    generation: GenerationId,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    reply: Reply<()>,
}

struct PublishWorkspaceViewCommand {
    connected_workspace: ConnectedWorkspaceId,
    members: Box<[WorkspaceViewMember]>,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    reply: Reply<WorkspaceViewId>,
}
