fn run_task_status(
    args: impl Iterator<Item = OsString>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
) -> u8 {
    let arguments: Vec<OsString> = args.take(MAX_TASK_STATUS_ARGUMENTS + 1).collect();
    if arguments.len() > MAX_TASK_STATUS_ARGUMENTS {
        return emit_error(
            stderr,
            EXIT_USAGE,
            "error: task-status received too many arguments\n",
        );
    }
    if matches!(arguments.as_slice(), [help] if help == OsStr::new("--help") || help == OsStr::new("-h"))
    {
        return emit_output(
            stdout,
            "Usage: repowitness task-status --repository-id <id> --database <path> --task-id <32 lowercase hex characters>\n",
        );
    }
    let mut repository = None;
    let mut database = None;
    let mut task = None;
    let mut index = 0_usize;
    while index < arguments.len() {
        let option = &arguments[index];
        let value = arguments.get(index + 1).ok_or(());
        let Ok(value) = value else {
            return emit_error(
                stderr,
                EXIT_USAGE,
                "error: task-status option requires a value\n",
            );
        };
        index += 2;
        if option == OsStr::new("--repository-id") && repository.replace(value.clone()).is_none() {
            continue;
        }
        if option == OsStr::new("--database") && database.replace(PathBuf::from(value)).is_none() {
            continue;
        }
        if option == OsStr::new("--task-id") && task.replace(value.clone()).is_none() {
            continue;
        }
        return emit_error(
            stderr,
            EXIT_USAGE,
            "error: task-status arguments are invalid\n",
        );
    }
    let Some(repository) = repository.and_then(|value| value.into_string().ok()) else {
        return emit_error(
            stderr,
            EXIT_USAGE,
            "error: task-status requires --repository-id\n",
        );
    };
    let Some(database) = database.filter(|value| !value.as_os_str().is_empty()) else {
        return emit_error(
            stderr,
            EXIT_USAGE,
            "error: task-status requires --database\n",
        );
    };
    let Some(task) = task
        .and_then(|value| value.into_string().ok())
        .and_then(parse_task_id)
    else {
        return emit_error(
            stderr,
            EXIT_USAGE,
            "error: task-status requires canonical --task-id\n",
        );
    };
    match poll_local_task(
        LocalTaskPollRequest::new(&database, &repository, task),
        Arc::new(AtomicBool::new(false)),
    ) {
        Ok(None) => emit_output(stdout, "operation=task-status\nstatus=not_found\n"),
        Ok(Some(status)) => emit_task_status(stdout, status),
        Err(_) => emit_error(stderr, EXIT_SOFTWARE, "error: task-status failed\n"),
    }
}

fn parse_task_id(text: String) -> Option<TaskId> {
    if text.len() != 32
        || !text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return None;
    }
    let mut bytes = [0_u8; 16];
    for (index, pair) in text.as_bytes().chunks_exact(2).enumerate() {
        let high = task_hex_nibble(pair[0])?;
        let low = task_hex_nibble(pair[1])?;
        bytes[index] = (high << 4) | low;
    }
    Some(TaskId::new(bytes))
}

const fn task_hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

fn emit_task_status(stdout: &mut impl Write, status: TaskStatus) -> u8 {
    let state = match status.state() {
        TaskState::Open => "open",
        TaskState::Blocked => "blocked",
        TaskState::Completed => "completed",
        TaskState::Cancelled => "cancelled",
    };
    let report = format!(
        "operation=task-status\nstatus=found\nstate={state}\ncheckpoint_sequence={}\nverification_count={}\n",
        status.checkpoint_sequence(),
        status.verification_count(),
    );
    emit_output(stdout, &report)
}

const TASK_HELP: &str = concat!(
    "Create or append an immutable bounded durable-task checkpoint.\n\n",
    "Usage:\n",
    "  repowitness task create --repository-id <id> --database <path> --state <state>\n",
    "      --objective <text> [--hypothesis <text>] [--next-safe-action <text>]\n",
    "  repowitness task checkpoint --repository-id <id> --database <path>\n",
    "      --task-id <32 lowercase hex characters> --state <state> --objective <text>\n",
    "      [--hypothesis <text>] [--next-safe-action <text>]\n\n",
    "States are open, blocked, completed, or cancelled. Checkpoint text is bounded\n",
    "and secret-scanned before persistence. Create prints one opaque task ID;\n",
    "checkpoint appends the next immutable sequence and rejects concurrent conflicts.\n",
);

struct TaskCheckpointInvocation {
    database: PathBuf,
    repository: String,
    task_id: Option<TaskId>,
    state: TaskState,
    objective: String,
    hypothesis: Option<String>,
    next_safe_action: Option<String>,
}

