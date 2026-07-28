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
    CanonicalMemoryDigest, MemoryAuditActorId, MemoryObservationSource, MemoryPresentationDigest,
    MemoryRecord, MemoryRecordedAtUnixMillis, RepositoryIdentityDigest,
};

/// Trusted approval policy for one admitted memory observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryImportApproval {
    /// Preserve the exact observation without making the authored claim active.
    ObservedOnly,
    /// Append a separate locally asserted approval for the exact version.
    LocallyApproved,
}

/// Complete validated input to one trusted local memory import.
pub struct ImportMemoryRecordRequest {
    repository: RepositoryIdentityDigest,
    record: MemoryRecord,
    presentation: MemoryPresentationDigest,
    source: MemoryObservationSource,
    audit_actor: MemoryAuditActorId,
    recorded_at: MemoryRecordedAtUnixMillis,
    approval: MemoryImportApproval,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
}

impl ImportMemoryRecordRequest {
    /// Constructs one import request from validated semantic and audit values.
    #[must_use]
    #[allow(
        clippy::too_many_arguments,
        reason = "each import identity and control is an independent trust input"
    )]
    pub const fn new(
        repository: RepositoryIdentityDigest,
        record: MemoryRecord,
        presentation: MemoryPresentationDigest,
        source: MemoryObservationSource,
        audit_actor: MemoryAuditActorId,
        recorded_at: MemoryRecordedAtUnixMillis,
        approval: MemoryImportApproval,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Self {
        Self {
            repository,
            record,
            presentation,
            source,
            audit_actor,
            recorded_at,
            approval,
            cancelled,
            deadline,
        }
    }
}

impl fmt::Debug for ImportMemoryRecordRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ImportMemoryRecordRequest")
            .field("repository", &self.repository)
            .field("record", &self.record)
            .field("presentation", &self.presentation)
            .field("source", &self.source)
            .field("audit_actor", &self.audit_actor)
            .field("recorded_at", &self.recorded_at)
            .field("approval", &self.approval)
            .field("cancelled", &self.cancelled.load(Ordering::Acquire))
            .field("deadline", &"<monotonic>")
            .finish()
    }
}

/// Idempotent insertion outcomes for one exact imported memory version.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MemoryImportReceipt {
    revision: CanonicalMemoryDigest,
    version_inserted: bool,
    observation_inserted: bool,
    approval_inserted: bool,
}

impl MemoryImportReceipt {
    /// Constructs a receipt from adapter-verified durable outcomes.
    #[must_use]
    pub const fn new(
        revision: CanonicalMemoryDigest,
        version_inserted: bool,
        observation_inserted: bool,
        approval_inserted: bool,
    ) -> Self {
        Self {
            revision,
            version_inserted,
            observation_inserted,
            approval_inserted,
        }
    }

    /// Returns the canonical semantic revision identity.
    #[must_use]
    pub const fn revision(self) -> CanonicalMemoryDigest {
        self.revision
    }

    /// Reports whether this call appended the immutable semantic version.
    #[must_use]
    pub const fn version_inserted(self) -> bool {
        self.version_inserted
    }

    /// Reports whether this call appended the exact observation receipt.
    #[must_use]
    pub const fn observation_inserted(self) -> bool {
        self.observation_inserted
    }

    /// Reports whether this call appended the trusted local approval.
    #[must_use]
    pub const fn approval_inserted(self) -> bool {
        self.approval_inserted
    }
}

/// Narrow append-only persistence boundary for trusted memory import.
pub trait MemoryVersionImportPort {
    /// Stable adapter failure mapped at its own boundary.
    type Error;

    /// Appends or verifies one exact version and its trusted audit receipts.
    #[allow(
        clippy::too_many_arguments,
        reason = "each import identity and control remains explicit at the I/O boundary"
    )]
    fn import_memory_version(
        &self,
        repository: RepositoryIdentityDigest,
        record: MemoryRecord,
        presentation: MemoryPresentationDigest,
        source: MemoryObservationSource,
        audit_actor: MemoryAuditActorId,
        recorded_at: MemoryRecordedAtUnixMillis,
        approval: MemoryImportApproval,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<MemoryImportReceipt, Self::Error>;
}

