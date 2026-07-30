struct MemoryRevalidationInvocation {
    repository_root: PathBuf,
    database: PathBuf,
    repository_identity: OsString,
}

struct MemoryRecallInvocation {
    database: PathBuf,
    repository_identity: OsString,
    selection: CliMemoryRecallSelection,
    max_results: u16,
}

enum CliMemoryRecallSelection {
    All,
    Query(OsString),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CliMemoryError {
    Failed,
    MutationOutcomeUnknown {
        request_scope: MemoryMutationRequestScope,
        operation: MemoryMutationOperation,
    },
}

impl CliMemoryError {
    const fn from_management(
        request_scope: MemoryMutationRequestScope,
        error: LocalMemoryManageError,
    ) -> Self {
        match error {
            LocalMemoryManageError::MutationOutcomeUnknown { operation } => {
                Self::MutationOutcomeUnknown {
                    request_scope,
                    operation: memory_mutation_operation(operation),
                }
            }
            _ => Self::Failed,
        }
    }

    fn from_revalidation(error: LocalMemoryRevalidationError) -> Self {
        match error {
            LocalMemoryRevalidationError::MutationOutcomeUnknown { operation } => {
                Self::MutationOutcomeUnknown {
                    request_scope: MemoryMutationRequestScope::Revalidation,
                    operation: memory_revalidation_mutation_operation(operation),
                }
            }
            _ => Self::Failed,
        }
    }
}

const fn memory_mutation_operation(operation: LocalMemoryMutation) -> MemoryMutationOperation {
    match operation {
        LocalMemoryMutation::StoreStartup => MemoryMutationOperation::StoreStartup,
        LocalMemoryMutation::Approval => MemoryMutationOperation::Approval,
        LocalMemoryMutation::HistoryImport => MemoryMutationOperation::HistoryImport,
        LocalMemoryMutation::CorrespondenceReview => MemoryMutationOperation::CorrespondenceReview,
        LocalMemoryMutation::Checkpoint => MemoryMutationOperation::Checkpoint,
    }
}

const fn memory_revalidation_mutation_operation(
    operation: LocalMemoryRevalidationMutation,
) -> MemoryMutationOperation {
    match operation {
        LocalMemoryRevalidationMutation::StoreStartup => MemoryMutationOperation::StoreStartup,
        LocalMemoryRevalidationMutation::ProjectionPublication => {
            MemoryMutationOperation::ProjectionPublication
        }
        LocalMemoryRevalidationMutation::Checkpoint => MemoryMutationOperation::Checkpoint,
    }
}

trait RepositoryMemory {
    fn revalidate(
        &self,
        invocation: &MemoryRevalidationInvocation,
    ) -> Result<CliMemoryRevalidationReport, CliMemoryError>;

    fn recall(
        &self,
        invocation: &MemoryRecallInvocation,
        configuration: &ResolvedConfiguration,
    ) -> Result<MemoryRecallOutput, CliMemoryError>;

    fn manage(
        &self,
        _invocation: &MemoryManageInvocation,
    ) -> Result<CliMemoryManageReport, CliMemoryError> {
        Err(CliMemoryError::Failed)
    }
}

struct LocalRepositoryMemory;

impl RepositoryMemory for LocalRepositoryMemory {
    fn revalidate(
        &self,
        invocation: &MemoryRevalidationInvocation,
    ) -> Result<CliMemoryRevalidationReport, CliMemoryError> {
        let repository_identity = invocation
            .repository_identity
            .to_str()
            .ok_or(CliMemoryError::Failed)?;
        let applied_at_unix_ms = current_unix_ms()?;
        revalidate_local_memory(
            LocalMemoryRevalidationRequest::new(
                &invocation.repository_root,
                &invocation.database,
                repository_identity,
                applied_at_unix_ms,
            ),
            Arc::new(AtomicBool::new(false)),
        )
        .map(CliMemoryRevalidationReport::from)
        .map_err(CliMemoryError::from_revalidation)
    }

    fn recall(
        &self,
        invocation: &MemoryRecallInvocation,
        configuration: &ResolvedConfiguration,
    ) -> Result<MemoryRecallOutput, CliMemoryError> {
        let repository_identity = invocation
            .repository_identity
            .to_str()
            .ok_or(CliMemoryError::Failed)?;
        let selection = match &invocation.selection {
            CliMemoryRecallSelection::All => LocalMemoryRecallSelection::All,
            CliMemoryRecallSelection::Query(query) => {
                LocalMemoryRecallSelection::Query(query.to_str().ok_or(CliMemoryError::Failed)?)
            }
        };
        let request =
            LocalMemoryRecallRequest::new(&invocation.database, repository_identity, selection)
                .with_max_results(invocation.max_results)
                .map_err(|_| CliMemoryError::Failed)?
                .with_configuration(configuration);
        recall_local_memory(request, Arc::new(AtomicBool::new(false)))
            .map_err(|_| CliMemoryError::Failed)
            .and_then(|result| mcp_memory_output(result).map_err(|_| CliMemoryError::Failed))
    }

