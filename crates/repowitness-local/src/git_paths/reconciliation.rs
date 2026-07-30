fn reconcile_repository_paths(
    worktree_root: &Path,
    cached: DiscoveredRepositoryPaths,
    untracked: DiscoveredRepositoryPaths,
    deleted: DiscoveredRepositoryPaths,
    limits: GitPathDiscoveryLimits,
    deadline: Instant,
    is_cancelled: &mut impl FnMut() -> bool,
) -> Result<DiscoveredRepositoryPaths, GitPathDiscoveryError> {
    let output_bytes = cached
        .stats()
        .output_bytes()
        .checked_add(untracked.stats().output_bytes())
        .ok_or(GitPathDiscoveryError::OutputByteCountNotRepresentable)?
        .checked_add(deleted.stats().output_bytes())
        .ok_or(GitPathDiscoveryError::OutputByteCountNotRepresentable)?;
    if output_bytes > limits.output_bytes() {
        return Err(GitPathDiscoveryError::OutputByteLimitExceeded {
            limit: limits.output_bytes(),
        });
    }

    let mut cached_paths = cached
        .into_paths()
        .into_vec()
        .into_iter()
        .collect::<BTreeSet<_>>();
    let untracked_paths = untracked
        .into_paths()
        .into_vec()
        .into_iter()
        .collect::<BTreeSet<_>>();
    remove_deleted_cached_paths(
        &mut cached_paths,
        deleted.into_paths(),
        limits,
        deadline,
        is_cancelled,
    )?;
    if cached_paths
        .iter()
        .any(|path| untracked_paths.contains(path))
    {
        return Err(GitPathDiscoveryError::InconsistentRepositoryPathSet);
    }

    let mut cached_ascii_aliases = BTreeMap::<Vec<u8>, u64>::new();
    for path in &cached_paths {
        check_operation_control(deadline, limits.deadline(), is_cancelled)?;
        increment_ascii_alias_count(&mut cached_ascii_aliases, path)?;
    }
    let mut untracked_ascii_aliases = BTreeMap::<Vec<u8>, u64>::new();
    for path in &untracked_paths {
        check_operation_control(deadline, limits.deadline(), is_cancelled)?;
        increment_ascii_alias_count(&mut untracked_ascii_aliases, path)?;
    }
    let mut retained = reconcile_cached_path_spellings(
        worktree_root,
        cached_paths,
        &untracked_paths,
        &cached_ascii_aliases,
        &untracked_ascii_aliases,
        limits,
        deadline,
        is_cancelled,
    )?;
    retained.extend(untracked_paths);

    let path_count = u64::try_from(retained.len())
        .map_err(|_| GitPathDiscoveryError::PathCountOverflowed)?;
    if path_count > limits.paths() {
        return Err(GitPathDiscoveryError::PathLimitExceeded {
            limit: limits.paths(),
        });
    }
    let mut stats = GitPathDiscoveryStats::new(output_bytes, 0, 0, 0, 0);
    for path in &retained {
        check_operation_control(deadline, limits.deadline(), is_cancelled)?;
        stats.path_count = stats
            .path_count
            .checked_add(1)
            .ok_or(GitPathDiscoveryError::PathCountOverflowed)?;
        stats.total_path_bytes = stats
            .total_path_bytes
            .checked_add(path.byte_count().get())
            .ok_or(GitPathDiscoveryError::TotalPathBytesOverflowed)?;
        stats.longest_path_bytes = stats.longest_path_bytes.max(path.byte_count().get());
        stats.most_components = stats.most_components.max(path.component_count().get());
    }
    Ok(DiscoveredRepositoryPaths {
        paths: retained.into_iter().collect::<Vec<_>>().into_boxed_slice(),
        stats,
    })
}

