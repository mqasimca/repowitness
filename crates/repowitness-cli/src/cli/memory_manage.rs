enum MemoryManageInvocation {
    Write {
        repository_root: PathBuf,
        repository_identity: OsString,
        input: PathBuf,
    },
    Approve {
        repository_root: PathBuf,
        database: PathBuf,
        repository_identity: OsString,
        record_id: OsString,
        actor: OsString,
    },
    Sync {
        repository_root: PathBuf,
        database: PathBuf,
        repository_identity: OsString,
        record_id: OsString,
        actor: OsString,
    },
    Review {
        repository_root: PathBuf,
        database: PathBuf,
        repository_identity: OsString,
        record_id: OsString,
        revision: OsString,
        evidence_ordinal: u8,
        operation: MemoryCorrespondenceReviewOperation,
        target_path: OsString,
        target_artifact: OsString,
        target_fact_ordinal: u64,
        target_snapshot: Option<OsString>,
        actor: OsString,
    },
    ImportHistory {
        repository_root: PathBuf,
        database: PathBuf,
        repository_identity: OsString,
        actor: OsString,
    },
}

enum CliMemoryManageReport {
    Write {
        revision: String,
        created: bool,
        canonical_bytes: u64,
        publication: CliMemoryPublicationStatus,
    },
    Approve {
        revision: String,
        version_inserted: bool,
        observation_inserted: bool,
        approval_inserted: bool,
        maintenance: CliMemoryMaintenanceStatus,
    },
    Sync {
        revision: String,
        version_inserted: bool,
        observation_inserted: bool,
        maintenance: CliMemoryMaintenanceStatus,
    },
    Review {
        inserted: bool,
        maintenance: CliMemoryMaintenanceStatus,
    },
    ImportHistory {
        commits_inspected: u32,
        records_inspected: u32,
        imported_versions: u32,
        appended_observations: u32,
        total_record_bytes: u64,
        git_processes: u32,
        history_complete: bool,
        maintenance: CliMemoryMaintenanceStatus,
    },
}

#[derive(Clone, Copy)]
struct CliMemoryMaintenanceStatus {
    complete: bool,
    warning_count: u8,
    checkpoint: &'static str,
    shutdown: &'static str,
    database_identity: &'static str,
}

#[cfg(test)]
impl CliMemoryMaintenanceStatus {
    const fn confirmed_for_test() -> Self {
        Self {
            complete: true,
            warning_count: 0,
            checkpoint: "complete",
            shutdown: "complete",
            database_identity: "confirmed_at_final_fence",
        }
    }

    const fn checkpoint_deferred_for_test() -> Self {
        Self {
            complete: false,
            warning_count: 1,
            checkpoint: "deferred",
            shutdown: "complete",
            database_identity: "confirmed_at_final_fence",
        }
    }

    const fn shutdown_deferred_for_test() -> Self {
        Self {
            complete: false,
            warning_count: 1,
            checkpoint: "complete",
            shutdown: "deferred",
            database_identity: "confirmed_at_final_fence",
        }
    }

    const fn all_steps_deferred_for_test() -> Self {
        Self {
            complete: false,
            warning_count: 2,
            checkpoint: "deferred",
            shutdown: "deferred",
            database_identity: "confirmed_at_final_fence",
        }
    }

    const fn changed_database_for_test() -> Self {
        Self {
            complete: false,
            warning_count: 1,
            checkpoint: "complete",
            shutdown: "complete",
            database_identity: "changed_after_commit",
        }
    }
}

#[derive(Clone, Copy)]
struct CliMemoryPublicationStatus {
    complete: bool,
    warning_count: u8,
    temporary_cleanup: &'static str,
    target_identity: &'static str,
    records_directory_identity: &'static str,
    directory_sync: &'static str,
}

impl CliMemoryPublicationStatus {
    #[cfg(test)]
    const fn confirmed_for_test() -> Self {
        Self {
            complete: true,
            warning_count: 0,
            temporary_cleanup: "complete",
            target_identity: "confirmed_at_final_fence",
            records_directory_identity: "confirmed_at_final_fence",
            directory_sync: "complete",
        }
    }
}

