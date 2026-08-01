const PERSONAL_MEMORY_HELP: &str = concat!(
    "Append or read explicit local-only personal memory.\n\n",
    "Usage:\n",
    "  repowitness personal-memory append --repository-id <id> --database <path>\n",
    "      --profile <32 lowercase hex characters> --kind <kind> --title <text>\n",
    "      --body <text> --lifecycle <lifecycle>\n",
    "  repowitness personal-memory read --repository-id <id> --database <path>\n",
    "      --profile <32 lowercase hex characters> [--limit <1-100>]\n\n",
    "Kinds are fact, decision, procedure, episode, preference, policy, or failure.\n",
    "Lifecycles are active, needs_review, stale, contradicted, superseded, quarantined,\n",
    "or tombstoned. Personal records remain local to this profile and repository; they\n",
    "are never written to the worktree, Git, default diagnostics, or default MCP output.\n",
);

struct PersonalMemoryInvocation {
    database: PathBuf,
    repository: String,
    profile: PersonalMemoryProfileId,
    operation: PersonalMemoryOperation,
    kind: Option<PersonalMemoryKind>,
    title: Option<String>,
    body: Option<String>,
    lifecycle: Option<MemoryLifecycle>,
    limit: Option<u16>,
}

fn run_personal_memory(
    args: impl Iterator<Item = OsString>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
) -> u8 {
    let arguments: Vec<OsString> = args
        .take(MAX_PERSONAL_MEMORY_COMMAND_ARGUMENTS + 1)
        .collect();
    if arguments.len() > MAX_PERSONAL_MEMORY_COMMAND_ARGUMENTS {
        return emit_error(
            stderr,
            EXIT_USAGE,
            "error: personal-memory received too many arguments\n",
        );
    }
    if matches!(arguments.as_slice(), [help] if help == OsStr::new("--help") || help == OsStr::new("-h"))
        || matches!(arguments.as_slice(), [_, help] if help == OsStr::new("--help") || help == OsStr::new("-h"))
    {
        return emit_output(stdout, PERSONAL_MEMORY_HELP);
    }
    let Some((operation, values)) = arguments.split_first() else {
        return emit_error(
            stderr,
            EXIT_USAGE,
            "error: personal-memory requires append or read\n",
        );
    };
    let operation = match operation.to_str() {
        Some("append") => PersonalMemoryOperation::Append,
        Some("read") => PersonalMemoryOperation::Read,
        _ => {
            return emit_error(
                stderr,
                EXIT_USAGE,
                "error: unknown personal-memory command; use personal-memory --help\n",
            );
        }
    };
    let invocation = match parse_personal_memory_arguments(operation, values) {
        Ok(invocation) => invocation,
        Err(message) => return emit_error(stderr, EXIT_USAGE, message),
    };
    match invocation.operation {
        PersonalMemoryOperation::Append => run_personal_memory_append(invocation, stdout, stderr),
        PersonalMemoryOperation::Read => run_personal_memory_read(invocation, stdout, stderr),
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "the complete option grammar is intentionally centralized so duplicate and cross-operation options fail before local-store access"
)]
fn parse_personal_memory_arguments(
    operation: PersonalMemoryOperation,
    arguments: &[OsString],
) -> Result<PersonalMemoryInvocation, &'static str> {
    let mut database = None;
    let mut repository = None;
    let mut profile = None;
    let mut kind = None;
    let mut title = None;
    let mut body = None;
    let mut lifecycle = None;
    let mut limit = None;
    let mut index = 0_usize;
    while index < arguments.len() {
        let option = &arguments[index];
        let value = arguments
            .get(index + 1)
            .ok_or("error: personal-memory option requires a value\n")?;
        index += 2;
        let value = value
            .to_str()
            .ok_or("error: personal-memory arguments must be UTF-8\n")?;
        if option == OsStr::new("--database") && database.replace(PathBuf::from(value)).is_none() {
            continue;
        }
        if option == OsStr::new("--repository-id") && repository.replace(value.to_owned()).is_none()
        {
            continue;
        }
        if option == OsStr::new("--profile")
            && profile
                .replace(parse_personal_memory_profile(value).ok_or(
                    "error: personal-memory --profile must be 32 lowercase hex characters\n",
                )?)
                .is_none()
        {
            continue;
        }
        if option == OsStr::new("--kind")
            && kind
                .replace(
                    parse_personal_memory_kind(value)
                        .ok_or("error: personal-memory --kind is invalid\n")?,
                )
                .is_none()
        {
            continue;
        }
        if option == OsStr::new("--title") && title.replace(value.to_owned()).is_none() {
            continue;
        }
        if option == OsStr::new("--body") && body.replace(value.to_owned()).is_none() {
            continue;
        }
        if option == OsStr::new("--lifecycle")
            && lifecycle
                .replace(
                    parse_personal_memory_lifecycle(value)
                        .ok_or("error: personal-memory --lifecycle is invalid\n")?,
                )
                .is_none()
        {
            continue;
        }
        if option == OsStr::new("--limit")
            && limit
                .replace(
                    value
                        .parse::<u16>()
                        .ok()
                        .filter(|limit| (1..=100).contains(limit))
                        .ok_or("error: personal-memory --limit must be between 1 and 100\n")?,
                )
                .is_none()
        {
            continue;
        }
        return Err("error: personal-memory arguments are invalid\n");
    }
    let database = database
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or("error: personal-memory requires --database\n")?;
    let repository = repository
        .filter(|value| !value.is_empty())
        .ok_or("error: personal-memory requires --repository-id\n")?;
    let profile = profile.ok_or("error: personal-memory requires --profile\n")?;
    match operation {
        PersonalMemoryOperation::Read => {
            if kind.is_some() || title.is_some() || body.is_some() || lifecycle.is_some() {
                return Err(
                    "error: personal-memory read accepts only --repository-id, --database, --profile, and --limit\n",
                );
            }
        }
        PersonalMemoryOperation::Append => {
            if limit.is_some() {
                return Err("error: personal-memory append does not accept --limit\n");
            }
            if kind.is_none() || title.is_none() || body.is_none() || lifecycle.is_none() {
                return Err(
                    "error: personal-memory append requires --kind, --title, --body, and --lifecycle\n",
                );
            }
        }
    }
    Ok(PersonalMemoryInvocation {
        database,
        repository,
        profile,
        operation,
        kind,
        title,
        body,
        lifecycle,
        limit,
    })
}

