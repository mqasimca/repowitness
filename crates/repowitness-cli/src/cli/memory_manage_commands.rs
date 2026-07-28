fn run_memory_manage(
    args: impl Iterator<Item = OsString>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
    memory: &impl RepositoryMemory,
) -> u8 {
    let arguments: Vec<OsString> = args.take(MAX_MEMORY_MANAGE_ARGUMENTS + 1).collect();
    if arguments.len() > MAX_MEMORY_MANAGE_ARGUMENTS {
        return emit_error(
            stderr,
            EXIT_USAGE,
            "error: memory-manage received too many arguments; use memory-manage --help\n",
        );
    }
    if matches!(arguments.as_slice(), [help] if help == OsStr::new("--help") || help == OsStr::new("-h"))
    {
        return emit_output(stdout, MEMORY_MANAGE_HELP);
    }
    let invocation = match parse_memory_manage_arguments(&arguments) {
        Ok(invocation) => invocation,
        Err(message) => return emit_error(stderr, EXIT_USAGE, message),
    };
    match memory.manage(&invocation) {
        Ok(report) => emit_memory_manage_report(stdout, stderr, report),
        Err(_) => emit_error(stderr, EXIT_SOFTWARE, "error: memory management failed\n"),
    }
}

fn parse_memory_manage_arguments(
    arguments: &[OsString],
) -> Result<MemoryManageInvocation, &'static str> {
    let (operation, remaining) = arguments
        .split_first()
        .ok_or("error: memory-manage requires an operation; use memory-manage --help\n")?;
    if operation == OsStr::new("write") {
        parse_memory_write_arguments(remaining)
    } else if operation == OsStr::new("approve") {
        parse_memory_approve_arguments(remaining)
    } else if operation == OsStr::new("review") {
        parse_memory_review_arguments(remaining)
    } else if operation == OsStr::new("import-history") {
        parse_memory_history_arguments(remaining)
    } else {
        Err("error: unknown memory-manage operation; use memory-manage --help\n")
    }
}

fn parse_memory_write_arguments(
    arguments: &[OsString],
) -> Result<MemoryManageInvocation, &'static str> {
    let parsed = parse_manage_options(arguments, ManageOptionSet::Write)?;
    Ok(MemoryManageInvocation::Write {
        repository_root: required_root(parsed.repository_root)?,
        repository_identity: required_manage_value(
            parsed.repository_identity,
            "error: memory-manage write requires --repository-id\n",
        )?,
        input: parsed
            .input
            .filter(|path| !path.as_os_str().is_empty())
            .ok_or("error: memory-manage write requires --input\n")?,
    })
}

fn parse_memory_approve_arguments(
    arguments: &[OsString],
) -> Result<MemoryManageInvocation, &'static str> {
    let parsed = parse_manage_options(arguments, ManageOptionSet::Approve)?;
    Ok(MemoryManageInvocation::Approve {
        repository_root: required_root(parsed.repository_root)?,
        database: required_database(parsed.database)?,
        repository_identity: required_manage_value(
            parsed.repository_identity,
            "error: memory-manage approve requires --repository-id\n",
        )?,
        record_id: required_manage_value(
            parsed.record_id,
            "error: memory-manage approve requires --record-id\n",
        )?,
        actor: required_manage_value(
            parsed.actor,
            "error: memory-manage approve requires --actor\n",
        )?,
    })
}

fn parse_memory_review_arguments(
    arguments: &[OsString],
) -> Result<MemoryManageInvocation, &'static str> {
    let parsed = parse_manage_options(arguments, ManageOptionSet::Review)?;
    Ok(MemoryManageInvocation::Review {
        repository_root: required_root(parsed.repository_root)?,
        database: required_database(parsed.database)?,
        repository_identity: required_manage_value(
            parsed.repository_identity,
            "error: memory-manage review requires --repository-id\n",
        )?,
        record_id: required_manage_value(
            parsed.record_id,
            "error: memory-manage review requires --record-id\n",
        )?,
        revision: required_manage_value(
            parsed.revision,
            "error: memory-manage review requires --revision\n",
        )?,
        evidence_ordinal: parsed
            .evidence_ordinal
            .ok_or("error: memory-manage review requires --evidence\n")?,
        operation: parsed
            .review_operation
            .ok_or("error: memory-manage review requires --operation\n")?,
        target_path: required_manage_value(
            parsed.target_path,
            "error: memory-manage review requires --target-path\n",
        )?,
        target_artifact: required_manage_value(
            parsed.target_artifact,
            "error: memory-manage review requires --target-artifact\n",
        )?,
        target_fact_ordinal: parsed
            .target_fact_ordinal
            .ok_or("error: memory-manage review requires --target-fact\n")?,
        actor: required_manage_value(
            parsed.actor,
            "error: memory-manage review requires --actor\n",
        )?,
    })
}

