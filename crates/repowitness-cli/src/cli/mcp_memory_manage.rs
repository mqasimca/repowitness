fn manage_mcp_memory(
    service: &LocalMcpRepositoryService,
    request: MemoryManageServiceRequest,
    cancelled: Arc<AtomicBool>,
) -> Result<MemoryManageOutput, RepositoryServiceError> {
    let actor = service
        .memory_actor
        .as_deref()
        .ok_or(RepositoryServiceError::MemoryManage)?;
    let now = current_unix_ms().map_err(|_| RepositoryServiceError::MemoryManage)?;
    match request {
        request @ MemoryManageServiceRequest::Write { .. } => {
            manage_mcp_memory_write(service, request, cancelled)
        }
        request @ MemoryManageServiceRequest::Approve { .. } => {
            manage_mcp_memory_approve(service, request, actor, now, cancelled)
        }
        request @ MemoryManageServiceRequest::Review { .. } => {
            manage_mcp_memory_review(service, request, actor, now, cancelled)
        }
        request @ MemoryManageServiceRequest::ImportHistory { .. } => {
            manage_mcp_memory_history(service, request, actor, now, cancelled)
        }
    }
}

fn manage_mcp_memory_write(
    service: &LocalMcpRepositoryService,
    request: MemoryManageServiceRequest,
    cancelled: Arc<AtomicBool>,
) -> Result<MemoryManageOutput, RepositoryServiceError> {
    let MemoryManageServiceRequest::Write {
        record_yaml,
        timeout,
    } = request
    else {
        unreachable!("write dispatcher supplied another operation");
    };
    write_local_memory(
        LocalMemoryWriteRequest::from_bytes(
            &service.root,
            record_yaml.as_bytes(),
            &service.repository_identity,
        )
        .with_deadline(timeout),
        cancelled,
    )
    .map(|receipt| {
        MemoryManageOutput::write(
            hex(receipt.revision().as_bytes()),
            receipt.created(),
            receipt.canonical_bytes(),
        )
    })
    .map_err(|_| RepositoryServiceError::MemoryManage)
}

fn manage_mcp_memory_approve(
    service: &LocalMcpRepositoryService,
    request: MemoryManageServiceRequest,
    actor: &str,
    now: u64,
    cancelled: Arc<AtomicBool>,
) -> Result<MemoryManageOutput, RepositoryServiceError> {
    let MemoryManageServiceRequest::Approve { record_id, timeout } = request else {
        unreachable!("approval dispatcher supplied another operation");
    };
    approve_local_memory(
        LocalMemoryApprovalRequest::new(
            &service.root,
            &service.database,
            &service.repository_identity,
            &record_id,
            actor,
            now,
            now,
        )
        .with_deadline(timeout),
        cancelled,
    )
    .map(|receipt| {
        MemoryManageOutput::approve(
            hex(receipt.revision().as_bytes()),
            receipt.version_inserted(),
            receipt.observation_inserted(),
            receipt.approval_inserted(),
        )
    })
    .map_err(|_| RepositoryServiceError::MemoryManage)
}

fn manage_mcp_memory_review(
    service: &LocalMcpRepositoryService,
    request: MemoryManageServiceRequest,
    actor: &str,
    now: u64,
    cancelled: Arc<AtomicBool>,
) -> Result<MemoryManageOutput, RepositoryServiceError> {
    let MemoryManageServiceRequest::Review {
            record_id,
            revision_sha256,
            evidence_ordinal,
            decision,
            target_path,
            target_artifact_sha256,
            target_fact_ordinal,
            timeout,
        } = request
    else {
        unreachable!("review dispatcher supplied another operation");
    };
    let operation = match decision {
        MemoryManageReviewDecision::Approve => MemoryCorrespondenceReviewOperation::Approved,
        MemoryManageReviewDecision::Reject => MemoryCorrespondenceReviewOperation::Rejected,
        MemoryManageReviewDecision::ManualLink => MemoryCorrespondenceReviewOperation::ManualLink,
    };
    review_local_memory_correspondence(
        LocalMemoryCorrespondenceReviewRequest::new(
            &service.root,
            &service.database,
            &service.repository_identity,
            &record_id,
            &revision_sha256,
            evidence_ordinal,
            operation,
            &target_path,
            &target_artifact_sha256,
            target_fact_ordinal,
            actor,
            now,
            now,
        )
        .with_deadline(timeout),
        cancelled,
    )
    .map(|receipt| MemoryManageOutput::review(receipt.inserted()))
    .map_err(|_| RepositoryServiceError::MemoryManage)
}

fn manage_mcp_memory_history(
    service: &LocalMcpRepositoryService,
    request: MemoryManageServiceRequest,
    actor: &str,
    now: u64,
    cancelled: Arc<AtomicBool>,
) -> Result<MemoryManageOutput, RepositoryServiceError> {
    let MemoryManageServiceRequest::ImportHistory { timeout } = request else {
        unreachable!("history dispatcher supplied another operation");
    };
    import_local_memory_history(
        LocalMemoryHistoryImportRequest::new(
            &service.root,
            &service.database,
            &service.repository_identity,
            actor,
            now,
            now,
        )
        .with_deadline(timeout),
        cancelled,
    )
    .map(|report| {
        MemoryManageOutput::import_history(
            report.commits_inspected(),
            report.records_inspected(),
            report.imported_versions(),
            report.appended_observations(),
            report.total_record_bytes(),
            report.git_processes(),
            report.history_complete(),
        )
    })
    .map_err(|_| RepositoryServiceError::MemoryManage)
}
