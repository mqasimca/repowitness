use std::fmt;

use repowitness_application::SourceSlotEpoch;
use repowitness_domain::{
    ConnectedWorkspaceId, RepositoryIdentityDigest, SourceSlotId, SourceSnapshotDigest,
};

use super::{GenerationId, SqliteStoreError};

/// Maximum number of source slots in one connected workspace.
pub const MAX_CONNECTED_WORKSPACE_SOURCE_SLOTS: usize = 256;

/// One immutable source-slot mapping requested for a connected workspace.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkspaceSourceSlot {
    source_slot: SourceSlotId,
    repository: RepositoryIdentityDigest,
}

impl WorkspaceSourceSlot {
    /// Maps one explicit source slot to one logical repository.
    #[must_use]
    pub const fn new(source_slot: SourceSlotId, repository: RepositoryIdentityDigest) -> Self {
        Self {
            source_slot,
            repository,
        }
    }

    /// Returns the opaque source-slot identity.
    #[must_use]
    pub const fn source_slot(self) -> SourceSlotId {
        self.source_slot
    }

    /// Returns the mapped logical repository identity.
    #[must_use]
    pub const fn repository(self) -> RepositoryIdentityDigest {
        self.repository
    }
}

/// One requested generation member of a new immutable workspace view.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkspaceViewMember {
    source_slot: SourceSlotId,
    source_epoch: SourceSlotEpoch,
    generation: GenerationId,
}

impl WorkspaceViewMember {
    /// Selects one concrete generation at the initial source-slot epoch.
    #[must_use]
    pub const fn new(source_slot: SourceSlotId, generation: GenerationId) -> Self {
        Self::at_epoch(source_slot, SourceSlotEpoch::INITIAL, generation)
    }

    /// Selects one concrete generation at an explicit completed slot epoch.
    #[must_use]
    pub const fn at_epoch(
        source_slot: SourceSlotId,
        source_epoch: SourceSlotEpoch,
        generation: GenerationId,
    ) -> Self {
        Self {
            source_slot,
            source_epoch,
            generation,
        }
    }

    /// Returns the selected source-slot identity.
    #[must_use]
    pub const fn source_slot(self) -> SourceSlotId {
        self.source_slot
    }

    /// Returns the completed source-slot epoch being selected.
    #[must_use]
    pub const fn source_epoch(self) -> SourceSlotEpoch {
        self.source_epoch
    }

    /// Returns the selected concrete generation.
    #[must_use]
    pub const fn generation(self) -> GenerationId {
        self.generation
    }
}

/// Database-local identity for one immutable connected-workspace view.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct WorkspaceViewId(i64);

impl WorkspaceViewId {
    pub(crate) const fn from_database(value: i64) -> Self {
        Self(value)
    }

    /// Returns the positive database-local identity.
    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }
}

/// One validated member captured from a pinned immutable workspace view.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PinnedWorkspaceViewMember {
    ordinal: u16,
    source_slot: SourceSlotId,
    source_epoch: SourceSlotEpoch,
    repository: RepositoryIdentityDigest,
    generation: GenerationId,
}

impl PinnedWorkspaceViewMember {
    pub(super) const fn new(
        ordinal: u16,
        source_slot: SourceSlotId,
        source_epoch: SourceSlotEpoch,
        repository: RepositoryIdentityDigest,
        generation: GenerationId,
    ) -> Self {
        Self {
            ordinal,
            source_slot,
            source_epoch,
            repository,
            generation,
        }
    }

    /// Returns the deterministic zero-based member ordinal.
    #[must_use]
    pub const fn ordinal(self) -> u16 {
        self.ordinal
    }

    /// Returns the opaque source-slot identity.
    #[must_use]
    pub const fn source_slot(self) -> SourceSlotId {
        self.source_slot
    }

    /// Returns the immutable source-slot epoch captured by the view.
    #[must_use]
    pub const fn source_epoch(self) -> SourceSlotEpoch {
        self.source_epoch
    }

    /// Returns the mapped logical repository identity.
    #[must_use]
    pub const fn repository(self) -> RepositoryIdentityDigest {
        self.repository
    }

    /// Returns the pinned concrete generation.
    #[must_use]
    pub const fn generation(self) -> GenerationId {
        self.generation
    }
}

