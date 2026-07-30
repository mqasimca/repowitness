fn sanitized_git_command(worktree_root: &Path, scope: GitPathDiscoveryScope) -> Command {
    let mut command = sanitized_git_base_command(worktree_root);
    command
        .arg("ls-files")
        .arg("-z")
        .arg("--full-name")
        .arg("--deduplicate");
    match scope {
        GitPathDiscoveryScope::Cached => {
            command.arg("--cached");
        }
        GitPathDiscoveryScope::Untracked => {
            command.arg("--others").arg("--exclude-standard");
        }
        GitPathDiscoveryScope::Deleted => {
            command.arg("--deleted");
        }
    }
    command
}

pub(crate) fn sanitized_git_base_command(worktree_root: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .arg("--no-pager")
        .arg("--literal-pathspecs")
        .arg("-c")
        .arg("core.fsmonitor=false")
        .arg("-c")
        .arg("core.ignorecase=false")
        .arg("-c")
        .arg(format!("core.hooksPath={}", null_device()))
        .arg("-c")
        .arg(format!("core.excludesFile={}", null_device()))
        .arg("-c")
        .arg("core.untrackedCache=false")
        .arg("-c")
        .arg("diff.external=")
        .arg("-c")
        .arg("pager.ls-files=false")
        .arg("-c")
        .arg("pager.rev-parse=false")
        .arg("-c")
        .arg("pager.status=false")
        .arg(worktree_argument(worktree_root))
        .arg("-c")
        .arg("core.bare=false")
        .arg("-C")
        .arg(worktree_root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", null_device())
        .env("GIT_CONFIG_SYSTEM", null_device())
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "never")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .env("GIT_PAGER", "cat")
        .env("PAGER", "cat")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
        .env_remove("GIT_SHALLOW_FILE")
        .env_remove("GIT_REPLACE_REF_BASE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_CONFIG")
        .env_remove("GIT_NAMESPACE")
        .env_remove("GIT_IMPLICIT_WORK_TREE")
        .env_remove("GIT_CEILING_DIRECTORIES")
        .env_remove("GIT_DISCOVERY_ACROSS_FILESYSTEM")
        .env_remove("GIT_REFERENCE_BACKEND")
        .env_remove("GIT_LITERAL_PATHSPECS")
        .env_remove("GIT_GLOB_PATHSPECS")
        .env_remove("GIT_NOGLOB_PATHSPECS")
        .env_remove("GIT_ICASE_PATHSPECS")
        .env_remove("GIT_CONFIG_COUNT")
        .env_remove("GIT_CONFIG_PARAMETERS")
        .env_remove("GIT_EXTERNAL_DIFF")
        .env_remove("GIT_FLUSH")
        .env_remove("GIT_TRACE")
        .env_remove("GIT_TRACE_CURL")
        .env_remove("GIT_TRACE_CURL_NO_DATA")
        .env_remove("GIT_TRACE_FSMONITOR")
        .env_remove("GIT_TRACE_PACKET")
        .env_remove("GIT_TRACE_PACK_ACCESS")
        .env_remove("GIT_TRACE_PACKFILE")
        .env_remove("GIT_TRACE_PERFORMANCE")
        .env_remove("GIT_TRACE_REDACT")
        .env_remove("GIT_TRACE_REFS")
        .env_remove("GIT_TRACE_SETUP")
        .env_remove("GIT_TRACE_SHALLOW")
        .env_remove("GIT_TRACE2")
        .env_remove("GIT_TRACE2_BRIEF")
        .env_remove("GIT_TRACE2_CONFIG_PARAMS")
        .env_remove("GIT_TRACE2_DST_DEBUG")
        .env_remove("GIT_TRACE2_ENV_VARS")
        .env_remove("GIT_TRACE2_EVENT")
        .env_remove("GIT_TRACE2_EVENT_BRIEF")
        .env_remove("GIT_TRACE2_EVENT_NESTING")
        .env_remove("GIT_TRACE2_MAX_FILES")
        .env_remove("GIT_TRACE2_PARENT_SID")
        .env_remove("GIT_TRACE2_PERF")
        .env_remove("GIT_TRACE2_PERF_BRIEF")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    command
}

pub(crate) fn discovered_worktree_root(root: &Path) -> Result<PathBuf, GitPathDiscoveryError> {
    let mut current = fs::canonicalize(root)
        .map_err(|source| GitPathDiscoveryError::WorktreeRootResolve { source })?;
    loop {
        match fs::symlink_metadata(current.join(".git")) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(GitPathDiscoveryError::WorktreeMarkerUnsupported);
            }
            Ok(metadata) if metadata.is_dir() || metadata.is_file() => return Ok(current),
            Ok(_) => return Err(GitPathDiscoveryError::WorktreeMarkerUnsupported),
            Err(source) if source.kind() == io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(GitPathDiscoveryError::WorktreeMarkerInspect { source });
            }
        }
        if !current.pop() {
            return Err(GitPathDiscoveryError::WorktreeMarkerNotFound);
        }
    }
}

fn worktree_argument(worktree: &Path) -> OsString {
    let mut argument = OsString::from("--work-tree=");
    argument.push(worktree.as_os_str());
    argument
}

#[cfg(unix)]
const fn null_device() -> &'static str {
    "/dev/null"
}

#[cfg(windows)]
const fn null_device() -> &'static str {
    "NUL"
}

