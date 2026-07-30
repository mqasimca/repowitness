use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::io::Cursor;

use super::*;

const TEST_LIMITS: GitPathDiscoveryLimits = GitPathDiscoveryLimits::new(
    Duration::from_secs(1),
    128,
    4,
    RepositoryPathLimits::new(32, 4),
);

#[test]
fn default_limits_are_explicit_and_stable() {
    let limits = GitPathDiscoveryLimits::default();
    assert_eq!(limits.deadline(), Duration::from_secs(30));
    assert_eq!(limits.output_bytes(), 64 * 1024 * 1024);
    assert_eq!(limits.paths(), 1_000_000);
    assert_eq!(
        limits.repository_path(),
        RepositoryPathLimits::new(1024 * 1024, 65_535)
    );
}

#[test]
fn parses_valid_paths_into_deterministic_order_and_stats() {
    let discovered = parse_git_paths(b"src/lib.rs\0Cargo.toml\0".to_vec(), TEST_LIMITS)
        .expect("valid Git output must pass");
    assert_eq!(
        discovered
            .paths()
            .iter()
            .map(RepositoryPath::as_bytes)
            .collect::<Vec<_>>(),
        [b"Cargo.toml".as_slice(), b"src/lib.rs".as_slice()]
    );
    assert_eq!(
        discovered.stats(),
        GitPathDiscoveryStats {
            output_bytes: 22,
            path_count: 2,
            total_path_bytes: 20,
            longest_path_bytes: 10,
            most_components: 2,
        }
    );
    let owned_paths = discovered.clone().into_paths();
    assert_eq!(owned_paths.as_ref(), discovered.paths());
}

#[test]
fn accepts_an_empty_repository() {
    let discovered = parse_git_paths(Vec::new(), TEST_LIMITS).expect("empty output is canonical");
    assert!(discovered.paths().is_empty());
    assert_eq!(
        discovered.stats(),
        GitPathDiscoveryStats {
            output_bytes: 0,
            path_count: 0,
            total_path_bytes: 0,
            longest_path_bytes: 0,
            most_components: 0,
        }
    );
}

#[test]
fn stable_deleted_paths_are_removed_and_inconsistent_sets_fail_closed() {
    let candidates = parse_git_paths(b"gone.rs\0kept.rs\0".to_vec(), TEST_LIMITS)
        .expect("cached paths should parse");
    let deleted =
        parse_git_paths(b"gone.rs\0".to_vec(), TEST_LIMITS).expect("deleted paths should parse");
    let mut cached_paths = candidates
        .into_paths()
        .into_vec()
        .into_iter()
        .collect::<BTreeSet<_>>();
    remove_deleted_cached_paths(
        &mut cached_paths,
        deleted.into_paths(),
        TEST_LIMITS,
        Instant::now() + TEST_LIMITS.deadline(),
        &mut || false,
    )
    .expect("a deleted subset should filter deterministically");

    assert_eq!(
        cached_paths
            .iter()
            .map(RepositoryPath::as_bytes)
            .collect::<Vec<_>>(),
        [b"kept.rs".as_slice()]
    );

    let candidates =
        parse_git_paths(b"kept.rs\0".to_vec(), TEST_LIMITS).expect("candidate path should parse");
    let inconsistent =
        parse_git_paths(b"missing.rs\0".to_vec(), TEST_LIMITS).expect("deleted path should parse");
    let mut cached_paths = candidates
        .into_paths()
        .into_vec()
        .into_iter()
        .collect::<BTreeSet<_>>();
    assert!(matches!(
        remove_deleted_cached_paths(
            &mut cached_paths,
            inconsistent.into_paths(),
            TEST_LIMITS,
            Instant::now() + TEST_LIMITS.deadline(),
            &mut || false,
        ),
        Err(GitPathDiscoveryError::InconsistentRepositoryPathSet)
    ));
}

