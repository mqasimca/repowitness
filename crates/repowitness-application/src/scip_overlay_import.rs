//! Storage-neutral preparation of one immutable SCIP precision overlay.

use std::{
    error::Error,
    fmt,
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

use repowitness_analysis::{
    ScipImmutableSourceLookup, ScipOverlayDocument, ScipOverlayDocumentError,
    ScipOverlayIndexSummary, decode_scip_overlay_index,
};
use repowitness_domain::{
    RepositoryPath, RepositoryPathLimits, SourceContentDigest, SourceFileKind, SourceManifest,
};

use crate::ScipOverlayIdentityInput;

/// All trusted and hostile inputs required to prepare one complete overlay.
#[must_use = "an import request must be executed or deliberately discarded"]
pub struct ScipOverlayImportRequest<'a, SourceLookup> {
    input: &'a [u8],
    source_manifest: &'a SourceManifest<RepositoryPath, SourceFileKind, SourceContentDigest>,
    path_limits: RepositoryPathLimits,
    source_lookup: &'a SourceLookup,
    identity: ScipOverlayIdentityInput,
    cancelled: &'a AtomicBool,
    deadline: Instant,
}

impl<'a, SourceLookup> ScipOverlayImportRequest<'a, SourceLookup> {
    /// Constructs a request from one already-pinned immutable source view.
    #[allow(
        clippy::too_many_arguments,
        reason = "hostile input, source authority, identity, and operation control are independent"
    )]
    pub const fn new(
        input: &'a [u8],
        source_manifest: &'a SourceManifest<RepositoryPath, SourceFileKind, SourceContentDigest>,
        path_limits: RepositoryPathLimits,
        source_lookup: &'a SourceLookup,
        identity: ScipOverlayIdentityInput,
        cancelled: &'a AtomicBool,
        deadline: Instant,
    ) -> Self {
        Self {
            input,
            source_manifest,
            path_limits,
            source_lookup,
            identity,
            cancelled,
            deadline,
        }
    }
}

impl<SourceLookup> fmt::Debug for ScipOverlayImportRequest<'_, SourceLookup> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ScipOverlayImportRequest")
            .field("input", &"<redacted-scip-input>")
            .field("source_manifest", &"<pinned-manifest>")
            .field("path_limits", &self.path_limits)
            .field("identity", &self.identity)
            .field("cancelled", &self.cancelled.load(Ordering::Acquire))
            .field("deadline", &"<monotonic>")
            .finish_non_exhaustive()
    }
}

/// Complete provisional facts ready for one local atomic publication.
#[derive(Debug)]
pub struct PreparedScipOverlayImport {
    identity: ScipOverlayIdentityInput,
    summary: ScipOverlayIndexSummary,
    documents: Vec<ScipOverlayDocument>,
}

impl PreparedScipOverlayImport {
    /// Returns the exact immutable identity inputs for the publication receipt.
    #[must_use]
    pub const fn identity(&self) -> ScipOverlayIdentityInput {
        self.identity
    }

    /// Returns bounded decode accounting for diagnostics and publication checks.
    #[must_use]
    pub const fn summary(&self) -> ScipOverlayIndexSummary {
        self.summary
    }

    /// Consumes the staging result into source-order document batches.
    #[must_use]
    pub fn into_documents(self) -> Vec<ScipOverlayDocument> {
        self.documents
    }
}

/// Categorical all-or-nothing application import failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScipOverlayImportError {
    /// Cancellation was visible before the complete provisional result existed.
    Cancelled,
    /// The shared monotonic deadline elapsed before the complete result existed.
    DeadlineExceeded,
    /// Hostile SCIP input or a claim against the pinned source view was rejected.
    Decode(ScipOverlayDocumentError),
    /// The decoder produced accounting that cannot match retained staging facts.
    InconsistentSummary,
}

impl fmt::Display for ScipOverlayImportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => formatter.write_str("SCIP overlay import cancelled"),
            Self::DeadlineExceeded => formatter.write_str("SCIP overlay import deadline exceeded"),
            Self::Decode(error) => write!(formatter, "SCIP overlay import rejected: {error}"),
            Self::InconsistentSummary => {
                formatter.write_str("SCIP overlay import produced inconsistent accounting")
            }
        }
    }
}

impl Error for ScipOverlayImportError {}