fn parse_memory_history_arguments(
    arguments: &[OsString],
) -> Result<MemoryManageInvocation, &'static str> {
    let parsed = parse_manage_options(arguments, ManageOptionSet::ImportHistory)?;
    Ok(MemoryManageInvocation::ImportHistory {
        repository_root: required_root(parsed.repository_root)?,
        database: required_database(parsed.database)?,
        repository_identity: required_manage_value(
            parsed.repository_identity,
            "error: memory-manage import-history requires --repository-id\n",
        )?,
        actor: required_manage_value(
            parsed.actor,
            "error: memory-manage import-history requires --actor\n",
        )?,
    })
}

#[derive(Clone, Copy)]
enum ManageOptionSet {
    Write,
    Approve,
    Review,
    ImportHistory,
}

#[derive(Default)]
struct ParsedManageOptions {
    repository_root: Option<PathBuf>,
    repository_identity: Option<OsString>,
    database: Option<PathBuf>,
    input: Option<PathBuf>,
    record_id: Option<OsString>,
    actor: Option<OsString>,
    revision: Option<OsString>,
    evidence_ordinal: Option<u8>,
    review_operation: Option<MemoryCorrespondenceReviewOperation>,
    target_path: Option<OsString>,
    target_artifact: Option<OsString>,
    target_fact_ordinal: Option<u64>,
}

fn parse_manage_options(
    arguments: &[OsString],
    set: ManageOptionSet,
) -> Result<ParsedManageOptions, &'static str> {
    let mut parsed = ParsedManageOptions::default();
    let mut positional_only = false;
    let mut index = 0_usize;
    while index < arguments.len() {
        let argument = &arguments[index];
        index += 1;
        if positional_only {
            set_manage_root(&mut parsed.repository_root, argument)?;
            continue;
        }
        if argument == OsStr::new("--") {
            positional_only = true;
            continue;
        }
        if argument == OsStr::new("--help") || argument == OsStr::new("-h") {
            return Err("error: memory-manage --help accepts no additional arguments\n");
        }
        if !os_string_starts_with_hyphen(argument) {
            set_manage_root(&mut parsed.repository_root, argument)?;
            continue;
        }
        let value = arguments
            .get(index)
            .ok_or("error: memory-manage option requires a value\n")?;
        index += 1;
        assign_manage_option(&mut parsed, set, argument, value)?;
    }
    Ok(parsed)
}

fn assign_manage_option(
    parsed: &mut ParsedManageOptions,
    set: ManageOptionSet,
    option: &OsStr,
    value: &OsStr,
) -> Result<(), &'static str> {
    if option == OsStr::new("--repository-id") {
        set_manage_once(
            &mut parsed.repository_identity,
            value.to_os_string(),
            "error: memory-manage accepts --repository-id only once\n",
        )
    } else if option == OsStr::new("--database")
        && matches!(
            set,
            ManageOptionSet::Approve | ManageOptionSet::Review | ManageOptionSet::ImportHistory
        )
    {
        set_manage_once(
            &mut parsed.database,
            PathBuf::from(value),
            "error: memory-manage accepts --database only once\n",
        )
    } else if option == OsStr::new("--input") && matches!(set, ManageOptionSet::Write) {
        set_manage_once(
            &mut parsed.input,
            PathBuf::from(value),
            "error: memory-manage write accepts --input only once\n",
        )
    } else if option == OsStr::new("--record-id")
        && matches!(set, ManageOptionSet::Approve | ManageOptionSet::Review)
    {
        set_manage_once(
            &mut parsed.record_id,
            value.to_os_string(),
            "error: memory-manage approve accepts --record-id only once\n",
        )
    } else if option == OsStr::new("--actor")
        && matches!(
            set,
            ManageOptionSet::Approve | ManageOptionSet::Review | ManageOptionSet::ImportHistory
        )
    {
        set_manage_once(
            &mut parsed.actor,
            value.to_os_string(),
            "error: memory-manage accepts --actor only once\n",
        )
    } else if option == OsStr::new("--revision") && matches!(set, ManageOptionSet::Review) {
        set_manage_once(
            &mut parsed.revision,
            value.to_os_string(),
            "error: memory-manage review accepts --revision only once\n",
        )
    } else if option == OsStr::new("--evidence") && matches!(set, ManageOptionSet::Review) {
        let evidence = parse_manage_evidence_ordinal(value)?;
        set_manage_once(
            &mut parsed.evidence_ordinal,
            evidence,
            "error: memory-manage review accepts --evidence only once\n",
        )
    } else if option == OsStr::new("--operation") && matches!(set, ManageOptionSet::Review) {
        let operation = parse_review_operation(value)?;
        set_manage_once(
            &mut parsed.review_operation,
            operation,
            "error: memory-manage review accepts --operation only once\n",
        )
    } else if option == OsStr::new("--target-path") && matches!(set, ManageOptionSet::Review) {
        set_manage_once(
            &mut parsed.target_path,
            value.to_os_string(),
            "error: memory-manage review accepts --target-path only once\n",
        )
    } else if option == OsStr::new("--target-artifact") && matches!(set, ManageOptionSet::Review) {
        set_manage_once(
            &mut parsed.target_artifact,
            value.to_os_string(),
            "error: memory-manage review accepts --target-artifact only once\n",
        )
    } else if option == OsStr::new("--target-fact") && matches!(set, ManageOptionSet::Review) {
        let fact = parse_manage_target_fact(value)?;
        set_manage_once(
            &mut parsed.target_fact_ordinal,
            fact,
            "error: memory-manage review accepts --target-fact only once\n",
        )
    } else {
        Err("error: option is not valid for this memory-manage operation\n")
    }
}

