use core::fmt;
use std::collections::BTreeSet;

use repowitness_domain::ConfigurationDigest;

use crate::local_index::connected_workspace::{
    CONNECTED_WORKSPACE_VIEW_RECEIPT_VERSION, PostCommitMaintenanceStatus,
    model::ConnectedWorkspaceIndexReport as InternalReport,
};

/// Version of the aggregate-only connected-workspace facade report.
pub const LOCAL_CONNECTED_WORKSPACE_REPORT_VERSION: u16 = 1;

/// Canonical semantic digest of one published connected-workspace view.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct LocalConnectedWorkspaceViewDigest([u8; 32]);

impl LocalConnectedWorkspaceViewDigest {
    pub(super) const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Returns the exact SHA-256 digest bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for LocalConnectedWorkspaceViewDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalConnectedWorkspaceViewDigest")
            .field("algorithm", &"SHA-256")
            .finish_non_exhaustive()
    }
}

impl fmt::Display for LocalConnectedWorkspaceViewDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// Explicit aggregate path and source-analysis coverage.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalConnectedWorkspaceCoverage {
    discovered_paths: u64,
    indexed_files: u64,
    skipped_policy_paths: u64,
    skipped_unsupported_paths: u64,
    reused_files: u64,
    analyzed_files: u64,
}

impl LocalConnectedWorkspaceCoverage {
    /// Returns every Git-discovered path across all source slots.
    #[must_use]
    pub const fn discovered_paths(self) -> u64 {
        self.discovered_paths
    }

    /// Returns every supported-language file selected for indexing.
    #[must_use]
    pub const fn indexed_files(self) -> u64 {
        self.indexed_files
    }

    /// Returns supported-language paths omitted by effective policy.
    #[must_use]
    pub const fn skipped_policy_paths(self) -> u64 {
        self.skipped_policy_paths
    }

    /// Returns paths omitted because no built-in adapter supports them.
    #[must_use]
    pub const fn skipped_unsupported_paths(self) -> u64 {
        self.skipped_unsupported_paths
    }

    /// Returns every file restored from an exact reusable artifact.
    #[must_use]
    pub const fn reused_files(self) -> u64 {
        self.reused_files
    }

    /// Returns every file analyzed by the current producer.
    #[must_use]
    pub const fn analyzed_files(self) -> u64 {
        self.analyzed_files
    }

    /// Returns all policy and unsupported-language omissions.
    #[must_use]
    pub const fn skipped_paths(self) -> u64 {
        self.skipped_policy_paths + self.skipped_unsupported_paths
    }
}

/// Categorical publication outcome for a successful facade call.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalConnectedWorkspaceOutcome {
    /// One complete immutable view was atomically published and activated.
    Published,
}

impl LocalConnectedWorkspaceOutcome {
    /// Returns the stable terminal-facing category.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Published => "published",
        }
    }
}

/// Bounded maintenance observation after view publication committed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalConnectedWorkspaceMaintenance {
    /// Checkpoint and writer shutdown both completed.
    Complete,
    /// View publication committed, but the WAL checkpoint was deferred.
    CheckpointDeferred,
    /// View publication committed, but writer shutdown was deferred.
    ShutdownDeferred,
    /// View publication committed, but both maintenance steps were deferred.
    CheckpointAndShutdownDeferred,
}

impl LocalConnectedWorkspaceMaintenance {
    /// Returns the stable terminal-facing category.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::CheckpointDeferred => "checkpoint_deferred",
            Self::ShutdownDeferred => "shutdown_deferred",
            Self::CheckpointAndShutdownDeferred => "checkpoint_and_shutdown_deferred",
        }
    }
}

/// Aggregate-only receipt for one atomically published connected workspace.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalConnectedWorkspaceIndexReport {
    manifest_schema_version: u16,
    view_receipt_version: u16,
    configuration_digest: ConfigurationDigest,
    view_digest: LocalConnectedWorkspaceViewDigest,
    source_count: u64,
    generation_count: u64,
    recovered_generations: u64,
    coverage: LocalConnectedWorkspaceCoverage,
    outcome: LocalConnectedWorkspaceOutcome,
    maintenance: LocalConnectedWorkspaceMaintenance,
}

