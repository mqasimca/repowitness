//! Contained one-shot publication of a validated SCIP precision overlay.

use std::{
    collections::BTreeMap,
    error::Error,
    fmt,
    path::Path,
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};

use repowitness_analysis::{
    MAX_SCIP_OVERLAY_INPUT_BYTES, ScipImmutableSourceLookup, ScipOverlayDocumentError,
};
use repowitness_application::{
    ConnectedWorkspaceIdTextV1, ScipOverlayIdentityInput, ScipOverlayImportError,
    ScipOverlayImportRequest, SourceSlotIdTextV1, bounded_scip_importer_digest, hash_scip_input,
    prepare_scip_overlay_import, reviewed_scip_schema_digest,
};
use repowitness_domain::{RepositoryPath, RepositoryPathLimits};

use crate::{
    BoundedFileReadError, ConnectedWorkspaceId, LocalRustIndexLimits,
    LocalSourceSnapshotFenceError, OwnedSqliteIndex, OwnedSqliteReader, PreparedScipOverlay,
    ScipOverlayPreparationError, ScipOverlaySummary, SourceSlotId, SqliteStoreError,
    read_bounded_regular_file_with_parent,
    rust_index::{
        LocalSourceSnapshotFenceRequest, SourceLanguageSelection,
        capture_confirmed_local_source_snapshot, confirm_local_source_snapshot,
    },
    sqlite::database_file_identity,
};

/// Maximum admitted bytes in one hostile local SCIP input file.
pub const MAX_LOCAL_SCIP_IMPORT_INPUT_BYTES: usize = MAX_SCIP_OVERLAY_INPUT_BYTES;
/// Default end-to-end deadline for one contained local SCIP import.
pub const DEFAULT_LOCAL_SCIP_IMPORT_DEADLINE: Duration = Duration::from_secs(120);
/// Hard ceiling for one contained local SCIP import.
pub const MAX_LOCAL_SCIP_IMPORT_DEADLINE: Duration = Duration::from_secs(300);

const PERSISTED_PATH_LIMITS: RepositoryPathLimits = RepositoryPathLimits::new(1_048_576, 1_048_576);

/// Explicit local inputs for one source-slot-scoped SCIP overlay import.
#[must_use = "a local SCIP import request must be executed or deliberately discarded"]
pub struct LocalScipOverlayImportRequest<'a> {
    database: &'a Path,
    root: &'a Path,
    scip_file: &'a Path,
    connected_workspace: &'a str,
    source_slot: &'a str,
    exact_view: Option<i64>,
    deadline: Duration,
}

impl<'a> LocalScipOverlayImportRequest<'a> {
    /// Constructs an active-view import for one explicit connected source slot.
    pub fn new(
        database: &'a Path,
        root: &'a Path,
        scip_file: &'a Path,
        connected_workspace: &'a str,
        source_slot: &'a str,
    ) -> Self {
        Self {
            database,
            root,
            scip_file,
            connected_workspace,
            source_slot,
            exact_view: None,
            deadline: DEFAULT_LOCAL_SCIP_IMPORT_DEADLINE,
        }
    }

    /// Pins an exact active workspace view for the import.
    pub fn with_exact_view(
        mut self,
        workspace_view: i64,
    ) -> Result<Self, LocalScipOverlayImportError> {
        if workspace_view <= 0 {
            return Err(LocalScipOverlayImportError::InvalidSelection);
        }
        self.exact_view = Some(workspace_view);
        Ok(self)
    }

    /// Replaces the complete wall-clock deadline.
    pub const fn with_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = deadline;
        self
    }
}

impl fmt::Debug for LocalScipOverlayImportRequest<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalScipOverlayImportRequest")
            .field("database", &"<redacted-path>")
            .field("root", &"<redacted-path>")
            .field("scip_file", &"<redacted-path>")
            .field("connected_workspace", &"<redacted-identity>")
            .field("source_slot", &"<redacted-identity>")
            .field("exact_view", &self.exact_view)
            .field("deadline", &self.deadline)
            .finish()
    }
}

/// Durable receipt for one completely imported and activated overlay.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalScipOverlayImportResult {
    connected_workspace: ConnectedWorkspaceId,
    workspace_view: i64,
    source_slot: SourceSlotId,
    overlay: ScipOverlaySummary,
}

impl LocalScipOverlayImportResult {
    /// Returns the immutable connected workspace whose active view was used.
    #[must_use]
    pub const fn connected_workspace(self) -> ConnectedWorkspaceId {
        self.connected_workspace
    }

