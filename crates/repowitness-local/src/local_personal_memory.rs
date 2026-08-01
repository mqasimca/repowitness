//! Local composition for explicit, profile-scoped personal-memory operations.

use std::{
    error::Error,
    fmt,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use repowitness_application::{
    PersonalMemoryError, RepositoryIdentityTextError, RepositoryIdentityTextV1,
    append_personal_memory, read_personal_memory,
};
use repowitness_domain::{
    MemoryLifecycle, PersonalMemoryId, PersonalMemoryKind, PersonalMemoryProfileId,
    PersonalMemoryRecord, PersonalMemoryRevision, TaskError, TaskText,
};
use sha2::{Digest, Sha256};

use crate::{OwnedSqliteIndex, OwnedSqliteReader, SqliteStoreError};

/// Default end-to-end deadline for a local personal-memory append.
pub const DEFAULT_LOCAL_PERSONAL_MEMORY_WRITE_DEADLINE: Duration = Duration::from_secs(60);
/// Default end-to-end deadline for a local personal-memory read.
pub const DEFAULT_LOCAL_PERSONAL_MEMORY_READ_DEADLINE: Duration = Duration::from_secs(5);

/// Explicit input for one local-only, immutable personal-memory append.
#[derive(Clone, Copy)]
pub struct LocalPersonalMemoryAppendRequest<'a> {
    database: &'a Path,
    repository_identity: &'a str,
    profile: PersonalMemoryProfileId,
    kind: PersonalMemoryKind,
    title: &'a str,
    body: &'a str,
    lifecycle: MemoryLifecycle,
    recorded_at_unix_ms: u64,
    deadline: Duration,
}

impl<'a> LocalPersonalMemoryAppendRequest<'a> {
    /// Creates one exact local-profile append request.
    #[must_use]
    #[allow(
        clippy::too_many_arguments,
        reason = "each local-only scope, immutable content field, and timestamp is explicit"
    )]
    pub const fn new(
        database: &'a Path,
        repository_identity: &'a str,
        profile: PersonalMemoryProfileId,
        kind: PersonalMemoryKind,
        title: &'a str,
        body: &'a str,
        lifecycle: MemoryLifecycle,
        recorded_at_unix_ms: u64,
    ) -> Self {
        Self {
            database,
            repository_identity,
            profile,
            kind,
            title,
            body,
            lifecycle,
            recorded_at_unix_ms,
            deadline: DEFAULT_LOCAL_PERSONAL_MEMORY_WRITE_DEADLINE,
        }
    }

    /// Replaces the monotonic end-to-end deadline.
    #[must_use]
    pub const fn with_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = deadline;
        self
    }
}

impl fmt::Debug for LocalPersonalMemoryAppendRequest<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalPersonalMemoryAppendRequest")
            .field("database", &"<redacted-path>")
            .field("repository_identity", &"<redacted-identity>")
            .field("profile", &self.profile)
            .field("kind", &self.kind)
            .field("title", &"<redacted>")
            .field("body", &"<redacted>")
            .field("lifecycle", &self.lifecycle)
            .field("recorded_at_unix_ms", &self.recorded_at_unix_ms)
            .field("deadline", &self.deadline)
            .finish_non_exhaustive()
    }
}

/// Explicit input for one exact local-profile and repository-scoped read.
#[derive(Clone, Copy)]
pub struct LocalPersonalMemoryReadRequest<'a> {
    database: &'a Path,
    repository_identity: &'a str,
    profile: PersonalMemoryProfileId,
    limit: u16,
    deadline: Duration,
}

impl<'a> LocalPersonalMemoryReadRequest<'a> {
    /// Creates a bounded read request for one exact personal-memory scope.
    #[must_use]
    pub const fn new(
        database: &'a Path,
        repository_identity: &'a str,
        profile: PersonalMemoryProfileId,
        limit: u16,
    ) -> Self {
        Self {
            database,
            repository_identity,
            profile,
            limit,
            deadline: DEFAULT_LOCAL_PERSONAL_MEMORY_READ_DEADLINE,
        }
    }

    /// Replaces the monotonic end-to-end deadline.
    #[must_use]
    pub const fn with_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = deadline;
        self
    }
}

impl fmt::Debug for LocalPersonalMemoryReadRequest<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalPersonalMemoryReadRequest")
            .field("database", &"<redacted-path>")
            .field("repository_identity", &"<redacted-identity>")
            .field("profile", &self.profile)
            .field("limit", &self.limit)
            .field("deadline", &self.deadline)
            .finish_non_exhaustive()
    }
}