/// Decodes hostile SCIP input solely against an immutable, already-pinned source view.
///
/// No facts become durable here. The local adapter must atomically stage and
/// activate the returned batches only after its final source/view fence holds.
pub fn prepare_scip_overlay_import<SourceLookup>(
    request: ScipOverlayImportRequest<'_, SourceLookup>,
) -> Result<PreparedScipOverlayImport, ScipOverlayImportError>
where
    SourceLookup: ScipImmutableSourceLookup,
{
    check_control(request.cancelled, request.deadline)?;
    let mut documents = Vec::new();
    let summary = decode_scip_overlay_index(
        request.input,
        request.source_manifest,
        request.path_limits,
        request.source_lookup,
        request.cancelled,
        request.deadline,
        |document| {
            documents.push(document);
            Ok(())
        },
    )
    .map_err(ScipOverlayImportError::Decode)?;
    check_control(request.cancelled, request.deadline)?;
    if usize::try_from(summary.documents()).ok() != Some(documents.len()) {
        return Err(ScipOverlayImportError::InconsistentSummary);
    }
    Ok(PreparedScipOverlayImport {
        identity: request.identity,
        summary,
        documents,
    })
}

fn check_control(cancelled: &AtomicBool, deadline: Instant) -> Result<(), ScipOverlayImportError> {
    if cancelled.load(Ordering::Acquire) {
        Err(ScipOverlayImportError::Cancelled)
    } else if Instant::now() >= deadline {
        Err(ScipOverlayImportError::DeadlineExceeded)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        sync::atomic::AtomicBool,
        time::{Duration, Instant},
    };

    use repowitness_analysis::ScipImmutableSourceLookup;
    use repowitness_domain::{
        ConfigurationDigest, ProducerManifestDigest, RepositoryPath, RepositoryPathLimits,
        ScipImporterDigest, ScipInputDigest, ScipSchemaDigest, SourceContentDigest, SourceFileKind,
        SourceFileLimit, SourceManifest, SourceManifestDigest, SourceManifestEntry,
        SourceSnapshotDigest,
    };
    use sha2::Digest;

    use super::{
        ScipOverlayIdentityInput, ScipOverlayImportError, ScipOverlayImportRequest,
        prepare_scip_overlay_import,
    };

    struct Sources(BTreeMap<RepositoryPath, Box<[u8]>>);

    impl ScipImmutableSourceLookup for Sources {
        fn source_bytes(&self, path: &RepositoryPath) -> Option<&[u8]> {
            self.0.get(path).map(Box::as_ref)
        }
    }

    #[test]
    fn malformed_input_has_no_partially_returned_documents() {
        let limits = RepositoryPathLimits::new(1_048_576, 1_048_576);
        let path = RepositoryPath::try_from_bytes(b"src/main.rs", limits).expect("path");
        let source = b"fn main() {}\n".to_vec().into_boxed_slice();
        let content = SourceContentDigest::new(sha2::Sha256::digest(&source).into());
        let manifest = SourceManifest::try_from_vec(
            vec![SourceManifestEntry::new(
                path.clone(),
                SourceFileKind::Regular,
                content,
            )],
            SourceFileLimit::new(1),
        )
        .expect("manifest");
        let sources = Sources(BTreeMap::from([(path, source)]));
        let cancelled = AtomicBool::new(false);
        let result = prepare_scip_overlay_import(ScipOverlayImportRequest::new(
            b"not a SCIP index",
            &manifest,
            limits,
            &sources,
            ScipOverlayIdentityInput::new(
                crate::ScipOverlayScopeIdentity::new(
                    repowitness_domain::ConnectedWorkspaceId::new([8; 32]),
                    9,
                    repowitness_domain::SourceSlotId::new([10; 32]),
                    crate::SourceSlotEpoch::INITIAL,
                    11,
                )
                .expect("scope"),
                SourceSnapshotDigest::new([1; 32]),
                SourceManifestDigest::new([2; 32]),
                ConfigurationDigest::new([3; 32]),
                ProducerManifestDigest::new([4; 32]),
                ScipSchemaDigest::new([5; 32]),
                ScipImporterDigest::new([6; 32]),
                ScipInputDigest::new([7; 32]),
            ),
            &cancelled,
            Instant::now() + Duration::from_secs(1),
        ));
        assert!(matches!(result, Err(ScipOverlayImportError::Decode(_))));
    }
}
