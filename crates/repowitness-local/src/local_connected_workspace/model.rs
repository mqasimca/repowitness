use core::fmt;
use std::{path::Path, time::Duration};

use repowitness_application::ResolvedConfiguration;
use repowitness_domain::ConfigurationDigest;

use crate::{
    AdmittedFileParent, LocalRustIndexLimits,
    source_selector::{
        DEFAULT_SOURCE_SELECTOR_DEADLINE, DEFAULT_SOURCE_SELECTOR_OUTPUT_BYTES,
        SourceSelectorLimits,
    },
};

use super::{LocalConnectedWorkspaceIndexError, LocalConnectedWorkspaceRequestErrorKind};

/// Default shared deadline for every source slot participating in one atomic
/// connected-workspace attempt.
///
/// The coordinator prepares and publishes source slots serially so that all
/// readers retain a single immutable view. A source deadline shorter than the
/// encompassing attempt can therefore expire while another declared source is
/// being prepared. Keep the defaults identical and large enough for a normal
/// local product-stack refresh, while retaining one explicit, finite bound.
pub const DEFAULT_LOCAL_CONNECTED_WORKSPACE_SOURCE_DEADLINE: Duration = Duration::from_secs(300);
/// Default deadline for one complete atomic connected-workspace attempt.
pub const DEFAULT_LOCAL_CONNECTED_WORKSPACE_DEADLINE: Duration = Duration::from_secs(300);

/// Complete per-source indexing, selector, and coordinator resource bounds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalConnectedWorkspaceSourceLimits {
    index: LocalRustIndexLimits,
    selector_deadline: Duration,
    selector_output_bytes: u64,
    deadline: Duration,
}

impl LocalConnectedWorkspaceSourceLimits {
    /// Validates all per-source deadlines and the selector output bound.
    pub fn try_new(
        index: LocalRustIndexLimits,
        selector_deadline: Duration,
        selector_output_bytes: u64,
        deadline: Duration,
    ) -> Result<Self, LocalConnectedWorkspaceIndexError> {
        if deadline.is_zero()
            || selector_deadline.is_zero()
            || index.deadline().is_zero()
            || index.discovery().deadline().is_zero()
            || index.source_read().deadline().is_zero()
        {
            return Err(LocalConnectedWorkspaceIndexError::InvalidRequest {
                kind: LocalConnectedWorkspaceRequestErrorKind::Deadline,
            });
        }
        if selector_output_bytes == 0 {
            return Err(LocalConnectedWorkspaceIndexError::InvalidRequest {
                kind: LocalConnectedWorkspaceRequestErrorKind::ResourceLimit,
            });
        }
        Ok(Self {
            index,
            selector_deadline,
            selector_output_bytes,
            deadline,
        })
    }

    /// Returns the complete local supported-language indexing limits.
    #[must_use]
    pub const fn index(self) -> LocalRustIndexLimits {
        self.index
    }

    /// Returns the wall-clock bound for one selector subprocess sequence.
    #[must_use]
    pub const fn selector_deadline(self) -> Duration {
        self.selector_deadline
    }

    /// Returns the inclusive output bound for each selector subprocess.
    #[must_use]
    pub const fn selector_output_bytes(self) -> u64 {
        self.selector_output_bytes
    }

    /// Returns the complete per-source coordinator deadline.
    #[must_use]
    pub const fn deadline(self) -> Duration {
        self.deadline
    }

    pub(super) const fn selector_limits(self) -> SourceSelectorLimits {
        SourceSelectorLimits::new(self.selector_deadline, self.selector_output_bytes)
    }
}

impl Default for LocalConnectedWorkspaceSourceLimits {
    fn default() -> Self {
        Self {
            index: LocalRustIndexLimits::default(),
            selector_deadline: DEFAULT_SOURCE_SELECTOR_DEADLINE,
            selector_output_bytes: DEFAULT_SOURCE_SELECTOR_OUTPUT_BYTES,
            deadline: DEFAULT_LOCAL_CONNECTED_WORKSPACE_SOURCE_DEADLINE,
        }
    }
}

/// Complete explicit input for one atomic manifest-backed workspace index.
pub struct LocalConnectedWorkspaceIndexRequest<'a> {
    pub(super) manifest_bytes: &'a [u8],
    pub(super) manifest_parent: &'a AdmittedFileParent,
    pub(super) database: &'a Path,
    pub(super) configuration: &'a ResolvedConfiguration,
    pub(super) migration_applied_at_unix_ms: u64,
    pub(super) source_limits: LocalConnectedWorkspaceSourceLimits,
    pub(super) deadline: Duration,
}

impl<'a> LocalConnectedWorkspaceIndexRequest<'a> {
    /// Constructs one request with conservative source and whole-operation bounds.
    #[must_use]
    pub fn new(
        manifest_bytes: &'a [u8],
        manifest_parent: &'a AdmittedFileParent,
        database: &'a Path,
        configuration: &'a ResolvedConfiguration,
        migration_applied_at_unix_ms: u64,
    ) -> Self {
        Self {
            manifest_bytes,
            manifest_parent,
            database,
            configuration,
            migration_applied_at_unix_ms,
            source_limits: LocalConnectedWorkspaceSourceLimits::default(),
            deadline: DEFAULT_LOCAL_CONNECTED_WORKSPACE_DEADLINE,
        }
    }

    /// Applies validated per-source bounds.
    #[must_use]
    pub const fn with_source_limits(
        mut self,
        source_limits: LocalConnectedWorkspaceSourceLimits,
    ) -> Self {
        self.source_limits = source_limits;
        self
    }

    /// Applies a positive whole-operation deadline.
    pub fn with_deadline(
        mut self,
        deadline: Duration,
    ) -> Result<Self, LocalConnectedWorkspaceIndexError> {
        if deadline.is_zero() {
            return Err(LocalConnectedWorkspaceIndexError::InvalidRequest {
                kind: LocalConnectedWorkspaceRequestErrorKind::Deadline,
            });
        }
        self.deadline = deadline;
        Ok(self)
    }

    /// Returns the admitted manifest byte count without exposing content.
    #[must_use]
    pub fn manifest_byte_count(&self) -> usize {
        self.manifest_bytes.len()
    }

    /// Returns the exact shared resolved-configuration digest.
    #[must_use]
    pub const fn configuration_digest(&self) -> ConfigurationDigest {
        self.configuration.digest()
    }

    /// Returns the migration timestamp applied to a newly created database.
    #[must_use]
    pub const fn migration_applied_at_unix_ms(&self) -> u64 {
        self.migration_applied_at_unix_ms
    }

    /// Returns complete per-source resource bounds.
    #[must_use]
    pub const fn source_limits(&self) -> LocalConnectedWorkspaceSourceLimits {
        self.source_limits
    }

    /// Returns the whole-operation deadline.
    #[must_use]
    pub const fn deadline(&self) -> Duration {
        self.deadline
    }
}

impl fmt::Debug for LocalConnectedWorkspaceIndexRequest<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalConnectedWorkspaceIndexRequest")
            .field("manifest_byte_count", &self.manifest_bytes.len())
            .field("manifest_parent", &"<admitted-parent>")
            .field("database", &"<redacted-path>")
            .field("configuration_digest", &self.configuration.digest())
            .field(
                "migration_applied_at_unix_ms",
                &self.migration_applied_at_unix_ms,
            )
            .field("source_limits", &self.source_limits)
            .field("deadline", &self.deadline)
            .finish()
    }
}
