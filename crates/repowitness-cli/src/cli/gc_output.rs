const MAX_CLI_GC_OUTPUT_BYTES: usize = 16 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
enum CliGcReport {
    Plan(CliGcPlanReport),
    Apply(CliGcApplyReport),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CliGcPolicy {
    configuration_sha256: String,
    policy_sha256: String,
    retained_generations_per_source_slot: u16,
    max_generation_candidates: u64,
    max_rows: u64,
    max_bytes: u64,
    generation_pin_count: u64,
    workspace_view_pin_count: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CliGcPlanReport {
    policy: CliGcPolicy,
    plan_sha256: String,
    candidate_count: u64,
    estimated_rows: u64,
    estimated_bytes: u64,
    root_count: u64,
    unresolved_count: u64,
    unresolved_truncated: bool,
    logical_work_rows: u64,
    more_work: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CliGcApplyReport {
    policy: CliGcPolicy,
    plan_sha256: String,
    collection_id: u64,
    generation_count: u64,
    workspace_view_count: u64,
    source_slot_receipt_count: u64,
    snapshot_count: u64,
    artifact_count: u64,
    deleted_rows: u64,
    estimated_deleted_bytes: u64,
    more_work: bool,
    shutdown_complete: bool,
    database_identity_confirmed: bool,
}

impl CliGcReport {
    fn from_local_plan(report: LocalRetentionPlanReport) -> Self {
        Self::Plan(CliGcPlanReport {
            policy: CliGcPolicy::from_local(
                report.configuration_digest(),
                report.policy_digest(),
                report.policy(),
            ),
            plan_sha256: hex(&report.plan_digest()),
            candidate_count: report.candidate_count(),
            estimated_rows: report.estimated_rows(),
            estimated_bytes: report.estimated_bytes(),
            root_count: report.root_count(),
            unresolved_count: report.unresolved_count(),
            unresolved_truncated: report.unresolved_truncated(),
            logical_work_rows: report.logical_work_rows(),
            more_work: report.more_work(),
        })
    }

    fn from_local_apply(report: LocalRetentionApplyReport) -> Self {
        Self::Apply(CliGcApplyReport {
            policy: CliGcPolicy::from_local(
                report.configuration_digest(),
                report.policy_digest(),
                report.policy(),
            ),
            plan_sha256: hex(&report.plan_digest()),
            collection_id: report.collection_id(),
            generation_count: report.generation_count(),
            workspace_view_count: report.workspace_view_count(),
            source_slot_receipt_count: report.source_slot_receipt_count(),
            snapshot_count: report.snapshot_count(),
            artifact_count: report.artifact_count(),
            deleted_rows: report.deleted_rows(),
            estimated_deleted_bytes: report.estimated_deleted_bytes(),
            more_work: report.more_work(),
            shutdown_complete: report.shutdown_complete(),
            database_identity_confirmed: report.database_identity_confirmed(),
        })
    }
}

impl CliGcPolicy {
    fn from_local(
        configuration_digest: repowitness_local::ConfigurationDigest,
        policy_digest: [u8; 32],
        policy: repowitness_local::LocalRetentionPolicySummary,
    ) -> Self {
        Self {
            configuration_sha256: hex(configuration_digest.as_bytes()),
            policy_sha256: hex(&policy_digest),
            retained_generations_per_source_slot: policy.retained_generations_per_source_slot(),
            max_generation_candidates: policy.max_generation_candidates(),
            max_rows: policy.max_rows(),
            max_bytes: policy.max_bytes(),
            generation_pin_count: policy.generation_pin_count(),
            workspace_view_pin_count: policy.workspace_view_pin_count(),
        }
    }
}

fn emit_gc_report(writer: &mut impl Write, report: &CliGcReport) -> u8 {
    if !gc_report_is_consistent(report) {
        return EXIT_SOFTWARE;
    }
    let mut output = Vec::with_capacity(2 * 1024);
    let result = match report {
        CliGcReport::Plan(report) => write_gc_plan_report(&mut output, report),
        CliGcReport::Apply(report) => write_gc_apply_report(&mut output, report),
    };
    if result.is_err() || output.len() > MAX_CLI_GC_OUTPUT_BYTES {
        return EXIT_SOFTWARE;
    }
    if writer.write_all(&output).is_ok() {
        EXIT_SUCCESS
    } else {
        EXIT_IO
    }
}

fn write_gc_plan_report(writer: &mut impl Write, report: &CliGcPlanReport) -> std::io::Result<()> {
    writeln!(writer, "status=ok")?;
    writeln!(writer, "operation=gc_plan")?;
    write_gc_policy(writer, &report.policy)?;
    writeln!(writer, "retention_plan_sha256={}", report.plan_sha256)?;
    writeln!(writer, "candidate_count={}", report.candidate_count)?;
    writeln!(writer, "estimated_rows={}", report.estimated_rows)?;
    writeln!(writer, "estimated_bytes={}", report.estimated_bytes)?;
    writeln!(writer, "root_count={}", report.root_count)?;
    writeln!(
        writer,
        "unresolved_candidate_count={}",
        report.unresolved_count
    )?;
    writeln!(
        writer,
        "unresolved_candidates_truncated={}",
        report.unresolved_truncated
    )?;
    writeln!(writer, "logical_work_rows={}", report.logical_work_rows)?;
    writeln!(writer, "more_work={}", report.more_work)
}

fn write_gc_apply_report(
    writer: &mut impl Write,
    report: &CliGcApplyReport,
) -> std::io::Result<()> {
    let warning_count =
        u8::from(!report.shutdown_complete) + u8::from(!report.database_identity_confirmed);
    writeln!(
        writer,
        "status={}",
        if warning_count == 0 { "ok" } else { "warning" }
    )?;
    writeln!(writer, "operation=gc_apply")?;
    write_gc_policy(writer, &report.policy)?;
    writeln!(writer, "retention_plan_sha256={}", report.plan_sha256)?;
    writeln!(writer, "collection_id={}", report.collection_id)?;
    writeln!(writer, "deleted_generations={}", report.generation_count)?;
    writeln!(
        writer,
        "deleted_workspace_views={}",
        report.workspace_view_count
    )?;
    writeln!(
        writer,
        "deleted_source_slot_receipts={}",
        report.source_slot_receipt_count
    )?;
    writeln!(writer, "deleted_snapshots={}", report.snapshot_count)?;
    writeln!(writer, "deleted_artifacts={}", report.artifact_count)?;
    writeln!(writer, "deleted_rows={}", report.deleted_rows)?;
    writeln!(
        writer,
        "estimated_deleted_bytes={}",
        report.estimated_deleted_bytes
    )?;
    writeln!(writer, "more_work={}", report.more_work)?;
    writeln!(
        writer,
        "maintenance_shutdown={}",
        if report.shutdown_complete {
            "complete"
        } else {
            "incomplete"
        }
    )?;
    writeln!(
        writer,
        "database_identity_fence={}",
        if report.database_identity_confirmed {
            "confirmed"
        } else {
            "changed"
        }
    )?;
    writeln!(writer, "warning_count={warning_count}")?;
    let mut warning_index = 0_u8;
    if !report.shutdown_complete {
        writeln!(
            writer,
            "warning_{warning_index}=committed_apply_shutdown_incomplete"
        )?;
        warning_index += 1;
    }
    if !report.database_identity_confirmed {
        writeln!(
            writer,
            "warning_{warning_index}=committed_apply_database_identity_changed"
        )?;
    }
    Ok(())
}

fn write_gc_policy(writer: &mut impl Write, policy: &CliGcPolicy) -> std::io::Result<()> {
    writeln!(writer, "schema_version=1")?;
    writeln!(
        writer,
        "retention_profile={}",
        repowitness_local::LOCAL_RETENTION_PROFILE_VERSION
    )?;
    writeln!(
        writer,
        "configuration_sha256={}",
        policy.configuration_sha256
    )?;
    writeln!(writer, "retention_policy_sha256={}", policy.policy_sha256)?;
    writeln!(
        writer,
        "retained_generations_per_source_slot={}",
        policy.retained_generations_per_source_slot
    )?;
    writeln!(
        writer,
        "max_generation_candidates={}",
        policy.max_generation_candidates
    )?;
    writeln!(writer, "max_rows={}", policy.max_rows)?;
    writeln!(writer, "max_bytes={}", policy.max_bytes)?;
    writeln!(
        writer,
        "generation_pin_count={}",
        policy.generation_pin_count
    )?;
    writeln!(
        writer,
        "workspace_view_pin_count={}",
        policy.workspace_view_pin_count
    )
}

fn gc_report_is_consistent(report: &CliGcReport) -> bool {
    let (policy, plan_sha256) = match report {
        CliGcReport::Plan(report) => {
            if report.candidate_count > report.policy.max_generation_candidates
                || report.estimated_rows > report.policy.max_rows
                || report.estimated_bytes > report.policy.max_bytes
                || report.logical_work_rows > report.policy.max_rows
                || report.root_count > report.logical_work_rows
                || ((report.unresolved_count > 0 || report.unresolved_truncated)
                    && !report.more_work)
            {
                return false;
            }
            (&report.policy, report.plan_sha256.as_str())
        }
        CliGcReport::Apply(report) => {
            if report.generation_count > report.policy.max_generation_candidates {
                return false;
            }
            (&report.policy, report.plan_sha256.as_str())
        }
    };
    valid_lower_hex_digest(&policy.configuration_sha256)
        && valid_lower_hex_digest(&policy.policy_sha256)
        && valid_lower_hex_digest(plan_sha256)
        && policy.retained_generations_per_source_slot > 0
        && policy.max_generation_candidates > 0
        && policy.max_rows > 0
        && policy.max_bytes > 0
}

fn valid_lower_hex_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
