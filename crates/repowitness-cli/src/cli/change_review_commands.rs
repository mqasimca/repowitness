const CHANGE_REVIEW_HELP: &str = concat!(
    "Build a bounded, read-only revision-pinned change-review receipt.\n\n",
    "Usage:\n",
    "  repowitness verify --repository-id <id> --database <path> --root <path>\n",
    "      --base <full-lowercase-git-object-id> --intent <literal terms>\n\n",
    "The current worktree is fenced before and after review work. A changing worktree\n",
    "fails without a partial receipt. Indexed context is pinned to an immutable generation\n",
    "only when exact source expansion remains current; otherwise its categorical absence is\n",
    "reported without stale source. This command makes no claim that context matches the worktree,\n",
    "that tests ran, or that a change is approved.\n",
);

const MAX_CLI_CHANGE_REVIEW_OUTPUT_BYTES: usize = 4 * 1024 * 1024;

struct ChangeReviewInvocation {
    root: PathBuf,
    database: PathBuf,
    repository_identity: String,
    intent: String,
    base: GitObjectId,
}

fn run_change_review(
    args: impl Iterator<Item = OsString>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
) -> u8 {
    let arguments: Vec<OsString> = args.take(11).collect();
    if arguments.len() > 10 {
        return emit_error(stderr, EXIT_USAGE, "error: verify received too many arguments; use verify --help\n");
    }
    if matches!(arguments.as_slice(), [help] if help == OsStr::new("--help") || help == OsStr::new("-h")) {
        return emit_output(stdout, CHANGE_REVIEW_HELP);
    }
    let invocation = match parse_change_review_arguments(&arguments) {
        Ok(invocation) => invocation,
        Err(message) => return emit_error(stderr, EXIT_USAGE, message),
    };
    let request = LocalChangeReviewRequest::new(
        &invocation.root,
        &invocation.database,
        &invocation.repository_identity,
        &invocation.intent,
        invocation.base,
    );
    match build_local_change_review(request, Arc::new(AtomicBool::new(false))) {
        Ok(receipt) => emit_change_review_receipt(stdout, &receipt),
        Err(_) => emit_error(stderr, EXIT_SOFTWARE, "error: change review failed\n"),
    }
}

fn parse_change_review_arguments(
    arguments: &[OsString],
) -> Result<ChangeReviewInvocation, &'static str> {
    let mut root = None;
    let mut database = None;
    let mut repository_identity = None;
    let mut intent = None;
    let mut base = None;
    let mut index = 0_usize;
    while index < arguments.len() {
        let option = &arguments[index];
        let value = arguments.get(index + 1).ok_or(
            "error: verify options require a value; use verify --help\n",
        )?;
        index += 2;
        if option == OsStr::new("--root") {
            replace_once(&mut root, PathBuf::from(value), "error: verify --root was supplied more than once\n")?;
        } else if option == OsStr::new("--database") {
            replace_once(&mut database, PathBuf::from(value), "error: verify --database was supplied more than once\n")?;
        } else if option == OsStr::new("--repository-id") {
            replace_once(&mut repository_identity, value.clone(), "error: verify --repository-id was supplied more than once\n")?;
        } else if option == OsStr::new("--intent") {
            replace_once(&mut intent, value.clone(), "error: verify --intent was supplied more than once\n")?;
        } else if option == OsStr::new("--base") {
            let text = value.to_str().ok_or("error: verify --base must be valid UTF-8\n")?;
            let parsed = GitObjectId::try_from_hex(text)
                .map_err(|_| "error: verify --base must be a full lowercase SHA-1 or SHA-256 object id\n")?;
            replace_once(&mut base, parsed, "error: verify --base was supplied more than once\n")?;
        } else {
            return Err("error: unknown verify option; use verify --help\n");
        }
    }
    let repository_identity = repository_identity
        .ok_or("error: verify requires --repository-id\n")?
        .into_string()
        .map_err(|_| "error: verify repository identity must be valid UTF-8\n")?;
    let intent = intent
        .ok_or("error: verify requires --intent\n")?
        .into_string()
        .map_err(|_| "error: verify intent must be valid UTF-8\n")?;
    if repository_identity.is_empty() || intent.is_empty() {
        return Err("error: verify values must not be empty\n");
    }
    Ok(ChangeReviewInvocation {
        root: root.ok_or("error: verify requires --root\n")?,
        database: database.ok_or("error: verify requires --database\n")?,
        repository_identity,
        intent,
        base: base.ok_or("error: verify requires --base\n")?,
    })
}