    fn manage(
        &self,
        invocation: &MemoryManageInvocation,
    ) -> Result<CliMemoryManageReport, CliMemoryError> {
        manage_local_memory(invocation)
    }
}

fn current_unix_ms() -> Result<u64, CliMemoryError> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| CliMemoryError::Failed)?;
    u64::try_from(elapsed.as_millis()).map_err(|_| CliMemoryError::Failed)
}

#[derive(Clone, Copy)]
struct CliMemoryRevalidationReport {
    projection_id: i64,
    generation: i64,
    source_epoch: u64,
    recovered_generations: u64,
    projected_records: u32,
    skipped_records: u32,
    unresolved_records: u32,
    git_queries: u32,
    head_available: bool,
    maintenance: CliMemoryMaintenanceStatus,
}

impl From<LocalMemoryRevalidationReport> for CliMemoryRevalidationReport {
    fn from(report: LocalMemoryRevalidationReport) -> Self {
        Self {
            projection_id: report.projection_id(),
            generation: report.generation().get(),
            source_epoch: report.source_epoch(),
            recovered_generations: report.recovered_generations(),
            projected_records: report.projected_records(),
            skipped_records: report.skipped_records(),
            unresolved_records: report.unresolved_records(),
            git_queries: report.git_queries(),
            head_available: report.head_available(),
            maintenance: cli_memory_maintenance(report.maintenance()),
        }
    }
}

fn mcp_memory_output(result: LocalMemoryRecallResult) -> Result<MemoryRecallOutput, String> {
    let coverage = result.projection_coverage();
    let mut records = Vec::with_capacity(result.records().len());
    for record in result.records() {
        records.push(mcp_memory_record(record)?);
    }
    let matches_returned =
        u64::try_from(records.len()).map_err(|_| "memory result count is too large".to_owned())?;
    Ok(MemoryRecallOutput {
        schema_version: 1,
        recall_profile: result.profile_version(),
        query_sha256: result.query().map(|query| hex(query.as_bytes())),
        snapshot_sha256: hex(result.snapshot().as_bytes()),
        generation: result.generation().get(),
        projection: *result.projection(),
        source_epoch: result.source_epoch(),
        target: mcp_memory_target(result.target()),
        producer: McpMemoryProducer {
            id: result.producer().id().to_owned(),
            version: result.producer().version(),
            profile_sha256: hex(result.producer().digest().as_bytes()),
        },
        matches_returned,
        matches_total: result.total_matches(),
        matches_omitted: result.omitted_matches(),
        coverage: mcp_memory_coverage(coverage),
        limitation: "rust_symbol_memory_only".to_owned(),
        records,
    })
}

fn mcp_memory_target(target: MemoryRevalidationTarget) -> McpMemoryTarget {
    match target {
        MemoryRevalidationTarget::Git { commit } => {
            let (format, identity) = memory_commit(commit);
            McpMemoryTarget {
                kind: "git".to_owned(),
                source_snapshot_sha256: None,
                commit_object_format: Some(format.to_owned()),
                commit_hex: Some(identity),
            }
        }
        MemoryRevalidationTarget::Worktree {
            source_snapshot,
            head,
        } => {
            let (commit_object_format, commit_hex) = head
                .map(memory_commit)
                .map_or((None, None), |(format, identity)| {
                    (Some(format.to_owned()), Some(identity))
                });
            McpMemoryTarget {
                kind: "worktree".to_owned(),
                source_snapshot_sha256: Some(hex(source_snapshot.as_bytes())),
                commit_object_format,
                commit_hex,
            }
        }
    }
}

fn memory_commit(commit: MemoryCommitId) -> (&'static str, String) {
    let format = match commit.object_format() {
        MemoryObjectFormat::Sha1 => "sha1",
        MemoryObjectFormat::Sha256 => "sha256",
    };
    (format, hex(commit.as_bytes()))
}