    /// Returns the exact active workspace view used for validation and publication.
    #[must_use]
    pub const fn workspace_view(self) -> i64 {
        self.workspace_view
    }

    /// Returns the exact source slot owning all imported paths.
    #[must_use]
    pub const fn source_slot(self) -> SourceSlotId {
        self.source_slot
    }

    /// Returns the immutable active overlay receipt.
    #[must_use]
    pub const fn overlay(self) -> ScipOverlaySummary {
        self.overlay
    }
}

/// Stable content- and path-redacted local import failure category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalScipOverlayImportFailureCategory {
    /// A workspace or source-slot identity was malformed or non-canonical.
    InvalidIdentity,
    /// An exact requested view was non-positive.
    InvalidSelection,
    /// The complete operation deadline could not be represented.
    DeadlineNotRepresentable,
    /// The input file could not be admitted through the no-follow boundary.
    InputAdmission,
    /// The read-only database owner failed.
    DatabaseRead,
    /// The selected source member or view was unavailable.
    SourceScopeUnavailable,
    /// The database identity could not be recovered.
    DatabaseIdentity,
    /// Exact source capture or confirmation failed.
    SourceFence,
    /// The captured source manifest differed from the selected immutable member.
    SourceManifestMismatch,
    /// The producer input was malformed or semantically unsupported.
    InvalidInput,
    /// The producer input did not match one current source file.
    SourceMismatch,
    /// The producer used a source encoding the importer does not support.
    UnsupportedSourceEncoding,
    /// Cancellation occurred before a complete import result existed.
    Cancelled,
    /// The import deadline elapsed before a complete result existed.
    DeadlineExceeded,
    /// A bounded producer field or collection exceeded its allowance.
    ResourceLimit,
    /// Decoded accounting could not match retained staging facts.
    InconsistentSummary,
    /// Complete decoded documents could not be prepared for publication.
    Preparation,
    /// The input file changed before publication.
    InputChanged,
    /// The writer could not atomically activate the complete overlay.
    Publication,
    /// The writer did not stop cleanly after the outcome was known.
    Shutdown,
}

impl LocalScipOverlayImportFailureCategory {
    /// Returns the stable, path- and content-free wire spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidIdentity => "invalid_identity",
            Self::InvalidSelection => "invalid_selection",
            Self::DeadlineNotRepresentable => "deadline_not_representable",
            Self::InputAdmission => "input_admission",
            Self::DatabaseRead => "database_read",
            Self::SourceScopeUnavailable => "source_scope_unavailable",
            Self::DatabaseIdentity => "database_identity",
            Self::SourceFence => "source_fence",
            Self::SourceManifestMismatch => "source_manifest_mismatch",
            Self::InvalidInput => "invalid_input",
            Self::SourceMismatch => "source_mismatch",
            Self::UnsupportedSourceEncoding => "unsupported_source_encoding",
            Self::Cancelled => "cancelled",
            Self::DeadlineExceeded => "deadline_exceeded",
            Self::ResourceLimit => "resource_limit",
            Self::InconsistentSummary => "inconsistent_summary",
            Self::Preparation => "preparation",
            Self::InputChanged => "input_changed",
            Self::Publication => "publication",
            Self::Shutdown => "shutdown",
        }
    }
}

/// Stable content- and path-redacted local import failure.
#[derive(Debug)]
pub enum LocalScipOverlayImportError {
    /// A workspace or source-slot identity was malformed or non-canonical.
    InvalidIdentity,
    /// An exact requested view was non-positive.
    InvalidSelection,
    /// The absolute deadline could not be represented.
    DeadlineNotRepresentable,
    /// The hostile input file could not be admitted through the no-follow boundary.
    Input(BoundedFileReadError),
    /// The read-only database owner could not start, pin, or stop.
    Reader(SqliteStoreError),
    /// The requested current view/member is unavailable.
    SourceScopeUnavailable,
    /// The database identity used for source alias rejection could not be recovered.
    DatabaseIdentity(SqliteStoreError),
    /// Exact source bytes could not be captured and confirmed.
    SourceFence(LocalSourceSnapshotFenceError),
    /// The captured source manifest differs from the selected immutable member.
    SourceManifestMismatch,
    /// Hostile SCIP input was rejected by the shared application import use case.
    Import(ScipOverlayImportError),
    /// Complete decoded facts could not be prepared for SQLite publication.
    Preparation(ScipOverlayPreparationError),
    /// The admitted SCIP file changed before publication.
    InputChanged(BoundedFileReadError),
    /// The writer could not atomically activate the complete overlay.
    Writer(SqliteStoreError),
    /// The writer did not stop cleanly after the import outcome was known.
    Shutdown(SqliteStoreError),
}