#[test]
fn parsing_has_cooperative_cancellation_and_deadline_diagnostics() {
    let mut checks = 0_u8;
    let error = parse_git_paths_with_control(
        b"a\0b\0".to_vec(),
        TEST_LIMITS,
        Instant::now() + TEST_LIMITS.deadline(),
        &mut || {
            checks += 1;
            checks >= 2
        },
    )
    .expect_err("parsing cancellation must be observed between records");
    assert!(matches!(error, GitPathDiscoveryError::Cancelled));

    let mut cancelled = || false;
    let error =
        parse_git_paths_with_control(b"a\0".to_vec(), TEST_LIMITS, Instant::now(), &mut cancelled)
            .expect_err("an expired parse deadline must fail");
    assert!(matches!(
        error,
        GitPathDiscoveryError::DeadlineExceeded {
            deadline
        } if deadline == TEST_LIMITS.deadline()
    ));
}

#[test]
fn enforces_output_and_path_count_bounds() {
    let output_limited = GitPathDiscoveryLimits::new(
        Duration::from_secs(1),
        1,
        4,
        RepositoryPathLimits::new(32, 4),
    );
    assert!(matches!(
        parse_git_paths(b"a\0".to_vec(), output_limited),
        Err(GitPathDiscoveryError::OutputByteLimitExceeded { limit: 1 })
    ));

    let path_limited = GitPathDiscoveryLimits::new(
        Duration::from_secs(1),
        128,
        1,
        RepositoryPathLimits::new(32, 4),
    );
    assert!(matches!(
        parse_git_paths(b"a\0b\0".to_vec(), path_limited),
        Err(GitPathDiscoveryError::PathLimitExceeded { limit: 1 })
    ));
}

#[test]
fn rejects_unterminated_invalid_and_duplicate_paths_without_exposing_bytes() {
    assert!(matches!(
        parse_git_paths(b"src/lib.rs".to_vec(), TEST_LIMITS),
        Err(GitPathDiscoveryError::OutputNotNulTerminated)
    ));

    let error = parse_git_paths(b"secret/../value\0".to_vec(), TEST_LIMITS)
        .expect_err("invalid path must fail");
    assert!(matches!(
        error,
        GitPathDiscoveryError::InvalidRepositoryPath { ordinal: 1, .. }
    ));
    let diagnostic = error.to_string();
    assert!(!diagnostic.contains("secret"));
    assert!(!diagnostic.contains("value"));

    assert!(matches!(
        parse_git_paths(b"same\0same\0".to_vec(), TEST_LIMITS),
        Err(GitPathDiscoveryError::DuplicateRepositoryPath)
    ));
}

#[test]
fn bounded_reader_accepts_exact_limit_and_rejects_one_more_byte() {
    assert_eq!(
        read_bounded(Cursor::new(b"abcd"), 4).expect("exact limit must pass"),
        b"abcd"
    );
    assert!(matches!(
        read_bounded(Cursor::new(b"abcde"), 4),
        Err(BoundedReadError::LimitExceeded)
    ));
}

struct FailingReader;

impl Read for FailingReader {
    fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::other("intentional test failure"))
    }
}

#[test]
fn bounded_reader_preserves_io_failures() {
    let error = read_bounded(FailingReader, 4).expect_err("I/O failure must propagate");
    assert!(matches!(error, BoundedReadError::Io(_)));
}

#[test]
fn cancellation_before_spawn_is_deterministic() {
    let error = discover_repository_paths_with_cancel(
        Path::new("does-not-need-to-exist"),
        TEST_LIMITS,
        || true,
    )
    .expect_err("pre-cancelled discovery must fail");
    assert!(matches!(error, GitPathDiscoveryError::Cancelled));
}

#[test]
fn zero_deadline_fails_before_spawn() {
    let limits = GitPathDiscoveryLimits::new(
        Duration::ZERO,
        TEST_LIMITS.output_bytes(),
        TEST_LIMITS.paths(),
        TEST_LIMITS.repository_path(),
    );
    let error = discover_repository_paths(Path::new("does-not-need-to-exist"), limits)
        .expect_err("a zero deadline must fail before Git starts");
    assert!(matches!(
        error,
        GitPathDiscoveryError::DeadlineExceeded {
            deadline: Duration::ZERO
        }
    ));
}