fn mcp_memory_record(record: &MemoryRecallRecord) -> Result<McpMemoryRecord, String> {
    let selected = record.record().map(|selected| McpSelectedMemory {
        schema_version: selected.schema_version(),
        display_revision: selected.header().display_revision().get(),
        kind: memory_kind(selected.claim().kind()).to_owned(),
        title: selected.claim().title().as_str().to_owned(),
        body: selected.claim().body().as_str().to_owned(),
        assurance: memory_assurance(selected.assurance()).to_owned(),
        lifecycle: memory_lifecycle(selected.lifecycle()).to_owned(),
        tombstone: selected.tombstone(),
    });
    let mut evidence = Vec::with_capacity(record.evidence().len());
    for result in record.evidence() {
        evidence.push(mcp_memory_evidence(result)?);
    }
    Ok(McpMemoryRecord {
        record_id: MemoryRecordIdTextV1::encode(record.record_id()).into_string(),
        revision_sha256: record.revision().map(|revision| hex(revision.as_bytes())),
        selected,
        effective_state: effective_state(record.effective_state()).to_owned(),
        validity_state: validity_state(record.validity_state()).to_owned(),
        evidence_state: evidence_state(record.evidence_state()).to_owned(),
        reason: memory_reason(record.reason()).to_owned(),
        evidence_count: record.evidence_count(),
        resolved_count: record.resolved_count(),
        review_count: record.review_count(),
        indeterminate_count: record.indeterminate_count(),
        head_count: record.head_count(),
        missing_parent_count: record.missing_parent_count(),
        evidence,
    })
}

fn mcp_memory_coverage(
    coverage: repowitness_local::MemoryRecallProjectionCoverage,
) -> McpMemoryCoverage {
    McpMemoryCoverage {
        searched: coverage.searched(),
        skipped: coverage.skipped(),
        unresolved: coverage.unresolved(),
        truncated: coverage.truncated(),
        total: coverage.total(),
        current: coverage.state_count(MemoryEffectiveState::Current),
        not_applicable: coverage.state_count(MemoryEffectiveState::NotApplicable),
        stale: coverage.state_count(MemoryEffectiveState::Stale),
        needs_review: coverage.state_count(MemoryEffectiveState::NeedsReview),
        indeterminate: coverage.state_count(MemoryEffectiveState::Indeterminate),
        conflicted: coverage.state_count(MemoryEffectiveState::Conflicted),
        contradicted: coverage.state_count(MemoryEffectiveState::Contradicted),
        superseded: coverage.state_count(MemoryEffectiveState::Superseded),
        quarantined: coverage.state_count(MemoryEffectiveState::Quarantined),
        tombstoned: coverage.state_count(MemoryEffectiveState::Tombstoned),
    }
}

fn mcp_memory_evidence(result: &MemoryRecallEvidence) -> Result<McpMemoryEvidence, String> {
    let target = result.target().map(mcp_memory_occurrence).transpose()?;
    let mut candidates = Vec::with_capacity(result.candidates().len());
    for candidate in result.candidates() {
        candidates.push(McpMemoryCandidate {
            relation: candidate_relation(candidate.relation()).to_owned(),
            occurrence: mcp_memory_occurrence(candidate.occurrence())?,
        });
    }
    Ok(McpMemoryEvidence {
        outcome: evidence_outcome(result.outcome()).to_owned(),
        assurance: evidence_assurance(result.assurance()).to_owned(),
        target,
        candidate_coverage_complete: result.candidate_coverage_complete(),
        candidate_count_before_limit: result.candidate_count_before_limit(),
        candidates,
    })
}

fn mcp_memory_occurrence(
    occurrence: &MemoryRecallOccurrence,
) -> Result<McpMemoryOccurrence, String> {
    Ok(McpMemoryOccurrence {
        path: RepositoryPathTextV1::encode(occurrence.path(), PATH_TEXT_LIMIT)
            .map_err(|error| error.to_string())?
            .into_string(),
        content_sha256: hex(occurrence.content_digest().as_bytes()),
        artifact_sha256: hex(occurrence.artifact_digest().as_bytes()),
        fact_ordinal: occurrence.fact_ordinal(),
        declaration_sha256: hex(occurrence.declaration_digest().as_bytes()),
        name_elided_sha256: hex(occurrence.name_elided_digest().as_bytes()),
    })
}

fn effective_state(state: MemoryEffectiveState) -> &'static str {
    match state {
        MemoryEffectiveState::Current => "current",
        MemoryEffectiveState::NotApplicable => "not_applicable",
        MemoryEffectiveState::Stale => "stale",
        MemoryEffectiveState::NeedsReview => "needs_review",
        MemoryEffectiveState::Indeterminate => "indeterminate",
        MemoryEffectiveState::Conflicted => "conflicted",
        MemoryEffectiveState::Contradicted => "contradicted",
        MemoryEffectiveState::Superseded => "superseded",
        MemoryEffectiveState::Quarantined => "quarantined",
        MemoryEffectiveState::Tombstoned => "tombstoned",
    }
}