fn manage_local_memory(
    invocation: &MemoryManageInvocation,
) -> Result<CliMemoryManageReport, CliMemoryError> {
    let now = current_unix_ms()?;
    let cancelled = Arc::new(AtomicBool::new(false));
    match invocation {
        MemoryManageInvocation::Write { .. } => manage_local_memory_write(invocation, cancelled),
        MemoryManageInvocation::Approve { .. } => {
            manage_local_memory_approve(invocation, now, cancelled)
        }
        MemoryManageInvocation::Sync { .. } => manage_local_memory_sync(invocation, now, cancelled),
        MemoryManageInvocation::Review { .. } => {
            manage_local_memory_review(invocation, now, cancelled)
        }
        MemoryManageInvocation::ImportHistory { .. } => {
            manage_local_memory_history(invocation, now, cancelled)
        }
    }
}

fn manage_local_memory_write(
    invocation: &MemoryManageInvocation,
    cancelled: Arc<AtomicBool>,
) -> Result<CliMemoryManageReport, CliMemoryError> {
    let MemoryManageInvocation::Write {
        repository_root,
        repository_identity,
        input,
    } = invocation
    else {
        unreachable!("write dispatcher supplied another operation");
    };
    let repository_identity = manage_utf8(repository_identity)?;
    write_local_memory(
        LocalMemoryWriteRequest::new(repository_root, input, repository_identity),
        cancelled,
    )
    .map(|receipt| CliMemoryManageReport::Write {
        revision: hex(receipt.revision().as_bytes()),
        created: receipt.created(),
        canonical_bytes: receipt.canonical_bytes(),
        publication: cli_memory_publication_status(receipt.publication_status()),
    })
    .map_err(|error| CliMemoryError::from_management(MemoryMutationRequestScope::Write, error))
}

fn cli_memory_publication_status(
    status: repowitness_local::LocalMemoryFilePublicationStatus,
) -> CliMemoryPublicationStatus {
    CliMemoryPublicationStatus {
        complete: status.is_complete(),
        warning_count: status.warning_count(),
        temporary_cleanup: cli_memory_publication_step(status.temporary_cleanup()),
        target_identity: cli_memory_identity(status.target_identity()),
        records_directory_identity: cli_memory_identity(status.records_directory_identity()),
        directory_sync: cli_memory_publication_step(status.directory_sync()),
    }
}

const fn cli_memory_identity(status: MemoryFileIdentityStatus) -> &'static str {
    match status {
        MemoryFileIdentityStatus::ConfirmedAtFinalFence => "confirmed_at_final_fence",
        MemoryFileIdentityStatus::ChangedAfterCommit => "changed_after_commit",
    }
}

const fn cli_memory_publication_step(status: MemoryFilePublicationStepStatus) -> &'static str {
    match status {
        MemoryFilePublicationStepStatus::NotRequired => "not_required",
        MemoryFilePublicationStepStatus::Complete => "complete",
        MemoryFilePublicationStepStatus::Deferred => "deferred",
    }
}

fn manage_local_memory_approve(
    invocation: &MemoryManageInvocation,
    now: u64,
    cancelled: Arc<AtomicBool>,
) -> Result<CliMemoryManageReport, CliMemoryError> {
    let MemoryManageInvocation::Approve {
        repository_root,
        database,
        repository_identity,
        record_id,
        actor,
    } = invocation
    else {
        unreachable!("approval dispatcher supplied another operation");
    };
    let repository_identity = manage_utf8(repository_identity)?;
    let record_id = manage_utf8(record_id)?;
    let actor = manage_utf8(actor)?;
    approve_local_memory(
        LocalMemoryApprovalRequest::new(
            repository_root,
            database,
            repository_identity,
            record_id,
            actor,
            now,
            now,
        ),
        cancelled,
    )
    .map(|receipt| CliMemoryManageReport::Approve {
        revision: hex(receipt.revision().as_bytes()),
        version_inserted: receipt.version_inserted(),
        observation_inserted: receipt.observation_inserted(),
        approval_inserted: receipt.approval_inserted(),
        maintenance: cli_memory_maintenance(receipt.maintenance()),
    })
    .map_err(|error| CliMemoryError::from_management(MemoryMutationRequestScope::Approve, error))
}

fn manage_local_memory_sync(
    invocation: &MemoryManageInvocation,
    now: u64,
    cancelled: Arc<AtomicBool>,
) -> Result<CliMemoryManageReport, CliMemoryError> {
    let MemoryManageInvocation::Sync {
        repository_root,
        database,
        repository_identity,
        record_id,
        actor,
    } = invocation else {
        unreachable!("team-sync dispatcher supplied another operation");
    };
    let repository_identity = manage_utf8(repository_identity)?;
    let record_id = manage_utf8(record_id)?;
    let actor = manage_utf8(actor)?;
    sync_local_team_memory(
        LocalTeamMemorySyncRequest::new(
            repository_root,
            database,
            repository_identity,
            record_id,
            actor,
            now,
            now,
        ),
        cancelled,
    )
    .map(|receipt| CliMemoryManageReport::Sync {
        revision: hex(receipt.revision().as_bytes()),
        version_inserted: receipt.version_inserted(),
        observation_inserted: receipt.observation_inserted(),
        maintenance: cli_memory_maintenance(receipt.maintenance()),
    })
    .map_err(|error| CliMemoryError::from_management(MemoryMutationRequestScope::TeamSync, error))
}

