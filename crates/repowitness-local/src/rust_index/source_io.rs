fn requested_artifact_digests(
    sources: &[ImmutableRustSource],
    identities: SourceArtifactIdentities,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<RequestedArtifactDigests, LocalRustIndexError> {
    let mut by_language = BTreeMap::<SourceLanguage, BTreeSet<AnalysisArtifactDigest>>::new();
    for source in sources {
        check_control(cancelled, deadline)?;
        let identity = identities.for_language(source.language());
        let key = AnalysisArtifactKey::new(
            hash_source_content(source.content()),
            identity.producer_manifest(),
            identity.configuration(),
            identity.schema(),
            identity.canonicalization_version(),
        );
        by_language
            .entry(source.language())
            .or_default()
            .insert(hash_analysis_artifact_key(&key));
    }
    Ok(RequestedArtifactDigests {
        by_language: by_language
            .into_iter()
            .map(|(language, digests)| (language, digests.into_iter().collect()))
            .collect(),
    })
}

struct RequestedArtifactDigests {
    by_language: BTreeMap<SourceLanguage, Box<[AnalysisArtifactDigest]>>,
}

fn load_reusable_artifacts(
    requested: &RequestedArtifactDigests,
    deadline: Instant,
    load: &mut impl FnMut(
        SourceLanguage,
        &[AnalysisArtifactDigest],
        Instant,
    ) -> Result<
        BTreeMap<AnalysisArtifactDigest, RustSourceAnalysis>,
        SqliteStoreError,
    >,
) -> Result<BTreeMap<AnalysisArtifactDigest, RustSourceAnalysis>, LocalRustIndexError> {
    let mut reusable = BTreeMap::new();
    for (&language, digests) in &requested.by_language {
        let language_artifacts = load(language, digests, deadline).map_err(map_reuse_error)?;
        for (digest, analysis) in language_artifacts {
            if reusable.insert(digest, analysis).is_some() {
                return Err(LocalRustIndexError::ArtifactReuse {
                    source: SqliteStoreError::IntegrityCheckFailed,
                });
            }
        }
    }
    Ok(reusable)
}

fn map_reuse_error(source: SqliteStoreError) -> LocalRustIndexError {
    match source {
        SqliteStoreError::Cancelled => LocalRustIndexError::Cancelled,
        SqliteStoreError::DeadlineExceeded | SqliteStoreError::ReplyTimeout => {
            LocalRustIndexError::DeadlineExceeded
        }
        source => LocalRustIndexError::ArtifactReuse { source },
    }
}

fn reject_excluded_file_aliases(
    root: &ContainedSourceRoot,
    discovered: &ScopedRepositoryPaths,
    excluded_identity: Option<&FileIdentity>,
    limits: LocalRustIndexLimits,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), LocalRustIndexError> {
    let Some(excluded_identity) = excluded_identity else {
        return Ok(());
    };
    for path in discovered.paths() {
        check_control(cancelled, deadline)?;
        let aliases = root.aliases_identity(
            path,
            excluded_identity,
            limits.deadline(),
            deadline,
            &mut || cancelled.load(Ordering::Relaxed),
        );
        match aliases {
            Ok(true) => return Err(LocalRustIndexError::ExcludedFileAlias),
            Ok(false) => {}
            Err(ContainedSourceError::Cancelled) => {
                return Err(LocalRustIndexError::Cancelled);
            }
            Err(ContainedSourceError::DeadlineExceeded { .. }) => {
                return Err(LocalRustIndexError::DeadlineExceeded);
            }
            Err(_) => {}
        }
    }
    Ok(())
}

fn capture_source_state_for_index(
    worktree_root: &Path,
    limits: GitPathDiscoveryLimits,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<crate::CapturedSourceState, LocalRustIndexError> {
    let limits = capped_discovery_limits(limits, deadline)?;
    capture_source_state_with_cancel(worktree_root, limits, || cancelled.load(Ordering::Relaxed))
        .map_err(|source| match source {
            SourceStateError::Git {
                source: GitPathDiscoveryError::Cancelled,
            } => LocalRustIndexError::Cancelled,
            SourceStateError::Git {
                source: GitPathDiscoveryError::DeadlineExceeded { .. },
            } => LocalRustIndexError::DeadlineExceeded,
            source => LocalRustIndexError::SourceState { source },
        })
}

fn recapture_source_state_for_index(
    worktree_root: &Path,
    limits: GitPathDiscoveryLimits,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<crate::CapturedSourceState, LocalRustIndexError> {
    capture_source_state_for_index(worktree_root, limits, cancelled, deadline).map_err(
        |error| match error {
            LocalRustIndexError::SourceState {
                source:
                    SourceStateError::SparseWorktreeUnsupported
                    | SourceStateError::SubmoduleUnsupported,
            } => LocalRustIndexError::SourceState {
                source: SourceStateError::ConcurrentSourceChange,
            },
            error => error,
        },
    )
}

fn validated_final_source_identity(
    source_state_before: &crate::CapturedSourceState,
    source_state_after: &crate::CapturedSourceState,
    selection: SelectionPolicy,
    manifest: repowitness_domain::SourceManifestDigest,
) -> Result<(GitStateDigest, WorktreeStateDigest), LocalRustIndexError> {
    if source_state_after != source_state_before {
        return Err(LocalRustIndexError::SourceState {
            source: SourceStateError::ConcurrentSourceChange,
        });
    }
    Ok(source_identity_from_state(
        source_state_after,
        selection,
        manifest,
    ))
}

fn source_identity_from_state(
    source_state: &crate::CapturedSourceState,
    selection: SelectionPolicy,
    manifest: repowitness_domain::SourceManifestDigest,
) -> (GitStateDigest, WorktreeStateDigest) {
    let git_state = source_state.git_state();
    let worktree_state = match selection {
        SelectionPolicy::RustOnly => source_state.worktree_state(manifest),
        SelectionPolicy::SupportedLanguages(_) => {
            source_state.source_worktree_state(manifest)
        }
    };
    (git_state, worktree_state)
}

/// Compact allow-list for source languages selected by resolved policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SourceLanguageSelection(u8);

impl SourceLanguageSelection {
    const RUST: u8 = 1;
    const GO: u8 = 1 << 1;
    const TYPESCRIPT: u8 = 1 << 2;
    const TSX: u8 = 1 << 3;
    const PYTHON: u8 = 1 << 4;

    pub(crate) const fn all() -> Self {
        Self(Self::RUST | Self::GO | Self::TYPESCRIPT | Self::TSX | Self::PYTHON)
    }

    pub(crate) fn from_allowed(languages: &BTreeSet<SourceLanguage>) -> Self {
        languages
            .iter()
            .copied()
            .fold(Self(0), |selection, language| {
                Self(selection.0 | Self::bit(language))
            })
    }

    const fn contains(self, language: SourceLanguage) -> bool {
        self.0 & Self::bit(language) != 0
    }

    const fn bit(language: SourceLanguage) -> u8 {
        match language {
            SourceLanguage::Rust => Self::RUST,
            SourceLanguage::Go => Self::GO,
            SourceLanguage::TypeScript => Self::TYPESCRIPT,
            SourceLanguage::Tsx => Self::TSX,
            SourceLanguage::Python => Self::PYTHON,
        }
    }
}

#[derive(Clone, Copy)]
enum SelectionPolicy {
    RustOnly,
    SupportedLanguages(SourceLanguageSelection),
}

struct SelectedRustSources {
    sources: Vec<ImmutableRustSource>,
    counts: SelectedLanguageCounts,
    skipped_policy_paths: u64,
    skipped_unsupported_paths: u64,
}

#[derive(Default)]
struct SelectedLanguageCounts {
    rust: u64,
    go: u64,
    typescript: u64,
    tsx: u64,
    python: u64,
}

impl SelectedLanguageCounts {
    fn record(&mut self, language: SourceLanguage) -> Result<(), LocalRustIndexError> {
        let count = match language {
            SourceLanguage::Rust => &mut self.rust,
            SourceLanguage::Go => &mut self.go,
            SourceLanguage::TypeScript => &mut self.typescript,
            SourceLanguage::Tsx => &mut self.tsx,
            SourceLanguage::Python => &mut self.python,
        };
        *count = count
            .checked_add(1)
            .ok_or(LocalRustIndexError::SourceByteCountOverflowed)?;
        Ok(())
    }

    fn total(&self) -> Result<u64, LocalRustIndexError> {
        [self.rust, self.go, self.typescript, self.tsx, self.python]
            .into_iter()
            .try_fold(0_u64, |total, count| {
                total
                    .checked_add(count)
                    .ok_or(LocalRustIndexError::SourceByteCountOverflowed)
            })
    }
}

fn read_selected_sources(
    root: &ContainedSourceRoot,
    discovered: &ScopedRepositoryPaths,
    selection: SelectionPolicy,
    limits: LocalRustIndexLimits,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<SelectedRustSources, LocalRustIndexError> {
    let mut selected_paths = Vec::new();
    let mut skipped_policy_paths = 0_u64;
    let mut skipped_unsupported_paths = 0_u64;
    for path in discovered.paths() {
        check_control(cancelled, deadline)?;
        match source_language(path.as_bytes()) {
            Some(language) if selection.allows(language) => {
                selected_paths.push((path, language));
            }
            Some(_) if matches!(selection, SelectionPolicy::SupportedLanguages(_)) => {
                increment_count(&mut skipped_policy_paths)?;
            }
            Some(_) | None => increment_count(&mut skipped_unsupported_paths)?,
        }
    }
    let count = u64::try_from(selected_paths.len())
        .map_err(|_| LocalRustIndexError::SourceByteCountOverflowed)?;
    if count > limits.preparation().max_files() {
        return Err(LocalRustIndexError::Preparation {
            source: RustIndexPreparationError::FileLimitExceeded {
                limit: limits.preparation().max_files(),
            },
        });
    }

    let mut sources = Vec::with_capacity(selected_paths.len());
    let mut total_source_bytes = 0_u64;
    let mut counts = SelectedLanguageCounts::default();
    let mut read_session = root
        .exact_read_session(
            selected_paths.iter().map(|(path, _language)| *path),
            deadline,
            || cancelled.load(Ordering::Relaxed),
        )
        .map_err(map_read_plan_error)?;
    for (index, (path, language)) in selected_paths.into_iter().enumerate() {
        check_control(cancelled, deadline)?;
        let ordinal = stable_ordinal(index)?;
        let read_limits = capped_source_read_limits(limits.source_read(), deadline)?;
        let content = read_source(&mut read_session, path, read_limits, cancelled, ordinal)?;
        total_source_bytes = total_source_bytes
            .checked_add(
                u64::try_from(content.len())
                    .map_err(|_| LocalRustIndexError::SourceByteCountOverflowed)?,
            )
            .ok_or(LocalRustIndexError::SourceByteCountOverflowed)?;
        if total_source_bytes > limits.preparation().max_total_source_bytes() {
            return Err(LocalRustIndexError::SourceByteLimitExceeded {
                limit: limits.preparation().max_total_source_bytes(),
            });
        }
        counts.record(language)?;
        sources.push(ImmutableRustSource::for_language(
            path.clone(),
            content,
            language,
        ));
    }
    Ok(SelectedRustSources {
        sources,
        counts,
        skipped_policy_paths,
        skipped_unsupported_paths,
    })
}

fn increment_count(count: &mut u64) -> Result<(), LocalRustIndexError> {
    *count = count
        .checked_add(1)
        .ok_or(LocalRustIndexError::SourceByteCountOverflowed)?;
    Ok(())
}

fn read_source(
    session: &mut crate::contained_source::ExactReadSession<'_>,
    path: &repowitness_domain::RepositoryPath,
    limits: SourceReadLimits,
    cancelled: &AtomicBool,
    ordinal: u64,
) -> Result<Box<[u8]>, LocalRustIndexError> {
    session
        .read_with_cancel(path, limits, || cancelled.load(Ordering::Relaxed))
        .map_err(|source| match source {
            ContainedSourceError::Cancelled => LocalRustIndexError::Cancelled,
            ContainedSourceError::DeadlineExceeded { .. } => LocalRustIndexError::DeadlineExceeded,
            source => LocalRustIndexError::SourceRead { ordinal, source },
        })
}

fn map_read_plan_error(
    error: crate::contained_source::ExactReadSessionError,
) -> LocalRustIndexError {
    match error {
        crate::contained_source::ExactReadSessionError::Cancelled => LocalRustIndexError::Cancelled,
        crate::contained_source::ExactReadSessionError::DeadlineExceeded => {
            LocalRustIndexError::DeadlineExceeded
        }
    }
}

fn discover_paths(
    worktree_root: &Path,
    limits: GitPathDiscoveryLimits,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<DiscoveredRepositoryPaths, LocalRustIndexError> {
    let limits = capped_discovery_limits(limits, deadline)?;
    discover_repository_paths_with_cancel(worktree_root, limits, || {
        cancelled.load(Ordering::Relaxed)
    })
    .map_err(|source| match source {
        GitPathDiscoveryError::Cancelled => LocalRustIndexError::Cancelled,
        GitPathDiscoveryError::DeadlineExceeded { .. } => LocalRustIndexError::DeadlineExceeded,
        source => LocalRustIndexError::Discovery { source },
    })
}

fn discover_tracked_paths(
    worktree_root: &Path,
    limits: GitPathDiscoveryLimits,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<DiscoveredRepositoryPaths, LocalRustIndexError> {
    let limits = capped_discovery_limits(limits, deadline)?;
    discover_cached_repository_paths_with_cancel(worktree_root, limits, || {
        cancelled.load(Ordering::Relaxed)
    })
    .map_err(|source| match source {
        GitPathDiscoveryError::Cancelled => LocalRustIndexError::Cancelled,
        GitPathDiscoveryError::DeadlineExceeded { .. } => LocalRustIndexError::DeadlineExceeded,
        source => LocalRustIndexError::Discovery { source },
    })
}

struct ScopedRepositoryPaths {
    paths: Box<[repowitness_domain::RepositoryPath]>,
    topology_paths: Option<Box<[repowitness_domain::RepositoryPath]>>,
    discovered_paths: u64,
    policy_omitted_paths: u64,
}

impl ScopedRepositoryPaths {
    fn paths(&self) -> &[repowitness_domain::RepositoryPath] {
        &self.paths
    }

    fn topology_paths(&self) -> Option<&[repowitness_domain::RepositoryPath]> {
        self.topology_paths.as_deref()
    }

    const fn discovered_paths(&self) -> u64 {
        self.discovered_paths
    }

    const fn policy_omitted_paths(&self) -> u64 {
        self.policy_omitted_paths
    }
}

fn select_discovered_paths(
    discovered: DiscoveredRepositoryPaths,
    topology_discovered: Option<DiscoveredRepositoryPaths>,
    package_scope: Option<&PackageScope>,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<ScopedRepositoryPaths, LocalRustIndexError> {
    let Some(package_scope) = package_scope else {
        let paths = discovered.into_paths();
        return Ok(ScopedRepositoryPaths {
            discovered_paths: u64::try_from(paths.len())
                .map_err(|_| LocalRustIndexError::SourceByteCountOverflowed)?,
            topology_paths: topology_discovered.map(DiscoveredRepositoryPaths::into_paths),
            paths,
            policy_omitted_paths: 0,
        });
    };
    let selected = crate::package_scope::filter_discovered_repository_paths(
        discovered,
        package_scope,
        cancelled,
        deadline,
    )
    .map_err(|source| match source {
        crate::package_scope::PackageScopeFilterError::Cancelled => LocalRustIndexError::Cancelled,
        crate::package_scope::PackageScopeFilterError::DeadlineExceeded => {
            LocalRustIndexError::DeadlineExceeded
        }
        _ => LocalRustIndexError::PackageScope,
    })?;
    let stats = selected.stats();
    Ok(ScopedRepositoryPaths {
        paths: selected.into_paths(),
        topology_paths: None,
        discovered_paths: stats.discovered_paths(),
        policy_omitted_paths: stats.policy_omitted_paths(),
    })
}

fn revalidate_path_set(
    worktree_root: &Path,
    original: &ScopedRepositoryPaths,
    package_scope: Option<&PackageScope>,
    discovery_limits: GitPathDiscoveryLimits,
    cancelled: &AtomicBool,
    deadline: Instant,
    source_state_before: Option<&CapturedSourceState>,
) -> Result<(), LocalRustIndexError> {
    let current = select_discovered_paths(
        discover_paths(worktree_root, discovery_limits, cancelled, deadline)?,
        original
            .topology_paths()
            .is_some()
            .then(|| discover_tracked_paths(worktree_root, discovery_limits, cancelled, deadline))
            .transpose()?,
        package_scope,
        cancelled,
        deadline,
    )?;
    if current.paths() != original.paths() {
        return Err(LocalRustIndexError::StalePathSet);
    }
    if current.topology_paths() != original.topology_paths() {
        if let Some(source_state_before) = source_state_before {
            let source_state_after = recapture_source_state_for_index(
                worktree_root,
                discovery_limits,
                cancelled,
                deadline,
            )?;
            if source_state_after != *source_state_before {
                return Err(LocalRustIndexError::SourceState {
                    source: SourceStateError::ConcurrentSourceChange,
                });
            }
        }
        return Err(LocalRustIndexError::StalePathSet);
    }
    Ok(())
}

fn revalidate_content(
    root: &ContainedSourceRoot,
    prepared: &PreparedRustIndex,
    source_limits: SourceReadLimits,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), LocalRustIndexError> {
    let mut read_session = root
        .exact_read_session(
            prepared.files().iter().map(|file| file.path()),
            deadline,
            || cancelled.load(Ordering::Relaxed),
        )
        .map_err(map_read_plan_error)?;
    for (index, file) in prepared.files().iter().enumerate() {
        check_control(cancelled, deadline)?;
        let ordinal = stable_ordinal(index)?;
        let read_limits = capped_source_read_limits(source_limits, deadline)?;
        let content = read_session
            .read_with_cancel(file.path(), read_limits, || {
                cancelled.load(Ordering::Relaxed)
            })
            .map_err(|source| match source {
                ContainedSourceError::Cancelled => LocalRustIndexError::Cancelled,
                ContainedSourceError::DeadlineExceeded { .. } => {
                    LocalRustIndexError::DeadlineExceeded
                }
                source => LocalRustIndexError::RevalidationRead { ordinal, source },
            })?;
        if hash_source_content(&content) != file.content_digest() {
            return Err(LocalRustIndexError::StaleSourceContent { ordinal });
        }
    }
    Ok(())
}

fn capped_discovery_limits(
    limits: GitPathDiscoveryLimits,
    deadline: Instant,
) -> Result<GitPathDiscoveryLimits, LocalRustIndexError> {
    let remaining = remaining(deadline)?;
    Ok(GitPathDiscoveryLimits::new(
        limits.deadline().min(remaining),
        limits.output_bytes(),
        limits.paths(),
        limits.repository_path(),
    ))
}

fn capped_source_read_limits(
    limits: SourceReadLimits,
    deadline: Instant,
) -> Result<SourceReadLimits, LocalRustIndexError> {
    let remaining = remaining(deadline)?;
    SourceReadLimits::try_new(
        limits.deadline().min(remaining),
        limits.file_bytes(),
        limits.read_chunk_bytes(),
    )
    .map_err(|source| LocalRustIndexError::DerivedReadLimits { source })
}

fn remaining(deadline: Instant) -> Result<Duration, LocalRustIndexError> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(LocalRustIndexError::DeadlineExceeded);
    }
    Ok(remaining)
}

fn stable_ordinal(index: usize) -> Result<u64, LocalRustIndexError> {
    u64::try_from(index)
        .ok()
        .and_then(|value| value.checked_add(1))
        .ok_or(LocalRustIndexError::SourceByteCountOverflowed)
}

fn check_control(cancelled: &AtomicBool, deadline: Instant) -> Result<(), LocalRustIndexError> {
    if cancelled.load(Ordering::Relaxed) {
        return Err(LocalRustIndexError::Cancelled);
    }
    if Instant::now() >= deadline {
        return Err(LocalRustIndexError::DeadlineExceeded);
    }
    Ok(())
}

fn is_rust_source_path(path: &[u8]) -> bool {
    path.ends_with(b".rs")
}

fn is_go_source_path(path: &[u8]) -> bool {
    path.ends_with(b".go")
}

fn is_typescript_source_path(path: &[u8]) -> bool {
    path.ends_with(b".ts")
}

fn is_tsx_source_path(path: &[u8]) -> bool {
    path.ends_with(b".tsx")
}

fn is_python_source_path(path: &[u8]) -> bool {
    path.ends_with(b".py") || path.ends_with(b".pyi")
}

#[cfg(test)]
fn selected_language(path: &[u8], selection: SelectionPolicy) -> Option<SourceLanguage> {
    source_language(path).filter(|&language| selection.allows(language))
}

fn source_language(path: &[u8]) -> Option<SourceLanguage> {
    let language = if is_rust_source_path(path) {
        SourceLanguage::Rust
    } else if is_go_source_path(path) {
        SourceLanguage::Go
    } else if is_typescript_source_path(path) {
        SourceLanguage::TypeScript
    } else if is_tsx_source_path(path) {
        SourceLanguage::Tsx
    } else if is_python_source_path(path) {
        SourceLanguage::Python
    } else {
        return None;
    };
    Some(language)
}

impl SelectionPolicy {
    const fn allows(self, language: SourceLanguage) -> bool {
        match self {
            SelectionPolicy::RustOnly => matches!(language, SourceLanguage::Rust),
            SelectionPolicy::SupportedLanguages(languages) => languages.contains(language),
        }
    }
}
