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
    discovered: &DiscoveredRepositoryPaths,
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

#[derive(Clone, Copy)]
enum SelectionPolicy {
    RustOnly,
    SupportedLanguages,
}

struct SelectedRustSources {
    sources: Vec<ImmutableRustSource>,
    counts: SelectedLanguageCounts,
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
        [
            self.rust,
            self.go,
            self.typescript,
            self.tsx,
            self.python,
        ]
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
    discovered: &DiscoveredRepositoryPaths,
    selection: SelectionPolicy,
    limits: LocalRustIndexLimits,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<SelectedRustSources, LocalRustIndexError> {
    let selected_paths = discovered
        .paths()
        .iter()
        .filter_map(|path| {
            selected_language(path.as_bytes(), selection).map(|language| (path, language))
        })
        .collect::<Vec<_>>();
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
        let content = read_source(
            &mut read_session,
            path,
            read_limits,
            cancelled,
            ordinal,
        )?;
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
    Ok(SelectedRustSources { sources, counts })
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

fn revalidate_path_set(
    worktree_root: &Path,
    original: &DiscoveredRepositoryPaths,
    discovery_limits: GitPathDiscoveryLimits,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), LocalRustIndexError> {
    let current = discover_paths(worktree_root, discovery_limits, cancelled, deadline)?;
    if current.paths() != original.paths() {
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

fn selected_language(path: &[u8], selection: SelectionPolicy) -> Option<SourceLanguage> {
    if is_rust_source_path(path) {
        Some(SourceLanguage::Rust)
    } else if matches!(selection, SelectionPolicy::SupportedLanguages) {
        if is_go_source_path(path) {
            Some(SourceLanguage::Go)
        } else if is_typescript_source_path(path) {
            Some(SourceLanguage::TypeScript)
        } else if is_tsx_source_path(path) {
            Some(SourceLanguage::Tsx)
        } else if is_python_source_path(path) {
            Some(SourceLanguage::Python)
        } else {
            None
        }
    } else {
        None
    }
}