fn parse_manage_evidence_ordinal(value: &OsStr) -> Result<u8, &'static str> {
    value
        .to_str()
        .and_then(|text| text.parse::<u8>().ok())
        .filter(|ordinal| *ordinal < 16)
        .ok_or("error: memory-manage review --evidence must be an integer from 0 to 15\n")
}

fn parse_manage_target_fact(value: &OsStr) -> Result<u64, &'static str> {
    value
        .to_str()
        .and_then(|text| text.parse::<u64>().ok())
        .filter(|ordinal| *ordinal <= MAX_MCP_INTEROPERABLE_INTEGER)
        .ok_or(
            "error: memory-manage review --target-fact must be an integer from 0 to 9007199254740991\n",
        )
}

fn parse_review_operation(
    value: &OsStr,
) -> Result<MemoryCorrespondenceReviewOperation, &'static str> {
    if value == OsStr::new("approve") {
        Ok(MemoryCorrespondenceReviewOperation::Approved)
    } else if value == OsStr::new("reject") {
        Ok(MemoryCorrespondenceReviewOperation::Rejected)
    } else if value == OsStr::new("manual-link") {
        Ok(MemoryCorrespondenceReviewOperation::ManualLink)
    } else {
        Err("error: memory-manage review --operation must be approve, reject, or manual-link\n")
    }
}

fn set_manage_once<T>(
    target: &mut Option<T>,
    value: T,
    duplicate: &'static str,
) -> Result<(), &'static str> {
    if target.replace(value).is_some() {
        Err(duplicate)
    } else {
        Ok(())
    }
}

fn set_manage_root(root: &mut Option<PathBuf>, value: &OsStr) -> Result<(), &'static str> {
    if value.is_empty() {
        return Err("error: memory-manage repository must not be empty\n");
    }
    set_manage_once(
        root,
        PathBuf::from(value),
        "error: memory-manage accepts exactly one repository\n",
    )
}

fn required_root(root: Option<PathBuf>) -> Result<PathBuf, &'static str> {
    root.ok_or("error: memory-manage requires one repository\n")
}

fn required_database(database: Option<PathBuf>) -> Result<PathBuf, &'static str> {
    database
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or("error: memory-manage operation requires --database\n")
}

fn required_manage_value(
    value: Option<OsString>,
    missing: &'static str,
) -> Result<OsString, &'static str> {
    value.filter(|value| !value.is_empty()).ok_or(missing)
}

fn emit_memory_manage_report(
    writer: &mut impl Write,
    stderr: &mut impl Write,
    report: CliMemoryManageReport,
) -> u8 {
    if !memory_manage_report_is_valid(&report) {
        return emit_error(stderr, EXIT_SOFTWARE, "error: memory management failed\n");
    }
    let result = match report {
        CliMemoryManageReport::Write {
            revision,
            created,
            canonical_bytes,
        } => writeln!(
            writer,
            "{{\"schema_version\":1,\"operation\":\"write\",\"revision_sha256\":\"{revision}\",\"created\":{created},\"canonical_bytes\":{canonical_bytes}}}"
        ),
        CliMemoryManageReport::Approve {
            revision,
            version_inserted,
            observation_inserted,
            approval_inserted,
        } => writeln!(
            writer,
            "{{\"schema_version\":1,\"operation\":\"approve\",\"revision_sha256\":\"{revision}\",\"version_inserted\":{version_inserted},\"observation_inserted\":{observation_inserted},\"approval_inserted\":{approval_inserted}}}"
        ),
        CliMemoryManageReport::Review { inserted } => writeln!(
            writer,
            "{{\"schema_version\":1,\"operation\":\"review\",\"inserted\":{inserted}}}"
        ),
        CliMemoryManageReport::ImportHistory {
            commits_inspected,
            records_inspected,
            imported_versions,
            appended_observations,
            total_record_bytes,
            git_processes,
            history_complete,
        } => writeln!(
            writer,
            "{{\"schema_version\":1,\"operation\":\"import_history\",\"commits_inspected\":{commits_inspected},\"records_inspected\":{records_inspected},\"imported_versions\":{imported_versions},\"appended_observations\":{appended_observations},\"total_record_bytes\":{total_record_bytes},\"git_processes\":{git_processes},\"history_complete\":{history_complete}}}"
        ),
    };
    if result.is_ok() {
        EXIT_SUCCESS
    } else {
        EXIT_IO
    }
}

fn memory_manage_report_is_valid(report: &CliMemoryManageReport) -> bool {
    match report {
        CliMemoryManageReport::Write { revision, .. }
        | CliMemoryManageReport::Approve { revision, .. } => {
            revision.len() == 64
                && revision
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        }
        CliMemoryManageReport::Review { .. } | CliMemoryManageReport::ImportHistory { .. } => true,
    }
}