/// One completed generation receipt for a durable source-slot epoch.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct SourceSlotGeneration {
    source_epoch: SourceSlotEpoch,
    generation: GenerationId,
    snapshot: SourceSnapshotDigest,
}

impl SourceSlotGeneration {
    pub(super) const fn new(
        source_epoch: SourceSlotEpoch,
        generation: GenerationId,
        snapshot: SourceSnapshotDigest,
    ) -> Self {
        Self {
            source_epoch,
            generation,
            snapshot,
        }
    }

    /// Returns the durable source-slot epoch bound by this receipt.
    #[must_use]
    pub const fn source_epoch(self) -> SourceSlotEpoch {
        self.source_epoch
    }

    /// Returns the immutable repository generation.
    #[must_use]
    pub const fn generation(self) -> GenerationId {
        self.generation
    }

    /// Returns the complete canonical source-snapshot digest.
    #[must_use]
    pub const fn snapshot(self) -> SourceSnapshotDigest {
        self.snapshot
    }
}

impl fmt::Debug for SourceSlotGeneration {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SourceSlotGeneration")
            .field("source_epoch", &self.source_epoch)
            .field("generation", &self.generation)
            .field("snapshot", &"<redacted-digest>")
            .finish()
    }
}

/// Durable and active publication state captured for one source slot.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct SourceSlotState {
    current_epoch: SourceSlotEpoch,
    current_completion: Option<SourceSlotGeneration>,
    active: Option<SourceSlotGeneration>,
}

impl SourceSlotState {
    pub(super) const fn new(
        current_epoch: SourceSlotEpoch,
        current_completion: Option<SourceSlotGeneration>,
        active: Option<SourceSlotGeneration>,
    ) -> Self {
        Self {
            current_epoch,
            current_completion,
            active,
        }
    }

    /// Returns the latest durably reserved epoch.
    #[must_use]
    pub const fn current_epoch(self) -> SourceSlotEpoch {
        self.current_epoch
    }

    /// Returns the completion receipt for the current epoch, when present.
    #[must_use]
    pub const fn current_completion(self) -> Option<SourceSlotGeneration> {
        self.current_completion
    }

    /// Returns the generation selected by the active immutable view, when present.
    #[must_use]
    pub const fn active(self) -> Option<SourceSlotGeneration> {
        self.active
    }
}

impl fmt::Debug for SourceSlotState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SourceSlotState")
            .field("current_epoch", &self.current_epoch)
            .field(
                "current_completion_present",
                &self.current_completion.is_some(),
            )
            .field("active_present", &self.active.is_some())
            .finish()
    }
}

/// Complete immutable source set captured for one connected-workspace read.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PinnedWorkspaceView {
    connected_workspace: ConnectedWorkspaceId,
    view: WorkspaceViewId,
    members: Box<[PinnedWorkspaceViewMember]>,
}

impl PinnedWorkspaceView {
    pub(super) fn new(
        connected_workspace: ConnectedWorkspaceId,
        view: WorkspaceViewId,
        members: Vec<PinnedWorkspaceViewMember>,
    ) -> Self {
        Self {
            connected_workspace,
            view,
            members: members.into_boxed_slice(),
        }
    }

    /// Returns the connected-workspace identity.
    #[must_use]
    pub const fn connected_workspace(&self) -> ConnectedWorkspaceId {
        self.connected_workspace
    }

    /// Returns the immutable view identity.
    #[must_use]
    pub const fn view(&self) -> WorkspaceViewId {
        self.view
    }

    /// Returns all members in canonical source-slot order.
    #[must_use]
    pub fn members(&self) -> &[PinnedWorkspaceViewMember] {
        &self.members
    }
}

pub(super) fn canonical_source_slots(
    source_slots: &[WorkspaceSourceSlot],
) -> Result<Vec<WorkspaceSourceSlot>, SqliteStoreError> {
    if source_slots.is_empty() {
        return Err(SqliteStoreError::InvalidWorkspaceMembership);
    }
    if source_slots.len() > MAX_CONNECTED_WORKSPACE_SOURCE_SLOTS {
        return Err(SqliteStoreError::WorkspaceSourceSlotLimitExceeded);
    }
    let mut canonical = source_slots.to_vec();
    canonical.sort_unstable_by_key(|slot| slot.source_slot());
    if canonical
        .windows(2)
        .any(|pair| pair[0].source_slot() == pair[1].source_slot())
    {
        return Err(SqliteStoreError::InvalidWorkspaceMembership);
    }
    Ok(canonical)
}