fn run_personal_memory_append(
    invocation: PersonalMemoryInvocation,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
) -> u8 {
    let Some(recorded_at_unix_ms) = personal_memory_current_unix_ms() else {
        return emit_error(
            stderr,
            EXIT_SOFTWARE,
            "error: personal-memory clock is unavailable\n",
        );
    };
    let request = LocalPersonalMemoryAppendRequest::new(
        &invocation.database,
        &invocation.repository,
        invocation.profile,
        invocation.kind.expect("validated append kind"),
        invocation.title.as_deref().expect("validated append title"),
        invocation.body.as_deref().expect("validated append body"),
        invocation.lifecycle.expect("validated append lifecycle"),
        recorded_at_unix_ms,
    );
    match append_local_personal_memory(request, Arc::new(AtomicBool::new(false))) {
        Ok(record) => emit_output(
            stdout,
            &format!(
                "operation=personal-memory-append\nscope=personal\nrecord_id={}\nrevision_sha256={}\nkind={}\nlifecycle={}\n",
                personal_memory_hex(&record.record_id().as_bytes()),
                personal_memory_hex(&record.revision().as_bytes()),
                personal_memory_kind_name(record.kind()),
                personal_memory_lifecycle_name(record.lifecycle()),
            ),
        ),
        Err(_) => emit_error(
            stderr,
            EXIT_SOFTWARE,
            "error: personal-memory append failed\n",
        ),
    }
}

fn run_personal_memory_read(
    invocation: PersonalMemoryInvocation,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
) -> u8 {
    let request = LocalPersonalMemoryReadRequest::new(
        &invocation.database,
        &invocation.repository,
        invocation.profile,
        invocation.limit.unwrap_or(20),
    );
    match read_local_personal_memory(request, Arc::new(AtomicBool::new(false))) {
        Ok(records) => {
            let records = records
                .into_iter()
                .map(|record| {
                    serde_json::json!({
                        "record_id": personal_memory_hex(&record.record_id().as_bytes()),
                        "revision_sha256": personal_memory_hex(&record.revision().as_bytes()),
                        "kind": personal_memory_kind_name(record.kind()),
                        "title": record.title().as_str(),
                        "body": record.body().as_str(),
                        "lifecycle": personal_memory_lifecycle_name(record.lifecycle()),
                        "recorded_at_unix_ms": record.recorded_at_unix_ms(),
                    })
                })
                .collect::<Vec<_>>();
            match serde_json::to_string(&serde_json::json!({
                "operation": "personal-memory-read", "scope": "personal", "records": records,
            })) {
                Ok(output) => emit_output(stdout, &(output + "\n")),
                Err(_) => emit_error(
                    stderr,
                    EXIT_SOFTWARE,
                    "error: personal-memory read failed\n",
                ),
            }
        }
        Err(_) => emit_error(
            stderr,
            EXIT_SOFTWARE,
            "error: personal-memory read failed\n",
        ),
    }
}

fn parse_personal_memory_profile(value: &str) -> Option<PersonalMemoryProfileId> {
    if value.len() != 32
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return None;
    }
    let mut bytes = [0_u8; 16];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        bytes[index] =
            (personal_memory_hex_nibble(pair[0])? << 4) | personal_memory_hex_nibble(pair[1])?;
    }
    Some(PersonalMemoryProfileId::new(bytes))
}

const fn personal_memory_hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

fn parse_personal_memory_kind(value: &str) -> Option<PersonalMemoryKind> {
    match value {
        "fact" => Some(PersonalMemoryKind::Fact),
        "decision" => Some(PersonalMemoryKind::Decision),
        "procedure" => Some(PersonalMemoryKind::Procedure),
        "episode" => Some(PersonalMemoryKind::Episode),
        "preference" => Some(PersonalMemoryKind::Preference),
        "policy" => Some(PersonalMemoryKind::Policy),
        "failure" => Some(PersonalMemoryKind::Failure),
        _ => None,
    }
}