impl LocalScipOverlayImportError {
    /// Returns the stable category without exposing input, source, or path details.
    #[must_use]
    pub const fn category(&self) -> LocalScipOverlayImportFailureCategory {
        match self {
            Self::InvalidIdentity => LocalScipOverlayImportFailureCategory::InvalidIdentity,
            Self::InvalidSelection => LocalScipOverlayImportFailureCategory::InvalidSelection,
            Self::DeadlineNotRepresentable => {
                LocalScipOverlayImportFailureCategory::DeadlineNotRepresentable
            }
            Self::Input(_) => LocalScipOverlayImportFailureCategory::InputAdmission,
            Self::Reader(_) => LocalScipOverlayImportFailureCategory::DatabaseRead,
            Self::SourceScopeUnavailable => {
                LocalScipOverlayImportFailureCategory::SourceScopeUnavailable
            }
            Self::DatabaseIdentity(_) => LocalScipOverlayImportFailureCategory::DatabaseIdentity,
            Self::SourceFence(_) => LocalScipOverlayImportFailureCategory::SourceFence,
            Self::SourceManifestMismatch => {
                LocalScipOverlayImportFailureCategory::SourceManifestMismatch
            }
            Self::Import(ScipOverlayImportError::Cancelled) => {
                LocalScipOverlayImportFailureCategory::Cancelled
            }
            Self::Import(ScipOverlayImportError::DeadlineExceeded) => {
                LocalScipOverlayImportFailureCategory::DeadlineExceeded
            }
            Self::Import(ScipOverlayImportError::Decode(
                ScipOverlayDocumentError::InvalidInput,
            )) => LocalScipOverlayImportFailureCategory::InvalidInput,
            Self::Import(ScipOverlayImportError::Decode(
                ScipOverlayDocumentError::SourceMismatch,
            )) => LocalScipOverlayImportFailureCategory::SourceMismatch,
            Self::Import(ScipOverlayImportError::Decode(
                ScipOverlayDocumentError::UnsupportedSourceEncoding,
            )) => LocalScipOverlayImportFailureCategory::UnsupportedSourceEncoding,
            Self::Import(ScipOverlayImportError::Decode(ScipOverlayDocumentError::Cancelled)) => {
                LocalScipOverlayImportFailureCategory::Cancelled
            }
            Self::Import(ScipOverlayImportError::Decode(
                ScipOverlayDocumentError::DeadlineExceeded,
            )) => LocalScipOverlayImportFailureCategory::DeadlineExceeded,
            Self::Import(ScipOverlayImportError::Decode(
                ScipOverlayDocumentError::ResourceLimit,
            )) => LocalScipOverlayImportFailureCategory::ResourceLimit,
            Self::Writer(SqliteStoreError::Cancelled) => {
                LocalScipOverlayImportFailureCategory::Cancelled
            }
            Self::Writer(SqliteStoreError::DeadlineExceeded) => {
                LocalScipOverlayImportFailureCategory::DeadlineExceeded
            }
            Self::Import(ScipOverlayImportError::InconsistentSummary) => {
                LocalScipOverlayImportFailureCategory::InconsistentSummary
            }
            Self::Preparation(_) => LocalScipOverlayImportFailureCategory::Preparation,
            Self::InputChanged(_) => LocalScipOverlayImportFailureCategory::InputChanged,
            Self::Writer(_) => LocalScipOverlayImportFailureCategory::Publication,
            Self::Shutdown(_) => LocalScipOverlayImportFailureCategory::Shutdown,
        }
    }
}

impl fmt::Display for LocalScipOverlayImportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidIdentity => "SCIP import workspace identity is invalid",
            Self::InvalidSelection => "SCIP import immutable view selection is invalid",
            Self::DeadlineNotRepresentable => "SCIP import deadline cannot be represented",
            Self::Input(_) => "SCIP import input admission failed",
            Self::Reader(_) => "SCIP import database read failed",
            Self::SourceScopeUnavailable => {
                "SCIP import source scope is unavailable or no longer active"
            }
            Self::DatabaseIdentity(_) => "SCIP import database identity could not be confirmed",
            Self::SourceFence(_) => "SCIP import source snapshot changed or could not be confirmed",
            Self::SourceManifestMismatch => {
                "SCIP import source manifest differs from the selected view"
            }
            Self::Import(_) => "SCIP import input was rejected",
            Self::Preparation(_) => "SCIP import prepared facts are invalid",
            Self::InputChanged(_) => "SCIP import input changed before publication",
            Self::Writer(_) => "SCIP import publication failed",
            Self::Shutdown(_) => "SCIP import writer shutdown failed",
        })
    }
}