/// Stable content- and path-redacted local personal-memory failure.
#[derive(Debug)]
pub enum LocalPersonalMemoryError {
    /// The repository identity was malformed or non-canonical.
    RepositoryIdentity {
        /// Stable identity-validation failure.
        source: RepositoryIdentityTextError,
    },
    /// One supplied text field violated the bounded domain contract.
    Text {
        /// Stable text-validation failure.
        source: TaskError,
    },
    /// Local admission rejected high-confidence secret material.
    SensitiveContent,
    /// Operating-system entropy was unavailable for a new opaque record identity.
    EntropyUnavailable,
    /// The requested bounded read limit was outside the supported range.
    InvalidReadLimit,
    /// The absolute deadline was not representable.
    DeadlineNotRepresentable,
    /// Cancellation was visible before persistence or reading.
    Cancelled,
    /// The deadline elapsed before persistence or reading.
    DeadlineExceeded,
    /// The owned local SQLite store could not start.
    StoreStart {
        /// Stable SQLite boundary failure.
        source: SqliteStoreError,
    },
    /// The bounded append use case failed.
    Append {
        /// Stable application or SQLite boundary failure.
        source: PersonalMemoryError<SqliteStoreError>,
    },
    /// The bounded read use case failed.
    Read {
        /// Stable application or SQLite boundary failure.
        source: PersonalMemoryError<SqliteStoreError>,
    },
    /// The owned local SQLite store could not shut down cleanly.
    Shutdown {
        /// Stable SQLite boundary failure.
        source: SqliteStoreError,
    },
}

impl fmt::Display for LocalPersonalMemoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::RepositoryIdentity { .. } => "personal memory repository identity is invalid",
            Self::Text { .. } => "personal memory text is invalid",
            Self::SensitiveContent => "personal memory contains sensitive content",
            Self::EntropyUnavailable => "personal memory identity entropy is unavailable",
            Self::InvalidReadLimit => "personal memory read limit is invalid",
            Self::DeadlineNotRepresentable => "personal memory deadline is not representable",
            Self::Cancelled => "personal memory operation was cancelled",
            Self::DeadlineExceeded => "personal memory deadline elapsed",
            Self::StoreStart { .. } => "personal memory store startup failed",
            Self::Append { .. } => "personal memory append failed",
            Self::Read { .. } => "personal memory read failed",
            Self::Shutdown { .. } => "personal memory store shutdown failed",
        })
    }
}

impl Error for LocalPersonalMemoryError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::RepositoryIdentity { source } => Some(source),
            Self::Text { source } => Some(source),
            Self::StoreStart { source } | Self::Shutdown { source } => Some(source),
            Self::Append { source } | Self::Read { source } => Some(source),
            Self::SensitiveContent
            | Self::EntropyUnavailable
            | Self::InvalidReadLimit
            | Self::DeadlineNotRepresentable
            | Self::Cancelled
            | Self::DeadlineExceeded => None,
        }
    }
}

/// Appends one local-only record using a fresh opaque record identity.
///
/// The revision identity is a deterministic domain-separated SHA-256 digest of
/// the submitted immutable fields. It is never an OS-random value.
pub fn append_local_personal_memory(
    request: LocalPersonalMemoryAppendRequest<'_>,
    cancelled: Arc<AtomicBool>,
) -> Result<PersonalMemoryRecord, LocalPersonalMemoryError> {
    let repository = RepositoryIdentityTextV1::decode(request.repository_identity)
        .map_err(|source| LocalPersonalMemoryError::RepositoryIdentity { source })?;
    let title = TaskText::try_new(request.title.to_owned())
        .map_err(|source| LocalPersonalMemoryError::Text { source })?;
    let body = TaskText::try_new(request.body.to_owned())
        .map_err(|source| LocalPersonalMemoryError::Text { source })?;
    if crate::memory_management::secret::contains_sensitive_text(title.as_str())
        || crate::memory_management::secret::contains_sensitive_text(body.as_str())
    {
        return Err(LocalPersonalMemoryError::SensitiveContent);
    }
    let deadline = Instant::now()
        .checked_add(request.deadline)
        .ok_or(LocalPersonalMemoryError::DeadlineNotRepresentable)?;
    check_control(cancelled.as_ref(), deadline)?;
    let record_id = generate_record_id()?;
    let revision = personal_memory_revision(request, repository, record_id, &title, &body);
    let record = PersonalMemoryRecord::new(
        request.profile,
        repository,
        record_id,
        revision,
        request.kind,
        title,
        body,
        request.lifecycle,
        request.recorded_at_unix_ms,
    );
    let (store, _) =
        OwnedSqliteIndex::start(request.database, request.recorded_at_unix_ms, deadline)
            .map_err(|source| LocalPersonalMemoryError::StoreStart { source })?;
    let result = append_personal_memory(&store, record.clone(), Arc::clone(&cancelled), deadline)
        .map_err(map_append_error);
    let shutdown = store.shutdown(deadline);
    match (result, shutdown) {
        (Ok(_), Ok(())) => Ok(record),
        (Err(error), _) => Err(error),
        (Ok(_), Err(source)) => Err(LocalPersonalMemoryError::Shutdown { source }),
    }
}

