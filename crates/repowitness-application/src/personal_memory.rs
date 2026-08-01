//! Application-owned admission for local-only immutable personal memory.

use std::{
    error::Error,
    fmt,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use repowitness_domain::{
    PersonalMemoryId, PersonalMemoryProfileId, PersonalMemoryRecord, PersonalMemoryRevision,
    RepositoryIdentityDigest,
};

/// Maximum records returned by an explicit personal-memory read.
pub const MAX_PERSONAL_MEMORY_READ_RESULTS: u16 = 100;

/// Adapter-confirmed receipt for one immutable local-only memory revision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PersonalMemoryReceipt {
    record_id: PersonalMemoryId,
    revision: PersonalMemoryRevision,
    inserted: bool,
}

impl PersonalMemoryReceipt {
    /// Creates a receipt after the adapter commits or recognizes the exact revision.
    #[must_use]
    pub const fn new(
        record_id: PersonalMemoryId,
        revision: PersonalMemoryRevision,
        inserted: bool,
    ) -> Self {
        Self {
            record_id,
            revision,
            inserted,
        }
    }

    /// Returns the opaque local record identity.
    #[must_use]
    pub const fn record_id(self) -> PersonalMemoryId {
        self.record_id
    }

    /// Returns the immutable revision identity.
    #[must_use]
    pub const fn revision(self) -> PersonalMemoryRevision {
        self.revision
    }

    /// Reports whether this invocation appended a new revision.
    #[must_use]
    pub const fn inserted(self) -> bool {
        self.inserted
    }
}

/// Narrow append-only port for personal memory. It has no team-memory method.
pub trait PersonalMemoryPort {
    /// Stable local-adapter error.
    type Error;

    /// Appends or recognizes one exact local-only immutable revision.
    fn append_personal_memory(
        &self,
        record: PersonalMemoryRecord,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<PersonalMemoryReceipt, Self::Error>;
}

/// Explicit, profile-scoped read port for local personal memory.
///
/// It is deliberately separate from team recall and requires both scopes on
/// every call, so personal records cannot enter a default response.
pub trait PersonalMemoryReadPort {
    /// Stable local-adapter error.
    type Error;

    /// Returns at most `limit` immutable revisions for this exact profile and repository.
    fn read_personal_memory(
        &self,
        profile: PersonalMemoryProfileId,
        repository: RepositoryIdentityDigest,
        limit: u16,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<Vec<PersonalMemoryRecord>, Self::Error>;
}

/// Stable application failure for personal-memory admission.
#[derive(Clone, Eq, PartialEq)]
pub enum PersonalMemoryError<PortError> {
    /// Cancellation was visible before or after persistence.
    Cancelled,
    /// The absolute deadline elapsed before or after persistence.
    DeadlineExceeded,
    /// An adapter receipt did not identify the submitted immutable revision.
    InvalidPortReceipt,
    /// The local adapter failed without exposing personal content.
    Port(PortError),
}

impl<PortError> fmt::Debug for PersonalMemoryError<PortError> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Cancelled => "PersonalMemoryError::Cancelled",
            Self::DeadlineExceeded => "PersonalMemoryError::DeadlineExceeded",
            Self::InvalidPortReceipt => "PersonalMemoryError::InvalidPortReceipt",
            Self::Port(_) => "PersonalMemoryError::Port",
        })
    }
}

impl<PortError> fmt::Display for PersonalMemoryError<PortError> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Cancelled => "personal memory operation was cancelled",
            Self::DeadlineExceeded => "personal memory deadline elapsed",
            Self::InvalidPortReceipt => "personal memory persistence returned an invalid receipt",
            Self::Port(_) => "personal memory persistence failed",
        })
    }
}

impl<PortError> Error for PersonalMemoryError<PortError> {}

/// Appends one local-only personal-memory revision through a bounded port.
pub fn append_personal_memory<Port>(
    port: &Port,
    record: PersonalMemoryRecord,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
) -> Result<PersonalMemoryReceipt, PersonalMemoryError<Port::Error>>
where
    Port: PersonalMemoryPort,
{
    check_control(&cancelled, deadline)?;
    let expected_record = record.record_id();
    let expected_revision = record.revision();
    let receipt = port
        .append_personal_memory(record, Arc::clone(&cancelled), deadline)
        .map_err(PersonalMemoryError::Port)?;
    check_control(&cancelled, deadline)?;
    if receipt.record_id != expected_record || receipt.revision != expected_revision {
        return Err(PersonalMemoryError::InvalidPortReceipt);
    }
    Ok(receipt)
}

/// Reads local-only records through an explicit profile-and-repository boundary.
pub fn read_personal_memory<Port>(
    port: &Port,
    profile: PersonalMemoryProfileId,
    repository: RepositoryIdentityDigest,
    limit: u16,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
) -> Result<Vec<PersonalMemoryRecord>, PersonalMemoryError<Port::Error>>
where
    Port: PersonalMemoryReadPort,
{
    if limit == 0 || limit > MAX_PERSONAL_MEMORY_READ_RESULTS {
        return Err(PersonalMemoryError::InvalidPortReceipt);
    }
    check_control(&cancelled, deadline)?;
    let records = port
        .read_personal_memory(profile, repository, limit, Arc::clone(&cancelled), deadline)
        .map_err(PersonalMemoryError::Port)?;
    check_control(&cancelled, deadline)?;
    if records.len() > usize::from(limit)
        || records
            .iter()
            .any(|record| record.profile() != profile || record.repository() != repository)
    {
        return Err(PersonalMemoryError::InvalidPortReceipt);
    }
    Ok(records)
}

fn check_control<PortError>(
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), PersonalMemoryError<PortError>> {
    if cancelled.load(Ordering::Acquire) {
        return Err(PersonalMemoryError::Cancelled);
    }
    if Instant::now() >= deadline {
        return Err(PersonalMemoryError::DeadlineExceeded);
    }
    Ok(())
}
