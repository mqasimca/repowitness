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
    },
    Review {
        inserted: bool,
    },
    ImportHistory {
        commits_inspected: u32,
        records_inspected: u32,
        imported_versions: u32,
        appended_observations: u32,
        total_record_bytes: u64,
        git_processes: u32,
        history_complete: bool,
    },
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
    const fn complete() -> Self {
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
) -> Result<CliMemoryManageReport, String> {
    let now = current_unix_ms()?;
    let cancelled = Arc::new(AtomicBool::new(false));
    match invocation {
        MemoryManageInvocation::Write { .. } => manage_local_memory_write(invocation, cancelled),
        MemoryManageInvocation::Approve { .. } => {
            manage_local_memory_approve(invocation, now, cancelled)
        }
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
) -> Result<CliMemoryManageReport, String> {
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
    .map_err(|error| error.to_string())
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
) -> Result<CliMemoryManageReport, String> {
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
    })
    .map_err(|error| error.to_string())
}

fn manage_local_memory_review(
    invocation: &MemoryManageInvocation,
    now: u64,
    cancelled: Arc<AtomicBool>,
) -> Result<CliMemoryManageReport, String> {
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
    review_local_memory_correspondence(
        LocalMemoryCorrespondenceReviewRequest::new(
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
        ),
        cancelled,
    )
    .map(|receipt| CliMemoryManageReport::Review {
        inserted: receipt.inserted(),
    })
    .map_err(|error| error.to_string())
}

fn manage_local_memory_history(
    invocation: &MemoryManageInvocation,
    now: u64,
    cancelled: Arc<AtomicBool>,
) -> Result<CliMemoryManageReport, String> {
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
    })
    .map_err(|error| error.to_string())
}

fn manage_utf8(value: &OsStr) -> Result<&str, String> {
    value
        .to_str()
        .ok_or_else(|| "memory management text is not valid UTF-8".to_owned())
}
