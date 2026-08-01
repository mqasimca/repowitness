const MEMORY_HISTORY_HELP: &str = "Read one exact historical memory applicability receipt.\n\nUsage:\n  repowitness memory-history --repository-id <id> --database <path> --known-at <unix-ms>\\\n\n      (--git-commit <lowercase-sha1-or-sha256>|--snapshot <lowercase-sha256>) <repository>\n\nBranch names are deliberately not accepted. Git targets use a bounded local\nobject fence; missing or pruned objects are reported unavailable.\n";

fn run_memory_history(
    args: impl Iterator<Item = OsString>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
) -> u8 {
    let arguments: Vec<OsString> = args.take(12).collect();
    if matches!(arguments.as_slice(), [help] if help == OsStr::new("--help") || help == OsStr::new("-h")) {
        return emit_output(stdout, MEMORY_HISTORY_HELP);
    }
    let invocation = match parse_known_at_history_arguments(&arguments) {
        Ok(invocation) => invocation,
        Err(message) => return emit_error(stderr, EXIT_USAGE, message),
    };
    match read_local_known_at_history(
        LocalKnownAtHistoryRequest::new(
            &invocation.repository_root,
            &invocation.database,
            &invocation.repository_identity,
            invocation.known_at_unix_ms,
            invocation.target,
        )
        .with_max_results(invocation.max_results),
        Arc::new(AtomicBool::new(false)),
    ) {
        Ok(receipt) => emit_memory_history_receipt(stdout, &receipt),
        Err(_) => emit_error(stderr, EXIT_SOFTWARE, "error: historical memory read failed\n"),
    }
}

struct MemoryHistoryInvocation {
    repository_root: PathBuf,
    database: PathBuf,
    repository_identity: String,
    known_at_unix_ms: u64,
    target: MemoryObservationSource,
    max_results: u16,
}

fn parse_known_at_history_arguments(
    arguments: &[OsString],
) -> Result<MemoryHistoryInvocation, &'static str> {
    let mut root = None;
    let mut database = None;
    let mut repository_identity = None;
    let mut known_at = None;
    let mut target = None;
    let mut limit = 32_u16;
    let mut limit_seen = false;
    let mut index = 0_usize;
    while index < arguments.len() {
        let option = &arguments[index];
        index += 1;
        if option == OsStr::new("--") {
            let value = arguments
                .get(index)
                .ok_or("error: memory-history requires one repository\n")?;
            index += 1;
            if index != arguments.len() || root.replace(PathBuf::from(value)).is_some() {
                return Err("error: memory-history accepts exactly one repository\n");
            }
            continue;
        }
        if !option.as_encoded_bytes().starts_with(b"-") {
            if root.replace(PathBuf::from(option)).is_some() {
                return Err("error: memory-history accepts exactly one repository\n");
            }
            continue;
        }
        let value = arguments
            .get(index)
            .ok_or("error: memory-history option requires a value\n")?;
        index += 1;
        if option == OsStr::new("--repository-id") {
            let value = history_utf8(value)?;
            if repository_identity.replace(value).is_some() {
                return Err("error: memory-history accepts --repository-id only once\n");
            }
        } else if option == OsStr::new("--database") {
            if database.replace(PathBuf::from(value)).is_some() {
                return Err("error: memory-history accepts --database only once\n");
            }
        } else if option == OsStr::new("--known-at") {
            let value = history_utf8(value)?
                .parse::<u64>()
                .map_err(|_| "error: memory-history --known-at must be an integer\n")?;
            if known_at.replace(value).is_some() {
                return Err("error: memory-history accepts --known-at only once\n");
            }
        } else if option == OsStr::new("--git-commit") {
            let commit = parse_history_commit(&history_utf8(value)?)?;
            if target.replace(MemoryObservationSource::Git(commit)).is_some() {
                return Err("error: memory-history accepts exactly one target\n");
            }
        } else if option == OsStr::new("--snapshot") {
            let snapshot = SourceSnapshotDigest::try_from_slice(&decode_history_hex::<32>(
                &history_utf8(value)?,
            )?)
            .map_err(|_| "error: memory-history snapshot target is invalid\n")?;
            if target.replace(MemoryObservationSource::Worktree(snapshot)).is_some() {
                return Err("error: memory-history accepts exactly one target\n");
            }
        } else if option == OsStr::new("--limit") {
            let value = history_utf8(value)?
                .parse::<u16>()
                .map_err(|_| "error: memory-history --limit must be an integer from 1 through 100\n")?;
            if value == 0 || value > 100 || limit_seen {
                return Err("error: memory-history --limit must be an integer from 1 through 100\n");
            }
            limit = value;
            limit_seen = true;
        } else {
            return Err("error: unknown memory-history option; use memory-history --help\n");
        }
    }
    Ok(MemoryHistoryInvocation {
        repository_root: root.ok_or("error: memory-history requires one repository\n")?,
        database: database.ok_or("error: memory-history requires --database\n")?,
        repository_identity: repository_identity
            .filter(|value| !value.is_empty())
            .ok_or("error: memory-history requires --repository-id\n")?,
        known_at_unix_ms: known_at.ok_or("error: memory-history requires --known-at\n")?,
        target: target.ok_or("error: memory-history requires one exact target\n")?,
        max_results: limit,
    })
}