impl Error for LocalScipOverlayImportError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Input(error) | Self::InputChanged(error) => Some(error),
            Self::Reader(error)
            | Self::DatabaseIdentity(error)
            | Self::Writer(error)
            | Self::Shutdown(error) => Some(error),
            Self::SourceFence(error) => Some(error),
            Self::Import(error) => Some(error),
            Self::Preparation(error) => Some(error),
            Self::InvalidIdentity
            | Self::InvalidSelection
            | Self::DeadlineNotRepresentable
            | Self::SourceScopeUnavailable
            | Self::SourceManifestMismatch => None,
        }
    }
}

struct CapturedSources(BTreeMap<RepositoryPath, Box<[u8]>>);

impl ScipImmutableSourceLookup for CapturedSources {
    fn source_bytes(&self, path: &RepositoryPath) -> Option<&[u8]> {
        self.0.get(path).map(Box::as_ref)
    }
}

/// Admits, validates, and atomically activates one source-slot-scoped SCIP overlay.
pub fn import_local_scip_overlay(
    request: LocalScipOverlayImportRequest<'_>,
    cancelled: Arc<AtomicBool>,
) -> Result<LocalScipOverlayImportResult, LocalScipOverlayImportError> {
    let connected_workspace = ConnectedWorkspaceIdTextV1::decode(request.connected_workspace)
        .map_err(|_| LocalScipOverlayImportError::InvalidIdentity)?;
    let source_slot = SourceSlotIdTextV1::decode(request.source_slot)
        .map_err(|_| LocalScipOverlayImportError::InvalidIdentity)?;
    let deadline = Instant::now()
        .checked_add(request.deadline)
        .ok_or(LocalScipOverlayImportError::DeadlineNotRepresentable)?;
    let (input, admitted_input) =
        read_bounded_regular_file_with_parent(request.scip_file, MAX_LOCAL_SCIP_IMPORT_INPUT_BYTES)
            .map_err(LocalScipOverlayImportError::Input)?;
    let reader = OwnedSqliteReader::start(request.database, deadline)
        .map_err(LocalScipOverlayImportError::Reader)?;
    let result = import_with_reader(
        &reader,
        request.database,
        request.root,
        connected_workspace,
        source_slot,
        request.exact_view,
        input.bytes(),
        &admitted_input,
        &cancelled,
        deadline,
    );
    let shutdown = reader.shutdown(deadline);
    match (result, shutdown) {
        (Ok(result), Ok(())) => Ok(result),
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(LocalScipOverlayImportError::Shutdown(error)),
    }
}

