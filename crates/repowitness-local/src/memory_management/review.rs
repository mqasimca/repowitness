use std::sync::{Arc, atomic::AtomicBool};

use repowitness_application::{
    RepositoryIdentityTextV1, RepositoryPathTextByteLimit, RepositoryPathTextV1,
};
use repowitness_domain::{
    AnalysisArtifactDigest, CanonicalMemoryDigest, MAX_MEMORY_EVIDENCE,
    MAX_MEMORY_INTEROPERABLE_INTEGER, MemoryAuditActorId, MemoryFactOrdinal,
    MemoryRecordedAtUnixMillis, RepositoryPathLimits,
};

use super::{
    LocalMemoryCorrespondenceReviewRequest, LocalMemoryManageError, check_control,
    checked_deadline, map_repository_identity_error, map_store_error, open_store, open_worktree,
    record_id,
};
use crate::sqlite::memory_review::PreparedMemoryCorrespondenceReview;

const REVIEW_PATH_TEXT_LIMIT: RepositoryPathTextByteLimit =
    RepositoryPathTextByteLimit::new(65_535);
const REVIEW_PATH_LIMITS: RepositoryPathLimits = RepositoryPathLimits::new(32_764, 16_382);

/// Durable outcome from one exact correspondence-review append.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalMemoryCorrespondenceReviewReceipt {
    inserted: bool,
}

impl LocalMemoryCorrespondenceReviewReceipt {
    /// Reports whether this call appended a new semantic review event.
    #[must_use]
    pub const fn inserted(self) -> bool {
        self.inserted
    }
}

pub(super) fn review(
    request: LocalMemoryCorrespondenceReviewRequest<'_>,
    cancelled: Arc<AtomicBool>,
) -> Result<LocalMemoryCorrespondenceReviewReceipt, LocalMemoryManageError> {
    let deadline = checked_deadline(request.deadline)?;
    check_control(cancelled.as_ref(), deadline)?;
    let repository = RepositoryIdentityTextV1::decode(request.repository_identity)
        .map_err(map_repository_identity_error)?;
    let record_id = record_id(request.record_id)?;
    let revision = CanonicalMemoryDigest::new(
        decode_sha256(request.revision_sha256)
            .map_err(|()| LocalMemoryManageError::RevisionInvalid)?,
    );
    if usize::from(request.evidence_ordinal) >= MAX_MEMORY_EVIDENCE {
        return Err(LocalMemoryManageError::ReviewTargetUnavailable);
    }
    let target_path = RepositoryPathTextV1::decode(
        request.target_path,
        REVIEW_PATH_TEXT_LIMIT,
        REVIEW_PATH_LIMITS,
    )
    .map_err(|_| LocalMemoryManageError::ReviewTargetUnavailable)?;
    let target_artifact = AnalysisArtifactDigest::new(
        decode_sha256(request.target_artifact_sha256)
            .map_err(|()| LocalMemoryManageError::ReviewTargetUnavailable)?,
    );
    if request.target_fact_ordinal > MAX_MEMORY_INTEROPERABLE_INTEGER {
        return Err(LocalMemoryManageError::ReviewTargetUnavailable);
    }
    let target_fact_ordinal = MemoryFactOrdinal::try_new(request.target_fact_ordinal)
        .map_err(|_| LocalMemoryManageError::ReviewTargetUnavailable)?;
    let actor = MemoryAuditActorId::try_new(request.actor.to_owned())
        .map_err(|_| LocalMemoryManageError::ActorInvalid)?;
    let recorded_at = MemoryRecordedAtUnixMillis::try_new(request.recorded_at_unix_ms)
        .map_err(|_| LocalMemoryManageError::InvalidLimits)?;
    let worktree = open_worktree(request.repository_root)?;
    let store = open_store(
        &worktree,
        request.database,
        request.migration_applied_at_unix_ms,
        Arc::clone(&cancelled),
        deadline,
    )?;
    let operation = store
        .append_memory_correspondence_review(
            PreparedMemoryCorrespondenceReview::new(
                repository,
                record_id,
                revision,
                request.evidence_ordinal,
                request.operation,
                target_path,
                target_artifact,
                target_fact_ordinal.get(),
                actor,
                recorded_at,
            ),
            Arc::clone(&cancelled),
            deadline,
        )
        .map(|receipt| LocalMemoryCorrespondenceReviewReceipt {
            inserted: receipt.inserted(),
        })
        .map_err(map_store_error);
    finish(store, operation, deadline)
}

fn finish(
    store: crate::OwnedSqliteIndex,
    operation: Result<LocalMemoryCorrespondenceReviewReceipt, LocalMemoryManageError>,
    deadline: std::time::Instant,
) -> Result<LocalMemoryCorrespondenceReviewReceipt, LocalMemoryManageError> {
    let checkpoint = if operation.is_ok() {
        store
            .checkpoint(deadline)
            .map(|_| ())
            .map_err(map_store_error)
    } else {
        Ok(())
    };
    let shutdown = store.shutdown(deadline).map_err(map_store_error);
    match (operation, checkpoint, shutdown) {
        (Err(error), _, _) | (Ok(_), Err(error), _) | (Ok(_), Ok(()), Err(error)) => Err(error),
        (Ok(receipt), Ok(()), Ok(())) => Ok(receipt),
    }
}

fn decode_sha256(text: &str) -> Result<[u8; 32], ()> {
    if text.len() != 64 {
        return Err(());
    }
    let mut output = [0_u8; 32];
    for (index, pair) in text.as_bytes().chunks_exact(2).enumerate() {
        let high = lowercase_hex_nibble(pair[0]).ok_or(())?;
        let low = lowercase_hex_nibble(pair[1]).ok_or(())?;
        output[index] = (high << 4) | low;
    }
    Ok(output)
}

const fn lowercase_hex_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        _ => None,
    }
}