fn validity_state(state: MemoryProjectionValidityState) -> &'static str {
    match state {
        MemoryProjectionValidityState::Valid => "valid",
        MemoryProjectionValidityState::Invalid => "invalid",
        MemoryProjectionValidityState::Indeterminate => "indeterminate",
        MemoryProjectionValidityState::NotEvaluated => "not_evaluated",
    }
}

fn evidence_state(state: MemoryRecallEvidenceState) -> &'static str {
    match state {
        MemoryRecallEvidenceState::Exact => "exact",
        MemoryRecallEvidenceState::Corresponded => "corresponded",
        MemoryRecallEvidenceState::Changed => "changed",
        MemoryRecallEvidenceState::Ambiguous => "ambiguous",
        MemoryRecallEvidenceState::Missing => "missing",
        MemoryRecallEvidenceState::Indeterminate => "indeterminate",
        MemoryRecallEvidenceState::Conflicted => "conflicted",
        MemoryRecallEvidenceState::NotEvaluated => "not_evaluated",
    }
}

fn memory_reason(reason: MemoryRecallReason) -> &'static str {
    match reason {
        MemoryRecallReason::EvidenceExact => "evidence_exact",
        MemoryRecallReason::EvidenceCorresponded => "evidence_corresponded",
        MemoryRecallReason::EvidenceChanged => "evidence_changed",
        MemoryRecallReason::EvidenceAmbiguous => "evidence_ambiguous",
        MemoryRecallReason::EvidenceMissing => "evidence_missing",
        MemoryRecallReason::EvidenceIndeterminate => "evidence_indeterminate",
        MemoryRecallReason::ProjectNotApplicable => "project_not_applicable",
        MemoryRecallReason::ProjectIndeterminate => "project_indeterminate",
        MemoryRecallReason::AuthoredNeedsReview => "authored_needs_review",
        MemoryRecallReason::AuthoredStale => "authored_stale",
        MemoryRecallReason::AuthoredContradicted => "authored_contradicted",
        MemoryRecallReason::AuthoredSuperseded => "authored_superseded",
        MemoryRecallReason::AuthoredQuarantined => "authored_quarantined",
        MemoryRecallReason::AuthoredTombstoned => "authored_tombstoned",
        MemoryRecallReason::ApprovedHeadConflict => "approved_head_conflict",
        MemoryRecallReason::MissingParent => "missing_parent",
        MemoryRecallReason::InvalidHeadGraph => "invalid_head_graph",
    }
}

fn evidence_outcome(outcome: MemoryRecallEvidenceOutcome) -> &'static str {
    match outcome {
        MemoryRecallEvidenceOutcome::Exact => "exact",
        MemoryRecallEvidenceOutcome::SamePathRename => "same_path_rename",
        MemoryRecallEvidenceOutcome::GitExactMove => "git_exact_move",
        MemoryRecallEvidenceOutcome::ReviewedLink => "reviewed_link",
        MemoryRecallEvidenceOutcome::Changed => "changed",
        MemoryRecallEvidenceOutcome::Ambiguous => "ambiguous",
        MemoryRecallEvidenceOutcome::Missing => "missing",
        MemoryRecallEvidenceOutcome::Indeterminate => "indeterminate",
    }
}

fn evidence_assurance(assurance: MemoryRecallEvidenceAssurance) -> &'static str {
    match assurance {
        MemoryRecallEvidenceAssurance::Automatic => "automatic",
        MemoryRecallEvidenceAssurance::Reviewed => "reviewed",
        MemoryRecallEvidenceAssurance::None => "none",
    }
}

fn candidate_relation(relation: MemoryRecallCandidateRelation) -> &'static str {
    match relation {
        MemoryRecallCandidateRelation::Same => "same",
        MemoryRecallCandidateRelation::Moved => "moved",
        MemoryRecallCandidateRelation::Renamed => "renamed",
        MemoryRecallCandidateRelation::MovedRenamed => "moved_renamed",
        MemoryRecallCandidateRelation::Split => "split",
        MemoryRecallCandidateRelation::Merged => "merged",
    }
}

fn memory_kind(kind: MemoryKind) -> &'static str {
    match kind {
        MemoryKind::Decision => "decision",
        MemoryKind::Failure => "failure",
    }
}

fn memory_assurance(assurance: MemoryAssurance) -> &'static str {
    match assurance {
        MemoryAssurance::LocallyApproved => "locally_approved",
    }
}

fn memory_lifecycle(lifecycle: MemoryLifecycle) -> &'static str {
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