#[test]
fn command_start_stdout_deadline_output_limit_and_cancellation_are_bounded() {
    let mut cancelled = || false;
    let error = capture_git_output_from_command(
        Command::new("repowitness-command-that-does-not-exist"),
        TEST_LIMITS,
        Instant::now() + TEST_LIMITS.deadline(),
        &mut cancelled,
    )
    .expect_err("a missing executable must fail to start");
    assert!(matches!(error, GitPathDiscoveryError::GitStart { .. }));

    let mut no_stdout = Command::new("git");
    no_stdout
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let error = capture_git_output_from_command(
        no_stdout,
        TEST_LIMITS,
        Instant::now() + TEST_LIMITS.deadline(),
        &mut cancelled,
    )
    .expect_err("a command without piped stdout must fail");
    assert!(matches!(error, GitPathDiscoveryError::GitStdoutUnavailable));

    let output_limited = GitPathDiscoveryLimits::new(
        Duration::from_secs(1),
        0,
        TEST_LIMITS.paths(),
        TEST_LIMITS.repository_path(),
    );
    let mut output_command = Command::new("git");
    output_command
        .arg("--version")
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let error = capture_git_output_from_command(
        output_command,
        output_limited,
        Instant::now() + output_limited.deadline(),
        &mut cancelled,
    )
    .expect_err("output over a zero-byte limit must fail");
    assert!(matches!(
        error,
        GitPathDiscoveryError::OutputByteLimitExceeded { limit: 0 }
    ));

    let mut waiting_command = Command::new("git");
    waiting_command
        .args(["hash-object", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let deadline = Duration::from_millis(10);
    let error = capture_git_output_from_command(
        waiting_command,
        GitPathDiscoveryLimits::new(
            deadline,
            TEST_LIMITS.output_bytes(),
            TEST_LIMITS.paths(),
            TEST_LIMITS.repository_path(),
        ),
        Instant::now() + deadline,
        &mut cancelled,
    )
    .expect_err("a waiting command must hit its deadline");
    assert!(matches!(
        error,
        GitPathDiscoveryError::DeadlineExceeded {
            deadline: observed
        } if observed == deadline
    ));

    let mut waiting_command = Command::new("git");
    waiting_command
        .args(["hash-object", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut cancel_now = || true;
    let error = capture_git_output_from_command(
        waiting_command,
        TEST_LIMITS,
        Instant::now() + TEST_LIMITS.deadline(),
        &mut cancel_now,
    )
    .expect_err("cancellation must terminate a waiting command");
    assert!(matches!(error, GitPathDiscoveryError::Cancelled));
}

#[cfg(unix)]
#[test]
fn inherited_stdout_writer_cannot_extend_the_declared_deadline() {
    let mut command = Command::new("sh");
    command
        .args(["-c", "sleep 1 &"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let deadline = Duration::from_millis(20);
    let started = Instant::now();
    let mut cancelled = || false;
    let error = capture_git_output_from_command(
        command,
        GitPathDiscoveryLimits::new(
            deadline,
            TEST_LIMITS.output_bytes(),
            TEST_LIMITS.paths(),
            TEST_LIMITS.repository_path(),
        ),
        started + deadline,
        &mut cancelled,
    )
    .expect_err("an inherited writer must not extend the direct child deadline");
    assert!(matches!(
        error,
        GitPathDiscoveryError::DeadlineExceeded {
            deadline: observed
        } if observed == deadline
    ));
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "the reader join extended the declared deadline"
    );
}

#[test]
fn an_invalid_root_returns_a_redacted_resolution_failure() {
    let error = discover_repository_paths(
        Path::new("repowitness-path-that-does-not-exist"),
        TEST_LIMITS,
    )
    .expect_err("worktree resolution must reject an invalid root");
    assert!(matches!(
        error,
        GitPathDiscoveryError::WorktreeRootResolve { .. }
    ));
    assert!(
        !error
            .to_string()
            .contains("repowitness-path-that-does-not-exist")
    );
}

#[test]
fn git_command_disables_ambient_and_interactive_behavior() {
    let command = sanitized_git_command(Path::new("repository"), GitPathDiscoveryScope::Untracked);
    assert_eq!(command.get_program(), OsStr::new("git"));

    let args = command.get_args().map(OsStr::to_owned).collect::<Vec<_>>();
    for expected in [
        "--no-pager",
        "--literal-pathspecs",
        "core.fsmonitor=false",
        "core.ignorecase=false",
        "core.untrackedCache=false",
        "diff.external=",
        "pager.ls-files=false",
        "-C",
        "repository",
        "ls-files",
        "-z",
        "--full-name",
        "--deduplicate",
        "--others",
        "--exclude-standard",
    ] {
        assert!(
            args.contains(&OsString::from(expected)),
            "missing {expected}"
        );
    }

    let environment = command
        .get_envs()
        .map(|(key, value)| (key.to_owned(), value.map(OsStr::to_owned)))
        .collect::<BTreeMap<_, _>>();
    for (key, expected) in [
        ("GIT_CONFIG_NOSYSTEM", Some("1")),
        ("GIT_CONFIG_GLOBAL", Some(null_device())),
        ("GIT_CONFIG_SYSTEM", Some(null_device())),
        ("GIT_TERMINAL_PROMPT", Some("0")),
        ("GCM_INTERACTIVE", Some("never")),
        ("GIT_OPTIONAL_LOCKS", Some("0")),
        ("GIT_NO_REPLACE_OBJECTS", Some("1")),
        ("GIT_PAGER", Some("cat")),
        ("PAGER", Some("cat")),
        ("GIT_DIR", None),
        ("GIT_WORK_TREE", None),
        ("GIT_INDEX_FILE", None),
        ("GIT_OBJECT_DIRECTORY", None),
        ("GIT_ALTERNATE_OBJECT_DIRECTORIES", None),
        ("GIT_SHALLOW_FILE", None),
        ("GIT_REPLACE_REF_BASE", None),
        ("GIT_COMMON_DIR", None),
        ("GIT_CONFIG", None),
        ("GIT_NAMESPACE", None),
        ("GIT_IMPLICIT_WORK_TREE", None),
        ("GIT_CEILING_DIRECTORIES", None),
        ("GIT_DISCOVERY_ACROSS_FILESYSTEM", None),
        ("GIT_REFERENCE_BACKEND", None),
        ("GIT_LITERAL_PATHSPECS", None),
        ("GIT_GLOB_PATHSPECS", None),
        ("GIT_NOGLOB_PATHSPECS", None),
        ("GIT_ICASE_PATHSPECS", None),
        ("GIT_CONFIG_COUNT", None),
        ("GIT_CONFIG_PARAMETERS", None),
        ("GIT_EXTERNAL_DIFF", None),
        ("GIT_FLUSH", None),
        ("GIT_TRACE", None),
        ("GIT_TRACE_CURL", None),
        ("GIT_TRACE_CURL_NO_DATA", None),
        ("GIT_TRACE_FSMONITOR", None),
        ("GIT_TRACE_PACKET", None),
        ("GIT_TRACE_PACK_ACCESS", None),
        ("GIT_TRACE_PACKFILE", None),
        ("GIT_TRACE_PERFORMANCE", None),
        ("GIT_TRACE_REDACT", None),
        ("GIT_TRACE_REFS", None),
        ("GIT_TRACE_SETUP", None),
        ("GIT_TRACE_SHALLOW", None),
        ("GIT_TRACE2", None),
        ("GIT_TRACE2_BRIEF", None),
        ("GIT_TRACE2_CONFIG_PARAMS", None),
        ("GIT_TRACE2_DST_DEBUG", None),
        ("GIT_TRACE2_ENV_VARS", None),
        ("GIT_TRACE2_EVENT", None),
        ("GIT_TRACE2_EVENT_BRIEF", None),
        ("GIT_TRACE2_EVENT_NESTING", None),
        ("GIT_TRACE2_MAX_FILES", None),
        ("GIT_TRACE2_PARENT_SID", None),
        ("GIT_TRACE2_PERF", None),
        ("GIT_TRACE2_PERF_BRIEF", None),
    ] {
        assert_eq!(
            environment.get(OsStr::new(key)),
            Some(&expected.map(OsString::from)),
            "unexpected environment setting for {key}"
        );
    }

    assert_cached_command_scope();
    assert_untracked_command_scope();
    assert_deleted_command_scope();
}

fn assert_cached_command_scope() {
    let cached = sanitized_git_command(Path::new("repository"), GitPathDiscoveryScope::Cached);
    let cached_args = cached.get_args().map(OsStr::to_owned).collect::<Vec<_>>();
    assert!(cached_args.contains(&OsString::from("--cached")));
    assert!(cached_args.contains(&OsString::from("--deduplicate")));
    assert!(!cached_args.contains(&OsString::from("--others")));
    assert!(!cached_args.contains(&OsString::from("--exclude-standard")));
}

fn assert_untracked_command_scope() {
    let untracked =
        sanitized_git_command(Path::new("repository"), GitPathDiscoveryScope::Untracked);
    let untracked_args = untracked
        .get_args()
        .map(OsStr::to_owned)
        .collect::<Vec<_>>();
    assert!(untracked_args.contains(&OsString::from("--others")));
    assert!(untracked_args.contains(&OsString::from("--exclude-standard")));
    assert!(untracked_args.contains(&OsString::from("--deduplicate")));
    assert!(!untracked_args.contains(&OsString::from("--cached")));
    assert!(!untracked_args.contains(&OsString::from("--deleted")));
}

fn assert_deleted_command_scope() {
    let deleted = sanitized_git_command(Path::new("repository"), GitPathDiscoveryScope::Deleted);
    let deleted_args = deleted.get_args().map(OsStr::to_owned).collect::<Vec<_>>();
    assert!(deleted_args.contains(&OsString::from("--deleted")));
    assert!(deleted_args.contains(&OsString::from("--deduplicate")));
    assert!(!deleted_args.contains(&OsString::from("--cached")));
    assert!(!deleted_args.contains(&OsString::from("--others")));
    assert!(!deleted_args.contains(&OsString::from("--exclude-standard")));
}

#[test]
fn errors_expose_only_safe_sources() {
    let io_error = GitPathDiscoveryError::GitStart {
        source: io::Error::other("safe test"),
    };
    assert!(std::error::Error::source(&io_error).is_some());

    let limit_error = GitPathDiscoveryError::PathLimitExceeded { limit: 1 };
    assert!(std::error::Error::source(&limit_error).is_none());
    assert_eq!(
        limit_error.to_string(),
        "repository path count exceeded its 1 path bound"
    );
}

#[test]
fn every_error_variant_has_a_stable_redacted_diagnostic() {
    let io_error = || io::Error::other("private-source-detail");
    let errors = [
        GitPathDiscoveryError::DeadlineNotRepresentable,
        GitPathDiscoveryError::Cancelled,
        GitPathDiscoveryError::WorktreeRootResolve { source: io_error() },
        GitPathDiscoveryError::WorktreeMarkerInspect { source: io_error() },
        GitPathDiscoveryError::WorktreeMarkerUnsupported,
        GitPathDiscoveryError::WorktreeMarkerNotFound,
        GitPathDiscoveryError::GitStart { source: io_error() },
        GitPathDiscoveryError::GitStdoutUnavailable,
        GitPathDiscoveryError::OutputReaderStart { source: io_error() },
        GitPathDiscoveryError::GitOutputRead { source: io_error() },
        GitPathDiscoveryError::OutputByteLimitExceeded { limit: 7 },
        GitPathDiscoveryError::OutputByteCountNotRepresentable,
        GitPathDiscoveryError::OutputReaderStopped,
        GitPathDiscoveryError::OutputReaderPanicked,
        GitPathDiscoveryError::GitPoll { source: io_error() },
        GitPathDiscoveryError::DeadlineExceeded {
            deadline: Duration::from_millis(9),
        },
        GitPathDiscoveryError::GitUnsuccessful { code: Some(128) },
        GitPathDiscoveryError::GitUnsuccessful { code: None },
        GitPathDiscoveryError::OutputNotNulTerminated,
        GitPathDiscoveryError::PathCountOverflowed,
        GitPathDiscoveryError::PathLimitExceeded { limit: 3 },
        GitPathDiscoveryError::InvalidRepositoryPath {
            ordinal: 2,
            source: RepositoryPathError::Empty,
        },
        GitPathDiscoveryError::DuplicateRepositoryPath,
        GitPathDiscoveryError::RepositoryPathInspection {
            source: ContainedSourceError::NotRegularFile,
        },
        GitPathDiscoveryError::InconsistentRepositoryPathSet,
        GitPathDiscoveryError::TotalPathBytesOverflowed,
    ];
    for error in errors {
        let diagnostic = error.to_string();
        assert!(!diagnostic.is_empty());
        assert!(!diagnostic.contains("private-source-detail"));
    }
}
