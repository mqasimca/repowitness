use core::fmt;

use repowitness_application::SourceSlotEpoch;
use repowitness_domain::{
    ConfigurationDigest, ConnectedWorkspaceId, RepositoryIdentityDigest, SourceSlotId,
    SourceSnapshotDigest,
};
use sha2::{Digest, Sha256};

const CONNECTED_WORKSPACE_VIEW_RECEIPT_DOMAIN: &[u8] =
    b"RepoWitness\0connected-workspace-view-receipt\0";
pub(crate) const CONNECTED_WORKSPACE_VIEW_RECEIPT_VERSION: u16 = 1;

/// Canonical semantic receipt for one published connected-workspace view.
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub(crate) struct ConnectedWorkspaceViewDigest([u8; 32]);

impl ConnectedWorkspaceViewDigest {
    pub(crate) const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for ConnectedWorkspaceViewDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ConnectedWorkspaceViewDigest")
            .field("algorithm", &"SHA-256")
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy)]
pub(super) struct CanonicalViewMemberReceipt {
    source_slot: SourceSlotId,
    source_epoch: SourceSlotEpoch,
    repository: RepositoryIdentityDigest,
    source_snapshot: SourceSnapshotDigest,
    configuration: ConfigurationDigest,
}

impl CanonicalViewMemberReceipt {
    pub(super) const fn new(
        source_slot: SourceSlotId,
        source_epoch: SourceSlotEpoch,
        repository: RepositoryIdentityDigest,
        source_snapshot: SourceSnapshotDigest,
        configuration: ConfigurationDigest,
    ) -> Self {
        Self {
            source_slot,
            source_epoch,
            repository,
            source_snapshot,
            configuration,
        }
    }
}

pub(super) fn canonical_view_receipt_digest(
    connected_workspace: ConnectedWorkspaceId,
    members: &[CanonicalViewMemberReceipt],
) -> ConnectedWorkspaceViewDigest {
    let mut canonical = members.to_vec();
    canonical.sort_unstable_by_key(|member| member.source_slot);

    let mut hasher = Sha256::new();
    hasher.update(CONNECTED_WORKSPACE_VIEW_RECEIPT_DOMAIN);
    hasher.update(CONNECTED_WORKSPACE_VIEW_RECEIPT_VERSION.to_be_bytes());
    hasher.update(connected_workspace.as_bytes());
    hasher.update(
        u64::try_from(canonical.len())
            .expect("connected-workspace member bound fits u64")
            .to_be_bytes(),
    );
    for member in canonical {
        hasher.update(member.source_slot.as_bytes());
        hasher.update(member.source_epoch.get().to_be_bytes());
        hasher.update(member.repository.as_bytes());
        hasher.update(member.source_snapshot.as_bytes());
        hasher.update(member.configuration.as_bytes());
    }
    ConnectedWorkspaceViewDigest(hasher.finalize().into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(value: u8) -> CanonicalViewMemberReceipt {
        CanonicalViewMemberReceipt::new(
            SourceSlotId::new([value; 32]),
            SourceSlotEpoch::try_new(u64::from(value)).expect("test epoch should validate"),
            RepositoryIdentityDigest::new([value.wrapping_add(1); 32]),
            SourceSnapshotDigest::new([value.wrapping_add(2); 32]),
            ConfigurationDigest::new([value.wrapping_add(3); 32]),
        )
    }

    #[test]
    fn receipt_is_order_independent_and_changes_for_every_semantic_member_input() {
        let workspace = ConnectedWorkspaceId::new([9; 32]);
        let first = member(1);
        let second = member(2);
        let baseline = canonical_view_receipt_digest(workspace, &[first, second]);
        assert_eq!(
            baseline,
            canonical_view_receipt_digest(workspace, &[second, first])
        );

        let variants = [
            (ConnectedWorkspaceId::new([8; 32]), vec![first, second]),
            (workspace, vec![member(3), second]),
            (
                workspace,
                vec![
                    CanonicalViewMemberReceipt::new(
                        first.source_slot,
                        SourceSlotEpoch::try_new(first.source_epoch.get() + 1)
                            .expect("test epoch should validate"),
                        first.repository,
                        first.source_snapshot,
                        first.configuration,
                    ),
                    second,
                ],
            ),
            (
                workspace,
                vec![
                    CanonicalViewMemberReceipt::new(
                        first.source_slot,
                        first.source_epoch,
                        RepositoryIdentityDigest::new([7; 32]),
                        first.source_snapshot,
                        first.configuration,
                    ),
                    second,
                ],
            ),
            (
                workspace,
                vec![
                    CanonicalViewMemberReceipt::new(
                        first.source_slot,
                        first.source_epoch,
                        first.repository,
                        SourceSnapshotDigest::new([7; 32]),
                        first.configuration,
                    ),
                    second,
                ],
            ),
            (
                workspace,
                vec![
                    CanonicalViewMemberReceipt::new(
                        first.source_slot,
                        first.source_epoch,
                        first.repository,
                        first.source_snapshot,
                        ConfigurationDigest::new([7; 32]),
                    ),
                    second,
                ],
            ),
        ];
        for (workspace, members) in variants {
            assert_ne!(baseline, canonical_view_receipt_digest(workspace, &members));
        }
    }

    #[test]
    fn receipt_debug_is_redacted() {
        let digest =
            canonical_view_receipt_digest(ConnectedWorkspaceId::new([0xA5; 32]), &[member(0xB6)]);
        let debug = format!("{digest:?}");
        assert!(debug.contains("SHA-256"));
        assert!(!debug.contains("A5"));
        assert!(!debug.contains("B6"));
    }
}
