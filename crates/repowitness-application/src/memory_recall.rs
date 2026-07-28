//! Shared bounded retrieval of one immutable current-memory projection.

mod evidence;
mod port;
mod query;
mod record;

pub use evidence::{
    MemoryRecallCandidate, MemoryRecallCandidateRelation, MemoryRecallEvidence,
    MemoryRecallEvidenceAssurance, MemoryRecallEvidenceOutcome, MemoryRecallEvidenceState,
    MemoryRecallOccurrence, MemoryRecallProducer, MemoryRecallReason,
};
pub use port::{
    MemoryRecallError, MemoryRecallPort, MemoryRecallPortOutputError, MemoryRecallPortResult,
    MemoryRecallRequest, MemoryRecallResult, MemoryRecallUseCaseResult, memory_recall,
};
pub use query::{
    DEFAULT_MEMORY_RECALL_OUTPUT_BYTES, DEFAULT_MEMORY_RECALL_RESULTS,
    DEFAULT_MEMORY_RECALL_SCAN_BYTES, MAX_MEMORY_RECALL_OUTPUT_BYTES, MAX_MEMORY_RECALL_RESULTS,
    MAX_MEMORY_RECALL_SCAN_BYTES, MEMORY_RECALL_PROFILE_VERSION, MemoryRecallLimitError,
    MemoryRecallLimits, MemoryRecallQuery, MemoryRecallQueryDigest, MemoryRecallQueryError,
};
pub use record::{MemoryRecallProjectionCoverage, MemoryRecallRecord};

#[cfg(test)]
mod tests;