/// Stable application failure from trusted memory import.
#[derive(Clone, Eq, PartialEq)]
pub enum ImportMemoryRecordError<PortError> {
    /// Cancellation was visible before persistence began.
    Cancelled,
    /// The absolute request deadline elapsed before persistence began.
    DeadlineExceeded,
    /// The request repository did not equal the validated record scope.
    ScopeMismatch,
    /// The persistence adapter failed.
    Port(PortError),
}

impl<PortError> fmt::Debug for ImportMemoryRecordError<PortError> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Cancelled => "ImportMemoryRecordError::Cancelled",
            Self::DeadlineExceeded => "ImportMemoryRecordError::DeadlineExceeded",
            Self::ScopeMismatch => "ImportMemoryRecordError::ScopeMismatch",
            Self::Port(_) => "ImportMemoryRecordError::Port",
        })
    }
}

impl<PortError> fmt::Display for ImportMemoryRecordError<PortError> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Cancelled => "memory import was cancelled",
            Self::DeadlineExceeded => "memory import deadline elapsed",
            Self::ScopeMismatch => "memory record repository scope does not match the request",
            Self::Port(_) => "memory persistence failed",
        })
    }
}

impl<PortError> Error for ImportMemoryRecordError<PortError> {}

/// Scope-checks and delegates one bounded, idempotent trusted memory import.
pub fn import_memory_record<Port>(
    port: &Port,
    request: ImportMemoryRecordRequest,
) -> Result<MemoryImportReceipt, ImportMemoryRecordError<Port::Error>>
where
    Port: MemoryVersionImportPort,
{
    check_control(&request.cancelled, request.deadline)?;
    if request.record.scope().repository() != request.repository {
        return Err(ImportMemoryRecordError::ScopeMismatch);
    }
    port.import_memory_version(
        request.repository,
        request.record,
        request.presentation,
        request.source,
        request.audit_actor,
        request.recorded_at,
        request.approval,
        request.cancelled,
        request.deadline,
    )
    .map_err(ImportMemoryRecordError::Port)
}