#[allow(clippy::too_many_arguments)]
fn reconcile_cached_path_spellings(
    worktree_root: &Path,
    cached_paths: BTreeSet<RepositoryPath>,
    untracked_paths: &BTreeSet<RepositoryPath>,
    cached_ascii_aliases: &BTreeMap<Vec<u8>, u64>,
    untracked_ascii_aliases: &BTreeMap<Vec<u8>, u64>,
    limits: GitPathDiscoveryLimits,
    deadline: Instant,
    is_cancelled: &mut impl FnMut() -> bool,
) -> Result<BTreeSet<RepositoryPath>, GitPathDiscoveryError> {
    let ambiguous_aliases = cached_ascii_aliases
        .iter()
        .filter(|(path, cached)| {
            **cached > 1 || untracked_ascii_aliases.contains_key(path.as_slice())
        })
        .map(|(path, _)| path.clone())
        .collect::<BTreeSet<_>>();
    if ambiguous_aliases.is_empty() {
        return Ok(cached_paths);
    }

    let requires_inspection =
        |path: &RepositoryPath| ambiguous_aliases.contains(&ascii_folded_repository_path(path));
    let source_root = ContainedSourceRoot::open(worktree_root)
        .map_err(map_repository_path_inspection_error)?;
    let mut exact_paths = source_root
        .exact_read_session(
            cached_paths
                .iter()
                .filter(|path| requires_inspection(path))
                .chain(
                    untracked_paths
                        .iter()
                        .filter(|path| requires_inspection(path)),
                ),
            deadline,
            &mut *is_cancelled,
        )
        .map_err(|error| map_exact_session_error(error, limits.deadline()))?;
    let mut exact_untracked_aliases = BTreeMap::<Vec<u8>, u64>::new();
    for path in untracked_paths
        .iter()
        .filter(|path| requires_inspection(path))
    {
        check_operation_control(deadline, limits.deadline(), is_cancelled)?;
        let exact = exact_paths
            .exact_components_available(
                path,
                limits.deadline(),
                deadline,
                is_cancelled,
            )
            .map_err(map_repository_path_inspection_error)?;
        if !exact {
            return Err(GitPathDiscoveryError::InconsistentRepositoryPathSet);
        }
        increment_ascii_alias_count(&mut exact_untracked_aliases, path)?;
    }

    let mut retained = BTreeSet::new();
    for path in cached_paths {
        check_operation_control(deadline, limits.deadline(), is_cancelled)?;
        if !requires_inspection(&path) {
            retained.insert(path);
            continue;
        }
        let exact = exact_paths
            .exact_components_available(
                &path,
                limits.deadline(),
                deadline,
                is_cancelled,
            )
            .map_err(map_repository_path_inspection_error)?;
        if exact {
            retained.insert(path);
            continue;
        }
        if exact_untracked_aliases
            .get(&ascii_folded_repository_path(&path))
            .is_some_and(|aliases| *aliases == 1)
        {
            continue;
        }
        return Err(GitPathDiscoveryError::InconsistentRepositoryPathSet);
    }
    Ok(retained)
}

fn remove_deleted_cached_paths(
    cached_paths: &mut BTreeSet<RepositoryPath>,
    deleted_paths: impl IntoIterator<Item = RepositoryPath>,
    limits: GitPathDiscoveryLimits,
    deadline: Instant,
    is_cancelled: &mut impl FnMut() -> bool,
) -> Result<(), GitPathDiscoveryError> {
    for deleted_path in deleted_paths {
        check_operation_control(deadline, limits.deadline(), is_cancelled)?;
        if !cached_paths.remove(&deleted_path) {
            return Err(GitPathDiscoveryError::InconsistentRepositoryPathSet);
        }
    }
    Ok(())
}

fn ascii_folded_repository_path(path: &RepositoryPath) -> Vec<u8> {
    path.as_bytes()
        .iter()
        .map(u8::to_ascii_lowercase)
        .collect()
}

fn increment_ascii_alias_count(
    aliases: &mut BTreeMap<Vec<u8>, u64>,
    path: &RepositoryPath,
) -> Result<(), GitPathDiscoveryError> {
    let count = aliases
        .entry(ascii_folded_repository_path(path))
        .or_default();
    *count = count
        .checked_add(1)
        .ok_or(GitPathDiscoveryError::PathCountOverflowed)?;
    Ok(())
}

fn map_exact_session_error(
    error: ExactReadSessionError,
    deadline: Duration,
) -> GitPathDiscoveryError {
    match error {
        ExactReadSessionError::Cancelled => GitPathDiscoveryError::Cancelled,
        ExactReadSessionError::DeadlineExceeded => {
            GitPathDiscoveryError::DeadlineExceeded { deadline }
        }
    }
}

fn map_repository_path_inspection_error(error: ContainedSourceError) -> GitPathDiscoveryError {
    match error {
        ContainedSourceError::Cancelled => GitPathDiscoveryError::Cancelled,
        ContainedSourceError::DeadlineExceeded { deadline } => {
            GitPathDiscoveryError::DeadlineExceeded { deadline }
        }
        source => GitPathDiscoveryError::RepositoryPathInspection { source },
    }
}