fn parse_personal_memory_lifecycle(value: &str) -> Option<MemoryLifecycle> {
    match value {
        "active" => Some(MemoryLifecycle::Active),
        "needs_review" => Some(MemoryLifecycle::NeedsReview),
        "stale" => Some(MemoryLifecycle::Stale),
        "contradicted" => Some(MemoryLifecycle::Contradicted),
        "superseded" => Some(MemoryLifecycle::Superseded),
        "quarantined" => Some(MemoryLifecycle::Quarantined),
        "tombstoned" => Some(MemoryLifecycle::Tombstoned),
        _ => None,
    }
}

const fn personal_memory_kind_name(kind: PersonalMemoryKind) -> &'static str {
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

const fn personal_memory_lifecycle_name(lifecycle: MemoryLifecycle) -> &'static str {
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

fn personal_memory_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(char::from(HEX[usize::from(byte >> 4)]));
        text.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    text
}

fn personal_memory_current_unix_ms() -> Option<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_millis()
        .try_into()
        .ok()
}

fn local_personal_memory_kind(kind: McpPersonalMemoryKind) -> PersonalMemoryKind {
    match kind {
        McpPersonalMemoryKind::Fact => PersonalMemoryKind::Fact,
        McpPersonalMemoryKind::Decision => PersonalMemoryKind::Decision,
        McpPersonalMemoryKind::Procedure => PersonalMemoryKind::Procedure,
        McpPersonalMemoryKind::Episode => PersonalMemoryKind::Episode,
        McpPersonalMemoryKind::Preference => PersonalMemoryKind::Preference,
        McpPersonalMemoryKind::Policy => PersonalMemoryKind::Policy,
        McpPersonalMemoryKind::Failure => PersonalMemoryKind::Failure,
    }
}

fn local_personal_memory_lifecycle(lifecycle: McpPersonalMemoryLifecycle) -> MemoryLifecycle {
    match lifecycle {
        McpPersonalMemoryLifecycle::Active => MemoryLifecycle::Active,
        McpPersonalMemoryLifecycle::NeedsReview => MemoryLifecycle::NeedsReview,
        McpPersonalMemoryLifecycle::Stale => MemoryLifecycle::Stale,
        McpPersonalMemoryLifecycle::Contradicted => MemoryLifecycle::Contradicted,
        McpPersonalMemoryLifecycle::Superseded => MemoryLifecycle::Superseded,
        McpPersonalMemoryLifecycle::Quarantined => MemoryLifecycle::Quarantined,
        McpPersonalMemoryLifecycle::Tombstoned => MemoryLifecycle::Tombstoned,
    }
}

fn personal_memory_mcp_output(
    operation: PersonalMemoryOperation,
    records: Vec<repowitness_local::PersonalMemoryRecord>,
) -> PersonalMemoryOutput {
    let include_content = operation == PersonalMemoryOperation::Read;
    let records = records
        .into_iter()
        .map(|record| PersonalMemoryRecordOutput {
            record_id: personal_memory_hex(&record.record_id().as_bytes()),
            revision_sha256: personal_memory_hex(&record.revision().as_bytes()),
            kind: match record.kind() {
                PersonalMemoryKind::Fact => McpPersonalMemoryKind::Fact,
                PersonalMemoryKind::Decision => McpPersonalMemoryKind::Decision,
                PersonalMemoryKind::Procedure => McpPersonalMemoryKind::Procedure,
                PersonalMemoryKind::Episode => McpPersonalMemoryKind::Episode,
                PersonalMemoryKind::Preference => McpPersonalMemoryKind::Preference,
                PersonalMemoryKind::Policy => McpPersonalMemoryKind::Policy,
                PersonalMemoryKind::Failure => McpPersonalMemoryKind::Failure,
            },
            title: include_content.then(|| record.title().as_str().to_owned()),
            body: include_content.then(|| record.body().as_str().to_owned()),
            lifecycle: match record.lifecycle() {
                MemoryLifecycle::Active => McpPersonalMemoryLifecycle::Active,
                MemoryLifecycle::NeedsReview => McpPersonalMemoryLifecycle::NeedsReview,
                MemoryLifecycle::Stale => McpPersonalMemoryLifecycle::Stale,
                MemoryLifecycle::Contradicted => McpPersonalMemoryLifecycle::Contradicted,
                MemoryLifecycle::Superseded => McpPersonalMemoryLifecycle::Superseded,
                MemoryLifecycle::Quarantined => McpPersonalMemoryLifecycle::Quarantined,
                MemoryLifecycle::Tombstoned => McpPersonalMemoryLifecycle::Tombstoned,
            },
            recorded_at_unix_ms: record.recorded_at_unix_ms(),
        })
        .collect();
    PersonalMemoryOutput {
        schema_version: 1,
        scope: "personal".to_owned(),
        operation,
        records,
    }
}