impl LocalConnectedWorkspaceIndexReport {
    pub(super) fn from_internal(manifest_schema_version: u16, report: InternalReport) -> Self {
        let source_count =
            u64::try_from(report.source_slots().len()).expect("source-slot bound fits u64");
        let generation_count = u64::try_from(
            report
                .source_slots()
                .iter()
                .map(|source| source.generation())
                .collect::<BTreeSet<_>>()
                .len(),
        )
        .expect("source-slot bound fits u64");
        let coverage = LocalConnectedWorkspaceCoverage {
            discovered_paths: aggregate(report.source_slots(), |source| source.discovered_paths()),
            indexed_files: aggregate(report.source_slots(), |source| source.indexed_files()),
            skipped_policy_paths: aggregate(report.source_slots(), |source| {
                source.skipped_policy_paths()
            }),
            skipped_unsupported_paths: aggregate(report.source_slots(), |source| {
                source.skipped_unsupported_paths()
            }),
            reused_files: aggregate(report.source_slots(), |source| source.reused_files()),
            analyzed_files: aggregate(report.source_slots(), |source| source.analyzed_files()),
        };
        Self {
            manifest_schema_version,
            view_receipt_version: CONNECTED_WORKSPACE_VIEW_RECEIPT_VERSION,
            configuration_digest: report.configuration_digest(),
            view_digest: LocalConnectedWorkspaceViewDigest::new(
                *report.view_receipt_digest().as_bytes(),
            ),
            source_count,
            generation_count,
            recovered_generations: report.recovered_generations(),
            coverage,
            outcome: LocalConnectedWorkspaceOutcome::Published,
            maintenance: map_maintenance(report.maintenance()),
        }
    }

    /// Returns the aggregate report contract version.
    #[must_use]
    pub const fn report_version(self) -> u16 {
        LOCAL_CONNECTED_WORKSPACE_REPORT_VERSION
    }

    /// Returns the strict manifest schema version that was admitted.
    #[must_use]
    pub const fn manifest_schema_version(self) -> u16 {
        self.manifest_schema_version
    }

    /// Returns the canonical semantic view-receipt version.
    #[must_use]
    pub const fn view_receipt_version(self) -> u16 {
        self.view_receipt_version
    }

    /// Returns the one shared resolved-configuration digest.
    #[must_use]
    pub const fn configuration_digest(self) -> ConfigurationDigest {
        self.configuration_digest
    }

    /// Returns the canonical semantic digest of the published view.
    #[must_use]
    pub const fn view_digest(self) -> LocalConnectedWorkspaceViewDigest {
        self.view_digest
    }

    /// Returns the exact source-slot count.
    #[must_use]
    pub const fn source_count(self) -> u64 {
        self.source_count
    }

    /// Returns the exact distinct immutable-generation count.
    #[must_use]
    pub const fn generation_count(self) -> u64 {
        self.generation_count
    }

    /// Returns incomplete generations recovered during writer startup.
    #[must_use]
    pub const fn recovered_generations(self) -> u64 {
        self.recovered_generations
    }

    /// Returns complete aggregate path and analysis coverage.
    #[must_use]
    pub const fn coverage(self) -> LocalConnectedWorkspaceCoverage {
        self.coverage
    }

    /// Returns the committed publication outcome.
    #[must_use]
    pub const fn outcome(self) -> LocalConnectedWorkspaceOutcome {
        self.outcome
    }

    /// Returns bounded post-commit maintenance status.
    #[must_use]
    pub const fn maintenance(self) -> LocalConnectedWorkspaceMaintenance {
        self.maintenance
    }
}

fn aggregate(
    sources: &[crate::local_index::connected_workspace::model::ConnectedSourceSlotReport],
    value: impl Fn(crate::local_index::connected_workspace::model::ConnectedSourceSlotReport) -> u64,
) -> u64 {
    sources.iter().copied().fold(0_u64, |total, source| {
        total
            .checked_add(value(source))
            .expect("admitted aggregate path bound prevents report overflow")
    })
}

const fn map_maintenance(
    status: PostCommitMaintenanceStatus,
) -> LocalConnectedWorkspaceMaintenance {
    match status {
        PostCommitMaintenanceStatus::Complete => LocalConnectedWorkspaceMaintenance::Complete,
        PostCommitMaintenanceStatus::CheckpointDeferred => {
            LocalConnectedWorkspaceMaintenance::CheckpointDeferred
        }
        PostCommitMaintenanceStatus::ShutdownDeferred => {
            LocalConnectedWorkspaceMaintenance::ShutdownDeferred
        }
        PostCommitMaintenanceStatus::CheckpointAndShutdownDeferred => {
            LocalConnectedWorkspaceMaintenance::CheckpointAndShutdownDeferred
        }
    }
}
