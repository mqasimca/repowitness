#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MemoryProjectionResultLimits {
    max_candidates: u64,
}

impl MemoryProjectionResultLimits {
    const MAX_CANDIDATES: u64 =
        MAX_MEMORY_PROJECTION_VERSIONS as u64 * MAX_RUST_CORRESPONDENCE_CANDIDATES as u64 * 16;
    const DEFAULT_CANDIDATES: u64 = 16_384;

    pub(crate) const fn try_new(max_candidates: u64) -> Result<Self, SqliteStoreError> {
        if max_candidates == 0 || max_candidates > Self::MAX_CANDIDATES {
            return Err(SqliteStoreError::InvalidMemoryProjectionLimits);
        }
        Ok(Self { max_candidates })
    }
}

impl Default for MemoryProjectionResultLimits {
    fn default() -> Self {
        Self {
            max_candidates: Self::DEFAULT_CANDIDATES,
        }
    }
}