fn emit_change_review_receipt(writer: &mut impl Write, receipt: &LocalChangeReviewReceipt) -> u8 {
    use std::fmt::Write as _;

    let mut output = String::new();
    let _ = writeln!(output, "operation=verify");
    let _ = writeln!(output, "change_manifest_profile={CHANGE_MANIFEST_PROFILE_VERSION}");
    let _ = writeln!(output, "base={}", receipt.manifest().base());
    let _ = writeln!(output, "worktree_git_state_sha256={}", hex(receipt.worktree_git_state().as_bytes()));
    let _ = writeln!(output, "worktree_change_count={}", receipt.manifest().path_count());
    match receipt.indexed_context() {
        IndexedContext::Available(context) => {
            let _ = writeln!(output, "indexed_context_availability=available");
            let _ = writeln!(output, "indexed_context_reason=not_applicable");
            let _ = writeln!(output, "indexed_snapshot_sha256={}", hex(context.snapshot().as_bytes()));
            let _ = writeln!(output, "indexed_generation={}", context.generation().get());
            let _ = writeln!(output, "indexed_context_items={}", context.items().len());
            let _ = writeln!(output, "indexed_context_omissions={}", context.omissions().len());
        }
        IndexedContext::Unavailable { reason } => {
            let _ = writeln!(output, "indexed_context_availability=unavailable");
            let _ = writeln!(output, "indexed_context_reason={}", reason.as_str());
            let _ = writeln!(output, "indexed_snapshot_sha256=not_provided");
            let _ = writeln!(output, "indexed_generation=not_provided");
            let _ = writeln!(output, "indexed_context_items=not_provided");
            let _ = writeln!(output, "indexed_context_omissions=not_provided");
        }
    }
    let _ = writeln!(
        output,
        "index_worktree_alignment={}",
        receipt.index_worktree_alignment().as_str()
    );
    let _ = writeln!(output, "verdict=not_provided");
    for (ordinal, entry) in receipt.manifest().entries().iter().enumerate() {
        let Ok(path) = RepositoryPathTextV1::encode(entry.path(), PATH_TEXT_LIMIT) else {
            return EXIT_SOFTWARE;
        };
        let _ = writeln!(output, "change[{ordinal}].kind={}", entry.kind().as_str());
        let _ = writeln!(output, "change[{ordinal}].path={}", path.as_str());
        let _ = writeln!(output, "change[{ordinal}].display_path={}", entry.path().display_text());
        if output.len() > MAX_CLI_CHANGE_REVIEW_OUTPUT_BYTES {
            return EXIT_SOFTWARE;
        }
    }
    if writer.write_all(output.as_bytes()).is_ok() { EXIT_SUCCESS } else { EXIT_IO }
}

#[cfg(test)]
mod change_review_command_tests {
    use std::ffi::OsString;

    use super::parse_change_review_arguments;

    #[test]
    fn parser_requires_one_complete_canonical_invocation() {
        let identity = format!("rwi1:h:{}", "01".repeat(32));
        let base = "ab".repeat(20);
        let invocation = parse_change_review_arguments(&[
            OsString::from("--root"), OsString::from("repository"),
            OsString::from("--database"), OsString::from("index.sqlite3"),
            OsString::from("--repository-id"), OsString::from(&identity),
            OsString::from("--intent"), OsString::from("review parser"),
            OsString::from("--base"), OsString::from(&base),
        ]).expect("complete invocation should parse");
        assert_eq!(invocation.base.to_hex(), base);
        assert!(parse_change_review_arguments(&[
            OsString::from("--base"), OsString::from("HEAD"),
        ]).is_err());
    }
}