pub(super) fn canonical_view_members(
    members: &[WorkspaceViewMember],
) -> Result<Vec<WorkspaceViewMember>, SqliteStoreError> {
    if members.is_empty() {
        return Err(SqliteStoreError::InvalidWorkspaceView);
    }
    if members.len() > MAX_CONNECTED_WORKSPACE_SOURCE_SLOTS {
        return Err(SqliteStoreError::WorkspaceSourceSlotLimitExceeded);
    }
    let mut canonical = members.to_vec();
    canonical.sort_unstable_by_key(|member| member.source_slot());
    if canonical
        .windows(2)
        .any(|pair| pair[0].source_slot() == pair[1].source_slot())
    {
        return Err(SqliteStoreError::InvalidWorkspaceView);
    }
    Ok(canonical)
}

#[cfg(test)]
mod tests {
    use repowitness_application::SourceSlotEpoch;
    use repowitness_domain::{ConnectedWorkspaceId, RepositoryIdentityDigest, SourceSlotId};

    use super::{
        MAX_CONNECTED_WORKSPACE_SOURCE_SLOTS, PinnedWorkspaceView, PinnedWorkspaceViewMember,
        WorkspaceSourceSlot, WorkspaceViewId, WorkspaceViewMember, canonical_source_slots,
        canonical_view_members,
    };
    use crate::{GenerationId, SqliteStoreError};

    #[test]
    fn canonical_inputs_sort_and_reject_duplicate_slots() {
        let first = SourceSlotId::new([1; 32]);
        let second = SourceSlotId::new([2; 32]);
        let repository = RepositoryIdentityDigest::new([3; 32]);
        let slots = [
            WorkspaceSourceSlot::new(second, repository),
            WorkspaceSourceSlot::new(first, repository),
        ];
        let members = [
            WorkspaceViewMember::new(second, GenerationId::from_database(2)),
            WorkspaceViewMember::new(first, GenerationId::from_database(1)),
        ];

        assert_eq!(
            canonical_source_slots(&slots)
                .expect("source slots should canonicalize")
                .iter()
                .map(|slot| slot.source_slot())
                .collect::<Vec<_>>(),
            vec![first, second]
        );
        assert_eq!(
            canonical_view_members(&members)
                .expect("view members should canonicalize")
                .iter()
                .map(|member| member.source_slot())
                .collect::<Vec<_>>(),
            vec![first, second]
        );
        assert_eq!(
            canonical_source_slots(&[slots[0], slots[0]]),
            Err(SqliteStoreError::InvalidWorkspaceMembership)
        );
        assert_eq!(
            canonical_view_members(&[members[0], members[0]]),
            Err(SqliteStoreError::InvalidWorkspaceView)
        );
    }

    #[test]
    fn slot_count_is_bounded_before_persistence() {
        let repository = RepositoryIdentityDigest::new([3; 32]);
        let source_slots = (0..=MAX_CONNECTED_WORKSPACE_SOURCE_SLOTS)
            .map(|ordinal| {
                let mut bytes = [0; 32];
                bytes[..8].copy_from_slice(
                    &u64::try_from(ordinal)
                        .expect("fixture ordinal should fit")
                        .to_be_bytes(),
                );
                WorkspaceSourceSlot::new(SourceSlotId::new(bytes), repository)
            })
            .collect::<Vec<_>>();

        assert_eq!(
            canonical_source_slots(&source_slots),
            Err(SqliteStoreError::WorkspaceSourceSlotLimitExceeded)
        );
    }

    #[test]
    fn pinned_view_debug_is_path_free_and_identity_redacted() {
        let workspace = ConnectedWorkspaceId::new([0xA5; 32]);
        let member = PinnedWorkspaceViewMember::new(
            0,
            SourceSlotId::new([0xB6; 32]),
            SourceSlotEpoch::INITIAL,
            RepositoryIdentityDigest::new([0xC7; 32]),
            GenerationId::from_database(1),
        );
        let view =
            PinnedWorkspaceView::new(workspace, WorkspaceViewId::from_database(1), vec![member]);
        let debug = format!("{view:?}");

        assert!(!debug.contains("A5"));
        assert!(!debug.contains("B6"));
        assert!(!debug.contains("C7"));
        assert!(!debug.contains('/'));
    }
}