/// Reads only exact-profile, exact-repository personal memory without creating a database.
pub fn read_local_personal_memory(
    request: LocalPersonalMemoryReadRequest<'_>,
    cancelled: Arc<AtomicBool>,
) -> Result<Vec<PersonalMemoryRecord>, LocalPersonalMemoryError> {
    let repository = RepositoryIdentityTextV1::decode(request.repository_identity)
        .map_err(|source| LocalPersonalMemoryError::RepositoryIdentity { source })?;
    if request.limit == 0 || request.limit > 100 {
        return Err(LocalPersonalMemoryError::InvalidReadLimit);
    }
    let deadline = Instant::now()
        .checked_add(request.deadline)
        .ok_or(LocalPersonalMemoryError::DeadlineNotRepresentable)?;
    check_control(cancelled.as_ref(), deadline)?;
    if !request.database.is_file() {
        return Ok(Vec::new());
    }
    let store = OwnedSqliteReader::start(request.database, deadline)
        .map_err(|source| LocalPersonalMemoryError::StoreStart { source })?;
    let result = read_personal_memory(
        &store,
        request.profile,
        repository,
        request.limit,
        Arc::clone(&cancelled),
        deadline,
    )
    .map_err(map_read_error);
    let shutdown = store.shutdown(deadline);
    match (result, shutdown) {
        (Ok(records), Ok(())) => Ok(records),
        (Err(error), _) => Err(error),
        (Ok(_), Err(source)) => Err(LocalPersonalMemoryError::Shutdown { source }),
    }
}

fn generate_record_id() -> Result<PersonalMemoryId, LocalPersonalMemoryError> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| LocalPersonalMemoryError::EntropyUnavailable)?;
    Ok(PersonalMemoryId::new(bytes))
}

fn personal_memory_revision(
    request: LocalPersonalMemoryAppendRequest<'_>,
    repository: repowitness_domain::RepositoryIdentityDigest,
    record_id: PersonalMemoryId,
    title: &TaskText,
    body: &TaskText,
) -> PersonalMemoryRevision {
    let mut hasher = Sha256::new();
    hasher.update(b"repowitness-personal-memory-revision-v1\0");
    update_bytes(&mut hasher, &request.profile.as_bytes());
    update_bytes(&mut hasher, repository.as_bytes());
    update_bytes(&mut hasher, &record_id.as_bytes());
    hasher.update([personal_memory_kind_byte(request.kind)]);
    update_bytes(&mut hasher, title.as_str().as_bytes());
    update_bytes(&mut hasher, body.as_str().as_bytes());
    hasher.update([memory_lifecycle_byte(request.lifecycle)]);
    hasher.update(request.recorded_at_unix_ms.to_be_bytes());
    PersonalMemoryRevision::new(hasher.finalize().into())
}

fn update_bytes(hasher: &mut Sha256, value: &[u8]) {
    hasher.update(u64::try_from(value.len()).unwrap_or(u64::MAX).to_be_bytes());
    hasher.update(value);
}

const fn personal_memory_kind_byte(kind: PersonalMemoryKind) -> u8 {
    match kind {
        PersonalMemoryKind::Fact => 1,
        PersonalMemoryKind::Decision => 2,
        PersonalMemoryKind::Procedure => 3,
        PersonalMemoryKind::Episode => 4,
        PersonalMemoryKind::Preference => 5,
        PersonalMemoryKind::Policy => 6,
        PersonalMemoryKind::Failure => 7,
    }
}