fn wait_until_deadline(
    child: &mut Child,
    deadline: Instant,
    configured_deadline: Duration,
    is_cancelled: &mut impl FnMut() -> bool,
) -> Result<ExitStatus, GitPathDiscoveryError> {
    loop {
        if is_cancelled() {
            terminate(child);
            return Err(GitPathDiscoveryError::Cancelled);
        }
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => {}
            Err(source) => {
                terminate(child);
                return Err(GitPathDiscoveryError::GitPoll { source });
            }
        }
        if Instant::now() >= deadline {
            terminate(child);
            return Err(GitPathDiscoveryError::DeadlineExceeded {
                deadline: configured_deadline,
            });
        }
        sleep_until_next_poll(deadline);
    }
}

fn sleep_until_next_poll(deadline: Instant) {
    let remaining = deadline.saturating_duration_since(Instant::now());
    thread::sleep(POLL_INTERVAL.min(remaining));
}

fn terminate(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

#[derive(Debug)]
enum BoundedReadError {
    Io(io::Error),
    LimitExceeded,
}

fn read_bounded(mut reader: impl Read, limit: u64) -> Result<Vec<u8>, BoundedReadError> {
    let capacity = usize::try_from(limit.min(64 * 1024)).unwrap_or(64 * 1024);
    let mut output = Vec::with_capacity(capacity);
    let mut buffer = [0_u8; 8 * 1024];
    let mut total = 0_u64;

    loop {
        let read_count = reader.read(&mut buffer).map_err(BoundedReadError::Io)?;
        if read_count == 0 {
            return Ok(output);
        }
        let read_count = u64::try_from(read_count)
            .map_err(|_| BoundedReadError::Io(io::Error::other("read length overflowed u64")))?;
        total = total
            .checked_add(read_count)
            .ok_or_else(|| BoundedReadError::Io(io::Error::other("read length overflowed u64")))?;
        if total > limit {
            return Err(BoundedReadError::LimitExceeded);
        }
        let read_count = usize::try_from(read_count)
            .map_err(|_| BoundedReadError::Io(io::Error::other("read length overflowed usize")))?;
        output.extend_from_slice(&buffer[..read_count]);
    }
}

#[cfg(test)]
pub(crate) fn parse_git_paths(
    output: Vec<u8>,
    limits: GitPathDiscoveryLimits,
) -> Result<DiscoveredRepositoryPaths, GitPathDiscoveryError> {
    let deadline = Instant::now()
        .checked_add(limits.deadline())
        .ok_or(GitPathDiscoveryError::DeadlineNotRepresentable)?;
    parse_git_paths_with_control(output, limits, deadline, &mut || false)
}

fn parse_git_paths_with_control(
    output: Vec<u8>,
    limits: GitPathDiscoveryLimits,
    deadline: Instant,
    is_cancelled: &mut impl FnMut() -> bool,
) -> Result<DiscoveredRepositoryPaths, GitPathDiscoveryError> {
    check_operation_control(deadline, limits.deadline(), is_cancelled)?;
    let output_bytes = u64::try_from(output.len())
        .map_err(|_| GitPathDiscoveryError::OutputByteCountNotRepresentable)?;
    if output_bytes > limits.output_bytes() {
        return Err(GitPathDiscoveryError::OutputByteLimitExceeded {
            limit: limits.output_bytes(),
        });
    }
    if output.is_empty() {
        return Ok(DiscoveredRepositoryPaths {
            paths: Box::new([]),
            stats: GitPathDiscoveryStats::new(0, 0, 0, 0, 0),
        });
    }

    let path_bytes = output
        .strip_suffix(&[0])
        .ok_or(GitPathDiscoveryError::OutputNotNulTerminated)?;
    let initial_capacity = usize::try_from(limits.paths().min(4096)).unwrap_or(4096);
    let mut paths = Vec::with_capacity(initial_capacity);
    let mut stats = GitPathDiscoveryStats::new(output_bytes, 0, 0, 0, 0);

    for raw_path in path_bytes.split(|byte| *byte == 0) {
        check_operation_control(deadline, limits.deadline(), is_cancelled)?;
        stats.path_count = stats
            .path_count
            .checked_add(1)
            .ok_or(GitPathDiscoveryError::PathCountOverflowed)?;
        if stats.path_count > limits.paths() {
            return Err(GitPathDiscoveryError::PathLimitExceeded {
                limit: limits.paths(),
            });
        }

        let path = RepositoryPath::try_from_bytes(raw_path, limits.repository_path()).map_err(
            |source| GitPathDiscoveryError::InvalidRepositoryPath {
                ordinal: stats.path_count,
                source,
            },
        )?;
        stats.total_path_bytes = stats
            .total_path_bytes
            .checked_add(path.byte_count().get())
            .ok_or(GitPathDiscoveryError::TotalPathBytesOverflowed)?;
        stats.longest_path_bytes = stats.longest_path_bytes.max(path.byte_count().get());
        stats.most_components = stats.most_components.max(path.component_count().get());
        paths.push(path);
    }

    check_operation_control(deadline, limits.deadline(), is_cancelled)?;
    paths.sort_unstable();
    check_operation_control(deadline, limits.deadline(), is_cancelled)?;
    if paths.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(GitPathDiscoveryError::DuplicateRepositoryPath);
    }

    Ok(DiscoveredRepositoryPaths {
        paths: paths.into_boxed_slice(),
        stats,
    })
}

fn check_operation_control(
    deadline: Instant,
    configured_deadline: Duration,
    is_cancelled: &mut impl FnMut() -> bool,
) -> Result<(), GitPathDiscoveryError> {
    if is_cancelled() {
        return Err(GitPathDiscoveryError::Cancelled);
    }
    if Instant::now() >= deadline {
        return Err(GitPathDiscoveryError::DeadlineExceeded {
            deadline: configured_deadline,
        });
    }
    Ok(())
}

#[cfg(test)]
mod gix_spike_tests;