fn run_task(
    args: impl Iterator<Item = OsString>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
) -> u8 {
    let arguments: Vec<OsString> = args.take(MAX_TASK_COMMAND_ARGUMENTS + 1).collect();
    if arguments.len() > MAX_TASK_COMMAND_ARGUMENTS {
        return emit_error(
            stderr,
            EXIT_USAGE,
            "error: task received too many arguments\n",
        );
    }
    if matches!(arguments.as_slice(), [help] if help == OsStr::new("--help") || help == OsStr::new("-h"))
        || matches!(arguments.as_slice(), [_, help] if help == OsStr::new("--help") || help == OsStr::new("-h"))
    {
        return emit_output(stdout, TASK_HELP);
    }
    let Some((operation, values)) = arguments.split_first() else {
        return emit_error(
            stderr,
            EXIT_USAGE,
            "error: task requires create or checkpoint\n",
        );
    };
    let create = operation == OsStr::new("create");
    if !create && operation != OsStr::new("checkpoint") {
        return emit_error(
            stderr,
            EXIT_USAGE,
            "error: unknown task command; use task --help\n",
        );
    }
    let invocation = match parse_task_checkpoint_arguments(values, create) {
        Ok(invocation) => invocation,
        Err(message) => return emit_error(stderr, EXIT_USAGE, message),
    };
    let Some(recorded_at_unix_ms) = task_current_unix_ms() else {
        return emit_error(stderr, EXIT_SOFTWARE, "error: task clock is unavailable\n");
    };
    let request = match invocation.task_id {
        None => LocalTaskCheckpointRequest::create(
            &invocation.database,
            &invocation.repository,
            invocation.state,
            &invocation.objective,
            invocation.hypothesis.as_deref(),
            invocation.next_safe_action.as_deref(),
            recorded_at_unix_ms,
        ),
        Some(task_id) => LocalTaskCheckpointRequest::update(
            &invocation.database,
            &invocation.repository,
            task_id,
            invocation.state,
            &invocation.objective,
            invocation.hypothesis.as_deref(),
            invocation.next_safe_action.as_deref(),
            recorded_at_unix_ms,
        ),
    };
    match append_local_task_checkpoint(request, Arc::new(AtomicBool::new(false))) {
        Ok(receipt) => {
            emit_task_checkpoint_receipt(stdout, operation, receipt.task_id(), receipt.sequence())
        }
        Err(_) => emit_error(stderr, EXIT_SOFTWARE, "error: task checkpoint failed\n"),
    }
}

fn parse_task_checkpoint_arguments(
    arguments: &[OsString],
    create: bool,
) -> Result<TaskCheckpointInvocation, &'static str> {
    let mut repository = None;
    let mut database = None;
    let mut task_id = None;
    let mut state = None;
    let mut objective = None;
    let mut hypothesis = None;
    let mut next_safe_action = None;
    let mut index = 0_usize;
    while index < arguments.len() {
        let option = &arguments[index];
        let Some(value) = arguments.get(index + 1) else {
            return Err("error: task option requires a value\n");
        };
        index += 2;
        let Some(value) = value.to_str() else {
            return Err("error: task arguments must be UTF-8\n");
        };
        if option == OsStr::new("--repository-id") && repository.replace(value.to_owned()).is_none()
        {
            continue;
        }
        if option == OsStr::new("--database") && database.replace(PathBuf::from(value)).is_none() {
            continue;
        }
        if option == OsStr::new("--task-id")
            && task_id
                .replace(
                    parse_task_id(value.to_owned())
                        .ok_or("error: task --task-id must be canonical lowercase hex\n")?,
                )
                .is_none()
        {
            continue;
        }
        if option == OsStr::new("--state")
            && state
                .replace(parse_task_state(value).ok_or("error: task --state is invalid\n")?)
                .is_none()
        {
            continue;
        }
        if option == OsStr::new("--objective") && objective.replace(value.to_owned()).is_none() {
            continue;
        }
        if option == OsStr::new("--hypothesis") && hypothesis.replace(value.to_owned()).is_none() {
            continue;
        }
        if option == OsStr::new("--next-safe-action")
            && next_safe_action.replace(value.to_owned()).is_none()
        {
            continue;
        }
        return Err("error: task arguments are invalid\n");
    }
    let Some(repository) = repository.filter(|value| !value.is_empty()) else {
        return Err("error: task requires --repository-id\n");
    };
    let Some(database) = database.filter(|value| !value.as_os_str().is_empty()) else {
        return Err("error: task requires --database\n");
    };
    let Some(state) = state else {
        return Err("error: task requires --state\n");
    };
    let Some(objective) = objective.filter(|value| !value.is_empty()) else {
        return Err("error: task requires --objective\n");
    };
    if create && task_id.is_some() {
        return Err("error: task create does not accept --task-id\n");
    }
    if !create && task_id.is_none() {
        return Err("error: task checkpoint requires --task-id\n");
    }
    Ok(TaskCheckpointInvocation {
        database,
        repository,
        task_id,
        state,
        objective,
        hypothesis,
        next_safe_action,
    })
}

const fn parse_task_state(text: &str) -> Option<TaskState> {
    match text.as_bytes() {
        b"open" => Some(TaskState::Open),
        b"blocked" => Some(TaskState::Blocked),
        b"completed" => Some(TaskState::Completed),
        b"cancelled" => Some(TaskState::Cancelled),
        _ => None,
    }
}

fn task_current_unix_ms() -> Option<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
}

fn emit_task_checkpoint_receipt(
    stdout: &mut impl Write,
    operation: &OsStr,
    task_id: TaskId,
    sequence: u32,
) -> u8 {
    let operation = if operation == OsStr::new("create") {
        "task-create"
    } else {
        "task-checkpoint"
    };
    let report = format!(
        "operation={operation}\ntask_id={}\ncheckpoint_sequence={sequence}\n",
        task_id_to_hex(task_id),
    );
    emit_output(stdout, &report)
}

fn task_id_to_hex(task_id: TaskId) -> String {
    let mut output = String::with_capacity(32);
    for byte in task_id.as_bytes() {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    output
}