const fn memory_lifecycle_byte(lifecycle: MemoryLifecycle) -> u8 {
    match lifecycle {
        MemoryLifecycle::Active => 1,
        MemoryLifecycle::NeedsReview => 2,
        MemoryLifecycle::Stale => 3,
        MemoryLifecycle::Contradicted => 4,
        MemoryLifecycle::Superseded => 5,
        MemoryLifecycle::Quarantined => 6,
        MemoryLifecycle::Tombstoned => 7,
    }
}

fn map_append_error(source: PersonalMemoryError<SqliteStoreError>) -> LocalPersonalMemoryError {
    match source {
        PersonalMemoryError::Cancelled => LocalPersonalMemoryError::Cancelled,
        PersonalMemoryError::DeadlineExceeded => LocalPersonalMemoryError::DeadlineExceeded,
        source => LocalPersonalMemoryError::Append { source },
    }
}

fn map_read_error(source: PersonalMemoryError<SqliteStoreError>) -> LocalPersonalMemoryError {
    match source {
        PersonalMemoryError::Cancelled => LocalPersonalMemoryError::Cancelled,
        PersonalMemoryError::DeadlineExceeded => LocalPersonalMemoryError::DeadlineExceeded,
        source => LocalPersonalMemoryError::Read { source },
    }
}

fn check_control(
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), LocalPersonalMemoryError> {
    if cancelled.load(Ordering::Acquire) {
        Err(LocalPersonalMemoryError::Cancelled)
    } else if Instant::now() >= deadline {
        Err(LocalPersonalMemoryError::DeadlineExceeded)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{path::Path, sync::atomic::AtomicBool};

    use super::*;

    #[test]
    fn append_and_read_are_explicitly_profile_and_repository_scoped() {
        let database = std::env::temp_dir().join(format!(
            "repowitness-personal-memory-{}-{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock is after epoch")
                .as_nanos(),
        ));
        let repository = format!("rwi1:h:{}", "12".repeat(32));
        let profile = PersonalMemoryProfileId::new([0x13; 16]);
        let record = append_local_personal_memory(
            LocalPersonalMemoryAppendRequest::new(
                &database,
                &repository,
                profile,
                PersonalMemoryKind::Preference,
                "prefer local evidence",
                "do not add this to shared memory",
                MemoryLifecycle::Active,
                1,
            ),
            Arc::new(AtomicBool::new(false)),
        )
        .expect("append succeeds");
        let exact = read_local_personal_memory(
            LocalPersonalMemoryReadRequest::new(&database, &repository, profile, 10),
            Arc::new(AtomicBool::new(false)),
        )
        .expect("exact scope reads record");
        assert_eq!(exact, vec![record]);
        assert!(
            read_local_personal_memory(
                LocalPersonalMemoryReadRequest::new(
                    &database,
                    &repository,
                    PersonalMemoryProfileId::new([0x14; 16]),
                    10,
                ),
                Arc::new(AtomicBool::new(false)),
            )
            .expect("other profile is isolated")
            .is_empty()
        );
        let other_repository = format!("rwi1:h:{}", "15".repeat(32));
        assert!(
            read_local_personal_memory(
                LocalPersonalMemoryReadRequest::new(&database, &other_repository, profile, 10),
                Arc::new(AtomicBool::new(false)),
            )
            .expect("other repository is isolated")
            .is_empty()
        );
        let _ = std::fs::remove_file(&database);
        let _ = std::fs::remove_file(database.with_extension("db-shm"));
        let _ = std::fs::remove_file(database.with_extension("db-wal"));
    }

    #[test]
    fn secret_and_invalid_inputs_fail_before_creating_a_store() {
        let database = Path::new("must-not-open-personal-memory.db");
        assert!(!database.exists(), "fixture must be absent");
        let repository = format!("rwi1:h:{}", "16".repeat(32));
        let profile = PersonalMemoryProfileId::new([0x17; 16]);
        assert!(matches!(
            append_local_personal_memory(
                LocalPersonalMemoryAppendRequest::new(
                    database,
                    &repository,
                    profile,
                    PersonalMemoryKind::Fact,
                    "api_key = private-value",
                    "sensitive",
                    MemoryLifecycle::Active,
                    1,
                ),
                Arc::new(AtomicBool::new(false)),
            ),
            Err(LocalPersonalMemoryError::SensitiveContent)
        ));
        assert!(
            !database.exists(),
            "secret input must not initialize storage"
        );
    }
}