fn manage_local_memory_review(
    invocation: &MemoryManageInvocation,
    now: u64,
    cancelled: Arc<AtomicBool>,
) -> Result<CliMemoryManageReport, CliMemoryError> {
    let MemoryManageInvocation::Review {
        repository_root,
        database,
        repository_identity,
        record_id,
        revision,
        evidence_ordinal,
        operation,
        target_path,
        target_artifact,
        target_fact_ordinal,
        target_snapshot,
        actor,
    } = invocation
    else {
        unreachable!("review dispatcher supplied another operation");
    };
    let repository_identity = manage_utf8(repository_identity)?;
    let record_id = manage_utf8(record_id)?;
    let revision = manage_utf8(revision)?;
    let target_path = manage_utf8(target_path)?;
    let target_artifact = manage_utf8(target_artifact)?;
    let actor = manage_utf8(actor)?;
    let request = LocalMemoryCorrespondenceReviewRequest::new(
            repository_root,
            database,
            repository_identity,
            record_id,
            revision,
            *evidence_ordinal,
            *operation,
            target_path,
            target_artifact,
            *target_fact_ordinal,
            actor,
            now,
            now,
        );
    let request = match target_snapshot {
        Some(snapshot) => request.with_archival_target_snapshot_sha256(manage_utf8(snapshot)?),
        None => request,
    };
    review_local_memory_correspondence(request, cancelled)
    .map(|receipt| CliMemoryManageReport::Review {
        inserted: receipt.inserted(),
        maintenance: cli_memory_maintenance(receipt.maintenance()),
    })
    .map_err(|error| CliMemoryError::from_management(MemoryMutationRequestScope::Review, error))
}

fn manage_local_memory_history(
    invocation: &MemoryManageInvocation,
    now: u64,
    cancelled: Arc<AtomicBool>,
) -> Result<CliMemoryManageReport, CliMemoryError> {
    let MemoryManageInvocation::ImportHistory {
        repository_root,
        database,
        repository_identity,
        actor,
    } = invocation
    else {
        unreachable!("history dispatcher supplied another operation");
    };
    let repository_identity = manage_utf8(repository_identity)?;
    let actor = manage_utf8(actor)?;
    import_local_memory_history(
        LocalMemoryHistoryImportRequest::new(
            repository_root,
            database,
            repository_identity,
            actor,
            now,
            now,
        ),
        cancelled,
    )
    .map(|report| CliMemoryManageReport::ImportHistory {
        commits_inspected: report.commits_inspected(),
        records_inspected: report.records_inspected(),
        imported_versions: report.imported_versions(),
        appended_observations: report.appended_observations(),
        total_record_bytes: report.total_record_bytes(),
        git_processes: report.git_processes(),
        history_complete: report.history_complete(),
        maintenance: cli_memory_maintenance(report.maintenance()),
    })
    .map_err(|error| {
        CliMemoryError::from_management(MemoryMutationRequestScope::ImportHistory, error)
    })
}

fn cli_memory_maintenance(status: LocalMemoryMaintenance) -> CliMemoryMaintenanceStatus {
    CliMemoryMaintenanceStatus {
        complete: status.complete(),
        warning_count: status.warning_count(),
        checkpoint: match status.checkpoint() {
            LocalMemoryMaintenanceStep::Complete => "complete",
            LocalMemoryMaintenanceStep::Deferred => "deferred",
        },
        shutdown: match status.shutdown() {
            LocalMemoryMaintenanceStep::Complete => "complete",
            LocalMemoryMaintenanceStep::Deferred => "deferred",
        },
        database_identity: match status.database_identity() {
            LocalMemoryDatabaseIdentity::ConfirmedAtFinalFence => "confirmed_at_final_fence",
            LocalMemoryDatabaseIdentity::ChangedAfterCommit => "changed_after_commit",
            LocalMemoryDatabaseIdentity::Unconfirmed => "unconfirmed",
        },
    }
}

fn manage_utf8(value: &OsStr) -> Result<&str, CliMemoryError> {
    value.to_str().ok_or(CliMemoryError::Failed)
}