fn history_utf8(value: &OsStr) -> Result<String, &'static str> {
    value
        .to_str()
        .map(str::to_owned)
        .ok_or("error: memory-history option must be UTF-8\n")
}

fn parse_history_commit(value: &str) -> Result<MemoryCommitId, &'static str> {
    match value.len() {
        40 => Ok(MemoryCommitId::Sha1(decode_history_hex::<20>(value)?)),
        64 => Ok(MemoryCommitId::Sha256(decode_history_hex::<32>(value)?)),
        _ => Err("error: memory-history Git target must be lowercase SHA-1 or SHA-256\n"),
    }
}

fn decode_history_hex<const N: usize>(value: &str) -> Result<[u8; N], &'static str> {
    if value.len() != N * 2 {
        return Err("error: memory-history target has an invalid length\n");
    }
    let mut output = [0_u8; N];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        let high = history_hex_nibble(pair[0])
            .ok_or("error: memory-history target must be lowercase hexadecimal\n")?;
        let low = history_hex_nibble(pair[1])
            .ok_or("error: memory-history target must be lowercase hexadecimal\n")?;
        output[index] = high << 4 | low;
    }
    Ok(output)
}

const fn history_hex_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        _ => None,
    }
}

fn emit_memory_history_receipt(
    stdout: &mut impl Write,
    receipt: &repowitness_local::KnownAtHistoryReceipt,
) -> u8 {
    let mut output = String::from("operation=memory-history\ncoverage=");
    output.push_str(match receipt.coverage() {
        repowitness_local::KnownAtHistoryCoverage::Complete => "complete",
        repowitness_local::KnownAtHistoryCoverage::Truncated => "truncated",
    });
    output.push_str("\napplicability=");
    output.push_str(match receipt.applicability() {
        repowitness_local::KnownAtApplicability::Unavailable => "unavailable",
        repowitness_local::KnownAtApplicability::NotApplicable => "not_applicable",
        repowitness_local::KnownAtApplicability::Applicable => "applicable",
    });
    output.push_str("\nevidence_count=");
    output.push_str(&receipt.evidence().len().to_string());
    output.push('\n');
    for (index, evidence) in receipt.evidence().iter().enumerate() {
        output.push_str("evidence.");
        output.push_str(&index.to_string());
        output.push_str(".record_id=");
        output.push_str(MemoryRecordIdTextV1::encode(evidence.record_id()).as_str());
        output.push_str("\nevidence.");
        output.push_str(&index.to_string());
        output.push_str(".revision=");
        output.push_str(&hex(evidence.revision().as_bytes()));
        output.push_str("\nevidence.");
        output.push_str(&index.to_string());
        output.push_str(".basis=");
        output.push_str(match evidence.basis() {
            repowitness_local::KnownAtEvidenceBasis::Observation => "observation",
            repowitness_local::KnownAtEvidenceBasis::ReviewedCorrespondence => {
                "reviewed_correspondence"
            }
        });
        output.push('\n');
    }
    emit_output(stdout, &output)
}
