/// Durable receipt for one immutable personal-memory revision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PersonalMemoryReceipt {
    record_id: repowitness_domain::PersonalMemoryId,
    revision: repowitness_domain::PersonalMemoryRevision,
    inserted: bool,
}

impl PersonalMemoryReceipt {
    /// Returns the opaque local record identity.
    #[must_use]
    pub const fn record_id(self) -> repowitness_domain::PersonalMemoryId {
        self.record_id
    }

    /// Returns the immutable revision identity.
    #[must_use]
    pub const fn revision(self) -> repowitness_domain::PersonalMemoryRevision {
        self.revision
    }

    /// Reports whether this exact immutable revision was newly appended.
    #[must_use]
    pub const fn inserted(self) -> bool {
        self.inserted
    }
}

impl WriterState {
    pub(super) fn append_personal_memory(
        &mut self,
        record: &PersonalMemoryRecord,
        control: WriteControl<'_>,
    ) -> Result<PersonalMemoryReceipt, SqliteStoreError> {
        check_control(control)?;
        if crate::memory_management::secret::contains_sensitive_text(record.title().as_str())
            || crate::memory_management::secret::contains_sensitive_text(record.body().as_str())
        {
            return Err(SqliteStoreError::InvalidPersonalMemory);
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        let inserted = transaction
            .execute(
                "INSERT OR IGNORE INTO personal_memory_records(
                    profile_id, repository_identity, record_id, revision_digest, kind, title,
                    body, lifecycle, recorded_at_unix_ms
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    record.profile().as_bytes().as_slice(),
                    record.repository().as_bytes().as_slice(),
                    record.record_id().as_bytes().as_slice(),
                    record.revision().as_bytes().as_slice(),
                    personal_memory_kind_text(record.kind()),
                    record.title().as_str(),
                    record.body().as_str(),
                    personal_memory_lifecycle_text(record.lifecycle()),
                    i64::try_from(record.recorded_at_unix_ms())
                        .map_err(|_| SqliteStoreError::InvalidPersonalMemory)?,
                ],
            )
            .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?
            == 1;
        if inserted {
            transaction
                .execute(
                    "INSERT INTO personal_memory_audit(
                        profile_id, repository_identity, record_id, revision_digest, operation,
                        recorded_at_unix_ms
                     ) VALUES (?1, ?2, ?3, ?4, 'recorded', ?5)",
                    params![
                        record.profile().as_bytes().as_slice(),
                        record.repository().as_bytes().as_slice(),
                        record.record_id().as_bytes().as_slice(),
                        record.revision().as_bytes().as_slice(),
                        i64::try_from(record.recorded_at_unix_ms())
                            .map_err(|_| SqliteStoreError::InvalidPersonalMemory)?,
                    ],
                )
                .map_err(|_| SqliteStoreError::DatabaseOperationFailed)?;
        }
        check_control(control)?;
        commit_mutation(transaction)?;
        Ok(PersonalMemoryReceipt {
            record_id: record.record_id(),
            revision: record.revision(),
            inserted,
        })
    }
}

const fn personal_memory_kind_text(kind: PersonalMemoryKind) -> &'static str {
    match kind {
        PersonalMemoryKind::Fact => "fact",
        PersonalMemoryKind::Decision => "decision",
        PersonalMemoryKind::Procedure => "procedure",
        PersonalMemoryKind::Episode => "episode",
        PersonalMemoryKind::Preference => "preference",
        PersonalMemoryKind::Policy => "policy",
        PersonalMemoryKind::Failure => "failure",
    }
}

const fn personal_memory_lifecycle_text(lifecycle: MemoryLifecycle) -> &'static str {
    match lifecycle {
        MemoryLifecycle::Active => "active",
        MemoryLifecycle::NeedsReview => "needs_review",
        MemoryLifecycle::Stale => "stale",
        MemoryLifecycle::Contradicted => "contradicted",
        MemoryLifecycle::Superseded => "superseded",
        MemoryLifecycle::Quarantined => "quarantined",
        MemoryLifecycle::Tombstoned => "tombstoned",
    }
}