fn check_control<PortError>(
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), ImportMemoryRecordError<PortError>> {
    if cancelled.load(Ordering::Acquire) {
        return Err(ImportMemoryRecordError::Cancelled);
    }
    if Instant::now() >= deadline {
        return Err(ImportMemoryRecordError::DeadlineExceeded);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        cell::{Cell, RefCell},
        sync::{Arc, atomic::AtomicBool},
        time::{Duration, Instant},
    };

    use repowitness_domain::{
        AnalysisArtifactDigest, ByteOffset, ByteSpan, CanonicalMemoryDigest, DeclarationDigest,
        MemoryActorId, MemoryActorKind, MemoryAssurance, MemoryAuditActorId, MemoryBody,
        MemoryClaim, MemoryDisplayRevision, MemoryEvidence, MemoryEvidenceIndex, MemoryFactOrdinal,
        MemoryKind, MemoryLifecycle, MemoryObservationSource, MemoryPresentationDigest,
        MemoryProducerId, MemoryProducerVersion, MemoryProvenance, MemoryProvenanceOrigin,
        MemoryQualifiedName, MemoryRecord, MemoryRecordHeader, MemoryRecordId,
        MemoryRecordedAtUnixMillis, MemoryScope, MemorySymbolName, MemoryTitle, MemoryValidity,
        ProducerIdentity, RepositoryIdentityDigest, RepositoryPath, RepositoryPathLimits,
        RustMemorySymbolKind, RustSymbolMemoryEvidence, SourceContentDigest, SourceSnapshotDigest,
    };

    use super::{
        ImportMemoryRecordError, ImportMemoryRecordRequest, MemoryImportApproval,
        MemoryImportReceipt, MemoryVersionImportPort, import_memory_record,
    };

    #[derive(Clone, Eq, PartialEq)]
    struct FakeError(&'static str);

    struct FakePort {
        calls: Cell<u32>,
        error: RefCell<Option<FakeError>>,
        cancel_during_call: Cell<bool>,
    }

    impl FakePort {
        fn successful() -> Self {
            Self {
                calls: Cell::new(0),
                error: RefCell::new(None),
                cancel_during_call: Cell::new(false),
            }
        }
    }

    impl MemoryVersionImportPort for FakePort {
        type Error = FakeError;

        fn import_memory_version(
            &self,
            repository: RepositoryIdentityDigest,
            record: MemoryRecord,
            _presentation: MemoryPresentationDigest,
            _source: MemoryObservationSource,
            _audit_actor: MemoryAuditActorId,
            _recorded_at: MemoryRecordedAtUnixMillis,
            _approval: MemoryImportApproval,
            cancelled: Arc<AtomicBool>,
            _deadline: Instant,
        ) -> Result<MemoryImportReceipt, Self::Error> {
            self.calls.set(self.calls.get() + 1);
            assert_eq!(record.scope().repository(), repository);
            if self.cancel_during_call.get() {
                cancelled.store(true, std::sync::atomic::Ordering::Release);
            }
            if let Some(error) = self.error.borrow().clone() {
                return Err(error);
            }
            Ok(MemoryImportReceipt::new(
                CanonicalMemoryDigest::new([0x44; 32]),
                true,
                true,
                true,
            ))
        }
    }

    fn record(repository: RepositoryIdentityDigest) -> MemoryRecord {
        let snapshot = SourceSnapshotDigest::new([0x22; 32]);
        let name = MemorySymbolName::try_new("publish".to_owned()).expect("name is valid");
        let evidence = RustSymbolMemoryEvidence::try_new(
            snapshot,
            RepositoryPath::try_from_bytes(b"src/lib.rs", RepositoryPathLimits::new(128, 8))
                .expect("path is valid"),
            SourceContentDigest::new([0x33; 32]),
            AnalysisArtifactDigest::new([0x44; 32]),
            MemoryFactOrdinal::try_new(0).expect("ordinal is valid"),
            RustMemorySymbolKind::Function,
            name,
            MemoryQualifiedName::try_new("crate::publish".to_owned())
                .expect("qualified name is valid"),
            ByteSpan::try_new(ByteOffset::new(3), ByteOffset::new(10)).expect("name span is valid"),
            ByteSpan::try_new(ByteOffset::new(0), ByteOffset::new(20))
                .expect("declaration span is valid"),
            DeclarationDigest::new([0x55; 32]),
            ProducerIdentity::new(
                MemoryProducerId::try_new("repowitness.rust.syntax".to_owned())
                    .expect("producer ID is valid"),
                MemoryProducerVersion::try_new("phase0-rust-syntax-v1".to_owned())
                    .expect("producer version is valid"),
            ),
        )
        .expect("evidence is valid");
        MemoryRecord::try_new(
            MemoryRecordHeader::try_new(
                MemoryRecordId::new([0x11; 16]),
                MemoryDisplayRevision::try_new(1).expect("display revision is valid"),
                Vec::new(),
            )
            .expect("header is valid"),
            MemoryClaim::new(
                MemoryKind::Decision,
                MemoryTitle::try_new("Keep publication atomic".to_owned()).expect("title is valid"),
                MemoryBody::try_new("Readers see only complete generations.".to_owned())
                    .expect("body is valid"),
            ),
            MemoryScope::new(
                repository,
                MemoryEvidenceIndex::try_new(0).expect("evidence index is valid"),
            ),
            MemoryProvenance::new(
                MemoryProvenanceOrigin::Human,
                MemoryActorKind::LocalAsserted,
                MemoryActorId::try_new("maintainer".to_owned()).expect("actor is valid"),
            ),
            MemoryAssurance::LocallyApproved,
            MemoryLifecycle::Active,
            MemoryValidity::worktree(snapshot),
            vec![MemoryEvidence::RustSymbol(evidence)],
            Vec::new(),
            false,
        )
        .expect("record is valid")
    }

    fn request(
        repository: RepositoryIdentityDigest,
        scoped_repository: RepositoryIdentityDigest,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> ImportMemoryRecordRequest {
        ImportMemoryRecordRequest::new(
            repository,
            record(scoped_repository),
            MemoryPresentationDigest::new([0xA5; 32]),
            MemoryObservationSource::Worktree(SourceSnapshotDigest::new([0x66; 32])),
            MemoryAuditActorId::try_new("trusted-local-actor".to_owned())
                .expect("audit actor is valid"),
            MemoryRecordedAtUnixMillis::try_new(1_722_000_000_000).expect("timestamp is valid"),
            MemoryImportApproval::LocallyApproved,
            cancelled,
            deadline,
        )
    }

    #[test]
    fn matching_scope_is_imported_once_with_exact_receipt() {
        let repository = RepositoryIdentityDigest::new([0x10; 32]);
        let port = FakePort::successful();
        let receipt = import_memory_record(
            &port,
            request(
                repository,
                repository,
                Arc::new(AtomicBool::new(false)),
                Instant::now() + Duration::from_secs(5),
            ),
        )
        .expect("matching request should import");

        assert_eq!(port.calls.get(), 1);
        assert_eq!(receipt.revision(), CanonicalMemoryDigest::new([0x44; 32]));
        assert!(receipt.version_inserted());
        assert!(receipt.observation_inserted());
        assert!(receipt.approval_inserted());
    }

    #[test]
    fn controls_and_scope_fail_before_the_port() {
        let repository = RepositoryIdentityDigest::new([0x10; 32]);
        let other_repository = RepositoryIdentityDigest::new([0x20; 32]);
        let port = FakePort::successful();

        assert_eq!(
            import_memory_record(
                &port,
                request(
                    repository,
                    repository,
                    Arc::new(AtomicBool::new(true)),
                    Instant::now() + Duration::from_secs(5),
                ),
            ),
            Err(ImportMemoryRecordError::Cancelled)
        );
        assert_eq!(
            import_memory_record(
                &port,
                request(
                    repository,
                    repository,
                    Arc::new(AtomicBool::new(false)),
                    Instant::now(),
                ),
            ),
            Err(ImportMemoryRecordError::DeadlineExceeded)
        );
        assert_eq!(
            import_memory_record(
                &port,
                request(
                    repository,
                    other_repository,
                    Arc::new(AtomicBool::new(false)),
                    Instant::now() + Duration::from_secs(5),
                ),
            ),
            Err(ImportMemoryRecordError::ScopeMismatch)
        );
        assert_eq!(port.calls.get(), 0);
    }

    #[test]
    fn port_failures_are_distinct_and_redacted() {
        let repository = RepositoryIdentityDigest::new([0x10; 32]);
        let port = FakePort {
            calls: Cell::new(0),
            error: RefCell::new(Some(FakeError("private adapter detail"))),
            cancel_during_call: Cell::new(false),
        };
        let error = import_memory_record(
            &port,
            request(
                repository,
                repository,
                Arc::new(AtomicBool::new(false)),
                Instant::now() + Duration::from_secs(5),
            ),
        )
        .expect_err("port failure should propagate by category");

        assert_eq!(
            error,
            ImportMemoryRecordError::Port(FakeError("private adapter detail"))
        );
        assert_eq!(error.to_string(), "memory persistence failed");
        assert!(!format!("{error:?}").contains("private"));
    }

    #[test]
    fn request_debug_redacts_identity_and_actor_values() {
        let repository = RepositoryIdentityDigest::new([0xA5; 32]);
        let request = request(
            repository,
            repository,
            Arc::new(AtomicBool::new(false)),
            Instant::now() + Duration::from_secs(5),
        );
        let debug = format!("{request:?}");

        assert!(!debug.contains("trusted-local-actor"));
        assert!(!debug.contains("A5"));
        assert!(!debug.contains("165"));
    }
}
