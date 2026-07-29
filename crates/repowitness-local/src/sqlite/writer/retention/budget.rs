#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RetentionWorkBudget {
    maximum: u64,
    consumed: u64,
    reserved: u64,
}

impl RetentionWorkBudget {
    pub(crate) const fn new(maximum: u64) -> Self {
        Self {
            maximum,
            consumed: 0,
            reserved: 0,
        }
    }

    pub(crate) fn consume(&mut self, rows: u64) -> Result<(), SqliteStoreError> {
        self.consumed = self
            .consumed
            .checked_add(rows)
            .ok_or(SqliteStoreError::RetentionLimitExceeded)?;
        self.ensure_within_limit()
    }

    pub(crate) fn reserve(&mut self, rows: u64) -> Result<(), SqliteStoreError> {
        self.reserved = self
            .reserved
            .checked_add(rows)
            .ok_or(SqliteStoreError::RetentionLimitExceeded)?;
        self.ensure_within_limit()
    }

    pub(crate) fn can_reserve(&self, rows: u64) -> bool {
        self.total()
            .and_then(|total| total.checked_add(rows))
            .is_some_and(|total| total <= self.maximum)
    }

    pub(crate) fn release_reservations(&mut self) {
        self.reserved = 0;
    }

    pub(crate) fn available(&self) -> u64 {
        self.total()
            .map_or(0, |total| self.maximum.saturating_sub(total))
    }

    pub(crate) fn logical_rows(&self) -> Result<u64, SqliteStoreError> {
        self.total().ok_or(SqliteStoreError::RetentionLimitExceeded)
    }

    fn total(&self) -> Option<u64> {
        self.consumed.checked_add(self.reserved)
    }

    fn ensure_within_limit(&self) -> Result<(), SqliteStoreError> {
        if self.logical_rows()? > self.maximum {
            Err(SqliteStoreError::RetentionLimitExceeded)
        } else {
            Ok(())
        }
    }
}