#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "each source, database, file, scope, and control authority remains explicit"
)]
fn import_with_reader(
    reader: &OwnedSqliteReader,
    database: &Path,
    root: &Path,
    connected_workspace: ConnectedWorkspaceId,
    source_slot: SourceSlotId,
    exact_view: Option<i64>,
    input: &[u8],
    admitted_input: &crate::AdmittedFileParent,
    cancelled: &Arc<AtomicBool>,
    deadline: Instant,
) -> Result<LocalScipOverlayImportResult, LocalScipOverlayImportError> {
    let view = reader
        .pin_workspace_view(
            connected_workspace,
            exact_view,
            Arc::clone(cancelled),
            deadline,
        )
        .map_err(LocalScipOverlayImportError::Reader)?
        .ok_or(LocalScipOverlayImportError::SourceScopeUnavailable)?;
    let scope = reader
        .scip_import_scope(&view, source_slot, Arc::clone(cancelled), deadline)
        .map_err(|error| match error {
            SqliteStoreError::InvalidWorkspaceView | SqliteStoreError::DatabaseOperationFailed => {
                LocalScipOverlayImportError::SourceScopeUnavailable
            }
            error => LocalScipOverlayImportError::Reader(error),
        })?;
    let database_identity =
        database_file_identity(database).map_err(LocalScipOverlayImportError::DatabaseIdentity)?;
    let fence_request = || {
        LocalSourceSnapshotFenceRequest::new(
            root,
            scope.source_identity(),
            scope.source_snapshot(),
            SourceLanguageSelection::all(),
            LocalRustIndexLimits::default(),
            cancelled.as_ref(),
            deadline,
            database_identity.as_ref(),
        )
    };
    let captured = capture_confirmed_local_source_snapshot(fence_request())
        .map_err(LocalScipOverlayImportError::SourceFence)?;
    let manifest = captured.manifest();
    if repowitness_application::hash_source_manifest(manifest) != scope.source_manifest() {
        return Err(LocalScipOverlayImportError::SourceManifestMismatch);
    }
    let sources = CapturedSources(
        captured
            .sources()
            .iter()
            .map(|source| {
                (
                    source.path().clone(),
                    source.content().to_vec().into_boxed_slice(),
                )
            })
            .collect(),
    );
    let prepared = prepare_scip_overlay_import(ScipOverlayImportRequest::new(
        input,
        manifest,
        PERSISTED_PATH_LIMITS,
        &sources,
        ScipOverlayIdentityInput::new(
            scope
                .overlay_scope_identity()
                .map_err(LocalScipOverlayImportError::Reader)?,
            scope.source_snapshot(),
            scope.source_manifest(),
            scope.source_identity().configuration(),
            scope.source_identity().producer_manifest(),
            reviewed_scip_schema_digest(),
            bounded_scip_importer_digest(),
            hash_scip_input(input),
        ),
        cancelled.as_ref(),
        deadline,
    ))
    .map_err(LocalScipOverlayImportError::Import)?;
    let decoded = prepared.summary();
    let overlay = PreparedScipOverlay::try_new(prepared.identity(), prepared.into_documents())
        .map_err(LocalScipOverlayImportError::Preparation)?;
    if decoded.documents() != u32::try_from(overlay.documents().len()).unwrap_or(u32::MAX)
        || decoded.occurrences() != overlay.occurrence_count()
        || decoded.relationships() != overlay.relationship_count()
    {
        return Err(LocalScipOverlayImportError::Preparation(
            ScipOverlayPreparationError::InvalidDocuments,
        ));
    }
    confirm_local_source_snapshot(fence_request())
        .map_err(LocalScipOverlayImportError::SourceFence)?;
    admitted_input
        .revalidate()
        .map_err(LocalScipOverlayImportError::InputChanged)?;
    let (writer, _) = OwnedSqliteIndex::start(database, 0, deadline)
        .map_err(LocalScipOverlayImportError::Writer)?;
    let staged = writer.stage_current_scip_overlay(
        scope.connected_workspace(),
        scope.workspace_view(),
        scope.source_slot(),
        overlay,
        Arc::clone(cancelled),
        deadline,
    );
    let shutdown = writer.shutdown(deadline);
    let digest = match (staged, shutdown) {
        (Ok(digest), Ok(())) => digest,
        (Err(error), _) => return Err(LocalScipOverlayImportError::Writer(error)),
        (Ok(_), Err(error)) => return Err(LocalScipOverlayImportError::Shutdown(error)),
    };
    Ok(LocalScipOverlayImportResult {
        connected_workspace: scope.connected_workspace(),
        workspace_view: scope.workspace_view().get(),
        source_slot: scope.source_slot(),
        overlay: ScipOverlaySummary::new(
            digest,
            scope.source_slot(),
            u64::from(decoded.documents()),
            decoded.occurrences(),
            decoded.relationships(),
        ),
    })
}

#[cfg(test)]
mod local_scip_overlay_import_tests {
    use super::*;

    #[test]
    fn failure_categories_preserve_the_safe_import_cause() {
        let source_mismatch = LocalScipOverlayImportError::Import(ScipOverlayImportError::Decode(
            ScipOverlayDocumentError::SourceMismatch,
        ));
        let invalid_input = LocalScipOverlayImportError::Import(ScipOverlayImportError::Decode(
            ScipOverlayDocumentError::InvalidInput,
        ));
        let preparation =
            LocalScipOverlayImportError::Preparation(ScipOverlayPreparationError::InvalidDocuments);
        let writer_deadline =
            LocalScipOverlayImportError::Writer(SqliteStoreError::DeadlineExceeded);

        assert_eq!(source_mismatch.category().as_str(), "source_mismatch");
        assert_eq!(invalid_input.category().as_str(), "invalid_input");
        assert_eq!(preparation.category().as_str(), "preparation");
        assert_eq!(writer_deadline.category().as_str(), "deadline_exceeded");
    }

    #[test]
    fn scip_input_admission_matches_the_decoder_ceiling() {
        assert_eq!(
            MAX_LOCAL_SCIP_IMPORT_INPUT_BYTES,
            MAX_SCIP_OVERLAY_INPUT_BYTES
        );
    }
}
