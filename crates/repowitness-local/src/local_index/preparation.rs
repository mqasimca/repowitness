struct ArtifactReusePreparationContext<'a> {
    worktree: &'a Path,
    database: &'a Path,
    database_identity: Option<&'a FileIdentity>,
    artifacts: SourceArtifactIdentities,
    graph_identity: RustArtifactIdentity,
    languages: SourceLanguageSelection,
    package_scope: Option<&'a PackageScope>,
    limits: LocalRustIndexLimits,
    build_graph: bool,
    cancelled: &'a Arc<AtomicBool>,
    deadline: Instant,
}

fn prepare_with_artifact_reuse(
    context: ArtifactReusePreparationContext<'_>,
) -> Result<crate::LocalRustIndexPreparation, LocalIndexError> {
    let ArtifactReusePreparationContext {
        worktree,
        database,
        database_identity,
        artifacts,
        graph_identity,
        languages,
        package_scope,
        limits,
        build_graph,
        cancelled,
        deadline,
    } = context;
    let reuse_reader = if database.is_file() {
        match OwnedSqliteReader::start(database, deadline) {
            Ok(reader) => Some(reader),
            Err(SqliteStoreError::SchemaVersionMismatch) => None,
            Err(source) => return Err(LocalIndexError::ArtifactReuse { source }),
        }
    } else {
        None
    };
    let reuse_request = match package_scope {
        Some(package_scope) => LocalSourceIndexReuseRequest::new_scoped(
            worktree,
            artifacts,
            graph_identity,
            languages,
            package_scope,
            limits,
            cancelled.as_ref(),
            database_identity,
        ),
        None => LocalSourceIndexReuseRequest::new(
            worktree,
            artifacts,
            languages,
            limits,
            cancelled.as_ref(),
            database_identity,
        ),
    };
    let reuse_request = if build_graph {
        reuse_request
    } else {
        reuse_request.without_graph()
    };
    let preparation = prepare_local_source_index_with_full_reuse_deferred_to_publication(
        reuse_request,
        |language, requested, load_deadline| match &reuse_reader {
            Some(reader) => reader.load_reusable_artifacts_for_language(
                requested,
                language,
                artifacts.for_language(language),
                limits.preparation(),
                Arc::clone(cancelled),
                load_deadline,
            ),
            None => Ok(Default::default()),
        },
        |requested, load_deadline| match &reuse_reader {
            Some(reader) if build_graph => reader.load_reusable_graph_artifacts(
                requested,
                graph_identity,
                limits.preparation(),
                repowitness_analysis::RustGraphAnalysisLimits::DEFAULT,
                Arc::clone(cancelled),
                load_deadline,
            ),
            Some(_) => Ok(Default::default()),
            None => Ok(Default::default()),
        },
        |requested, load_deadline| match &reuse_reader {
            Some(reader) => reader.load_reusable_raw_syntax_artifacts(
                requested,
                raw_syntax_artifact_identities(),
                limits.preparation(),
                repowitness_analysis::RawSyntaxSiteAnalysisLimits::DEFAULT,
                Arc::clone(cancelled),
                load_deadline,
            ),
            None => Ok(Default::default()),
        },
    )
    .map_err(|source| match source {
        LocalRustIndexError::ExcludedFileAlias => LocalIndexError::DatabaseHasMultipleLinks,
        LocalRustIndexError::ArtifactReuse { source } => LocalIndexError::ArtifactReuse { source },
        source => LocalIndexError::Preparation { source },
    })?;
    if let Some(reader) = reuse_reader {
        reader
            .shutdown(deadline)
            .map_err(|source| LocalIndexError::ArtifactReuse { source })?;
    }
    Ok(preparation)
}

struct LocalIndexPublicationPreparationContext<'a> {
    worktree: &'a Path,
    database: &'a Path,
    database_identity: Option<&'a FileIdentity>,
    repository: repowitness_domain::RepositoryIdentityDigest,
    configuration_digest: ConfigurationDigest,
    languages: SourceLanguageSelection,
    limits: LocalRustIndexLimits,
    build_graph: bool,
    cancelled: &'a Arc<AtomicBool>,
    deadline: Instant,
}

fn prepare_local_index_source(
    context: LocalIndexPublicationPreparationContext<'_>,
) -> Result<PreparedLocalIndexSource, LocalIndexError> {
    let artifacts = phase0_local_source_artifact_identities();
    let preparation = prepare_with_artifact_reuse(ArtifactReusePreparationContext {
        worktree: context.worktree,
        database: context.database,
        database_identity: context.database_identity,
        artifacts,
        graph_identity: phase1_rust_graph_artifact_identity(),
        languages: context.languages,
        package_scope: None,
        limits: remaining_preparation_limits(context.limits, context.deadline)?,
        build_graph: context.build_graph,
        cancelled: context.cancelled,
        deadline: context.deadline,
    })?;
    let report_input = ReportInput::from_preparation(&preparation);
    let snapshot_profile =
        phase0_local_source_snapshot_profile(artifacts, context.configuration_digest);
    let identity = RustSourceSnapshotIdentity::new_supported_languages(
        context.repository,
        preparation.git_state(),
        preparation.worktree_state(),
        snapshot_profile.configuration,
        snapshot_profile.producer_manifest,
        snapshot_profile.analysis_schema,
        snapshot_profile.canonicalization_version,
    );
    let coverage = RustIndexCoverage::new(
        report_input.indexed_files,
        report_input.skipped_paths()?,
        report_input.syntax_error_nodes,
        0,
    );
    let confirmed_database_identity = database_alias_identity(context.database)?;
    if confirmed_database_identity.as_ref() != context.database_identity {
        return Err(LocalIndexError::DatabaseChangedDuringIndexing);
    }

    Ok(PreparedLocalIndexSource {
        identity,
        preparation,
        coverage,
        report_input,
        build_graph: context.build_graph,
    })
}

fn prepare_local_index_publication(
    source: PreparedLocalIndexSource,
    repository: repowitness_domain::RepositoryIdentityDigest,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<PreparedLocalIndexPublication, LocalIndexError> {
    let (prepared, graph_artifacts, raw_syntax_artifacts, topology_paths) =
        source.preparation.into_prepared_parts();
    let graph = source
        .build_graph
        .then(|| {
            prepare_local_rust_graph_projection(
                repository,
                &prepared,
                graph_artifacts,
                cancelled,
                deadline,
            )
            .map_err(|source| LocalIndexError::GraphPreparation { source })
        })
        .transpose()?;
    let raw_syntax = prepare_local_raw_syntax_projection(
        raw_syntax_artifacts,
        cancelled,
        deadline,
    )
    .map_err(|source| LocalIndexError::RawSyntaxPreparation { source })?;
    let topology = topology_paths
        .map(|paths| {
            crate::prepare_repository_topology(paths, cancelled, deadline)
        })
        .transpose()
        .map_err(|source| LocalIndexError::RepositoryTopologyPreparation { source })?;
    Ok(PreparedLocalIndexPublication {
        identity: source.identity,
        prepared,
        graph,
        raw_syntax,
        topology,
        coverage: source.coverage,
    })
}

struct ScopedLocalIndexPublicationPreparationContext<'a> {
    worktree: &'a Path,
    database: &'a Path,
    database_identity: Option<&'a FileIdentity>,
    connected_workspace: repowitness_domain::ConnectedWorkspaceId,
    source_slot: repowitness_domain::SourceSlotId,
    repository: repowitness_domain::RepositoryIdentityDigest,
    configuration_digest: ConfigurationDigest,
    languages: SourceLanguageSelection,
    package_scope: &'a PackageScope,
    limits: LocalRustIndexLimits,
    build_graph: bool,
    cancelled: &'a Arc<AtomicBool>,
    deadline: Instant,
}

fn prepare_scoped_local_index_publication(
    context: ScopedLocalIndexPublicationPreparationContext<'_>,
) -> Result<(PreparedLocalIndexPublication, ReportInput), LocalIndexError> {
    let scoped_configuration =
        connected_scope_configuration(context.configuration_digest, context.package_scope);
    let artifacts = connected_scope_source_artifact_identities(scoped_configuration);
    let graph_identity = connected_scope_artifact_identity(
        phase1_rust_graph_artifact_identity(),
        scoped_configuration,
    );
    let preparation = prepare_with_artifact_reuse(ArtifactReusePreparationContext {
        worktree: context.worktree,
        database: context.database,
        database_identity: context.database_identity,
        artifacts,
        graph_identity,
        languages: context.languages,
        package_scope: Some(context.package_scope),
        limits: remaining_preparation_limits(context.limits, context.deadline)?,
        build_graph: context.build_graph,
        cancelled: context.cancelled,
        deadline: context.deadline,
    })?;
    let report_input = ReportInput::from_preparation(&preparation);
    let snapshot_profile = phase0_local_source_snapshot_profile(artifacts, scoped_configuration);
    let identity = RustSourceSnapshotIdentity::new_supported_languages(
        context.repository,
        preparation.git_state(),
        preparation.worktree_state(),
        snapshot_profile.configuration,
        snapshot_profile.producer_manifest,
        snapshot_profile.analysis_schema,
        snapshot_profile.canonicalization_version,
    );
    let coverage = RustIndexCoverage::new(
        report_input.indexed_files,
        report_input.skipped_paths()?,
        report_input.syntax_error_nodes,
        0,
    );
    let confirmed_database_identity = database_alias_identity(context.database)?;
    if confirmed_database_identity.as_ref() != context.database_identity {
        return Err(LocalIndexError::DatabaseChangedDuringIndexing);
    }

    let (prepared, graph_artifacts, raw_syntax_artifacts, _topology_paths) =
        preparation.into_prepared_parts();
    let graph = context
        .build_graph
        .then(|| {
            prepare_local_rust_graph_projection_for_source_slot(
                context.connected_workspace,
                context.source_slot,
                &prepared,
                graph_artifacts,
                context.cancelled.as_ref(),
                context.deadline,
            )
            .map_err(|source| LocalIndexError::GraphPreparation { source })
        })
        .transpose()?;
    let raw_syntax = prepare_local_raw_syntax_projection(
        raw_syntax_artifacts,
        context.cancelled.as_ref(),
        context.deadline,
    )
    .map_err(|source| LocalIndexError::RawSyntaxPreparation { source })?;
    Ok((
        PreparedLocalIndexPublication {
            identity,
            prepared,
            graph,
            raw_syntax,
            topology: None,
            coverage,
        },
        report_input,
    ))
}

fn prepare_local_raw_syntax_projection(
    artifacts: Box<[crate::rust_index::PreparedLocalRawSyntaxArtifact]>,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<crate::PreparedRawSyntaxGeneration, crate::RawSyntaxPreparationError> {
    crate::prepare_raw_syntax_generation(
        artifacts
            .into_vec()
            .into_iter()
            .map(crate::rust_index::PreparedLocalRawSyntaxArtifact::into_parts)
            .collect(),
        crate::RawSyntaxPreparationControl::new(cancelled, deadline),
    )
}

fn connected_scope_configuration(
    configuration: ConfigurationDigest,
    package_scope: &PackageScope,
) -> ConfigurationDigest {
    let mut hasher = Sha256::new();
    hasher.update(CONNECTED_SCOPE_CONFIGURATION_DOMAIN);
    hasher.update(CONNECTED_SCOPE_CONFIGURATION_VERSION.to_be_bytes());
    hasher.update(configuration.as_bytes());
    hasher.update(package_scope.semantic_digest().as_bytes());
    ConfigurationDigest::new(hasher.finalize().into())
}

fn connected_scope_source_artifact_identities(
    scoped_configuration: ConfigurationDigest,
) -> SourceArtifactIdentities {
    let base = phase0_local_source_artifact_identities();
    SourceArtifactIdentities::new(
        connected_scope_artifact_identity(
            base.for_language(SourceLanguage::Rust),
            scoped_configuration,
        ),
        connected_scope_artifact_identity(
            base.for_language(SourceLanguage::Go),
            scoped_configuration,
        ),
        connected_scope_artifact_identity(
            base.for_language(SourceLanguage::TypeScript),
            scoped_configuration,
        ),
        connected_scope_artifact_identity(
            base.for_language(SourceLanguage::Tsx),
            scoped_configuration,
        ),
        connected_scope_artifact_identity(
            base.for_language(SourceLanguage::Python),
            scoped_configuration,
        ),
    )
}

fn connected_scope_artifact_identity(
    base: RustArtifactIdentity,
    scoped_configuration: ConfigurationDigest,
) -> RustArtifactIdentity {
    let mut hasher = Sha256::new();
    hasher.update(CONNECTED_SCOPE_ARTIFACT_CONFIGURATION_DOMAIN);
    hasher.update(CONNECTED_SCOPE_CONFIGURATION_VERSION.to_be_bytes());
    hasher.update(base.configuration().as_bytes());
    hasher.update(scoped_configuration.as_bytes());
    RustArtifactIdentity::new(
        base.producer_manifest(),
        ConfigurationDigest::new(hasher.finalize().into()),
        base.schema(),
        base.canonicalization_version(),
    )
}

fn resolved_index_configuration(
    explicit: Option<&ResolvedConfiguration>,
) -> Result<Cow<'_, ResolvedConfiguration>, LocalIndexError> {
    explicit.map_or_else(
        || {
            resolve_configuration(&[])
                .map(Cow::Owned)
                .map_err(|source| LocalIndexError::ConfigurationResolution { source })
        },
        |configuration| Ok(Cow::Borrowed(configuration)),
    )
}

pub(crate) fn configured_index_inputs(
    limits: LocalRustIndexLimits,
    configuration: &ResolvedConfiguration,
) -> Result<(LocalRustIndexLimits, SourceLanguageSelection), LocalIndexError> {
    let source_read = limits.source_read();
    let source_file_bytes = source_read
        .file_bytes()
        .min(*configuration.policy().max_source_file_bytes().effective());
    let source_read = crate::SourceReadLimits::try_new(
        source_read.deadline(),
        source_file_bytes,
        source_read.read_chunk_bytes(),
    )
    .map_err(|_| LocalIndexError::InvalidEffectiveConfiguration)?;

    let preparation = limits.preparation();
    let max_files = preparation
        .max_files()
        .min(*configuration.policy().max_source_files().effective());
    let preparation = repowitness_application::RustIndexLimits::try_new(
        max_files,
        preparation.max_total_source_bytes(),
        preparation.max_total_facts(),
        preparation.per_file(),
    )
    .map_err(|_| LocalIndexError::InvalidEffectiveConfiguration)?;

    let limits = LocalRustIndexLimits::new(
        limits.deadline(),
        limits.discovery(),
        source_read,
        preparation,
    );
    let languages = SourceLanguageSelection::from_allowed(
        configuration.policy().allowed_languages().effective(),
    );
    Ok((limits, languages))
}

fn remaining_preparation_limits(
    limits: LocalRustIndexLimits,
    deadline: Instant,
) -> Result<LocalRustIndexLimits, LocalIndexError> {
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .filter(|duration| !duration.is_zero())
        .ok_or(LocalIndexError::Preparation {
            source: LocalRustIndexError::DeadlineExceeded,
        })?;
    Ok(LocalRustIndexLimits::new(
        remaining,
        limits.discovery(),
        limits.source_read(),
        limits.preparation(),
    ))
}

pub(crate) fn validated_database_outside_worktree(
    worktree: &Path,
    database: &Path,
) -> Result<std::path::PathBuf, LocalIndexError> {
    let database = match fs::symlink_metadata(database) {
        Ok(_) => {
            fs::canonicalize(database).map_err(|_| LocalIndexError::DatabasePathUnavailable)?
        }
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            let parent = match database.parent() {
                Some(parent) if !parent.as_os_str().is_empty() => parent,
                _ => Path::new("."),
            };
            let parent =
                fs::canonicalize(parent).map_err(|_| LocalIndexError::DatabasePathUnavailable)?;
            let file_name = database
                .file_name()
                .ok_or(LocalIndexError::DatabasePathUnavailable)?;
            parent.join(file_name)
        }
        Err(_) => return Err(LocalIndexError::DatabasePathUnavailable),
    };
    if database.starts_with(worktree) {
        return Err(LocalIndexError::DatabaseInsideWorktree);
    }
    Ok(database)
}

#[cfg(any(unix, windows))]
pub(crate) fn database_alias_identity(
    database: &Path,
) -> Result<Option<FileIdentity>, LocalIndexError> {
    match fs::File::open(database) {
        Ok(file) => {
            let metadata = file
                .metadata()
                .map_err(|_| LocalIndexError::DatabasePathUnavailable)?;
            if !metadata.is_file() {
                return Err(LocalIndexError::DatabasePathUnavailable);
            }
            if !file_has_single_link(&file)
                .map_err(|_| LocalIndexError::DatabasePathUnavailable)?
            {
                return Err(LocalIndexError::DatabaseHasMultipleLinks);
            }
            FileIdentity::from_file(file)
                .map(Some)
                .map_err(|_| LocalIndexError::DatabasePathUnavailable)
        }
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(LocalIndexError::DatabasePathUnavailable),
    }
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn database_alias_identity(
    _database: &Path,
) -> Result<Option<FileIdentity>, LocalIndexError> {
    Ok(None)
}

fn phase0_local_source_artifact_identities() -> SourceArtifactIdentities {
    // Source facts are content-local parser artifacts. Their identities must
    // contain only parser semantics, grammar, schema, and configuration inputs
    // so a snapshot-fencing change does not discard otherwise exact artifacts.
    phase0_source_artifact_identities()
}

#[cfg(test)]
fn phase0_local_rust_artifact_identity() -> RustArtifactIdentity {
    phase0_local_source_artifact_identities().for_language(SourceLanguage::Rust)
}

pub(super) fn local_snapshot_implementation_fingerprint_inputs() -> [&'static [u8]; 13] {
    [
        include_bytes!("../contained_source.rs"),
        include_bytes!("../contained_source/exact_session.rs"),
        include_bytes!("../contained_source/io.rs"),
        include_bytes!("../git_paths.rs"),
        include_bytes!("../git_paths/process.rs"),
        include_bytes!("../rust_index.rs"),
        include_bytes!("../rust_index/source_io.rs"),
        include_bytes!("../rust_index/source_snapshot_fence.rs"),
        include_bytes!("../source_state.rs"),
        include_bytes!("../source_state/parsing.rs"),
        include_bytes!("../local_index.rs"),
        include_bytes!("../local_index/final_fence.rs"),
        include_bytes!("../local_index/preparation.rs"),
    ]
}

#[derive(Clone, Copy)]
struct LocalSourceSnapshotProfile {
    configuration: ConfigurationDigest,
    producer_manifest: ProducerManifestDigest,
    analysis_schema: AnalysisSchemaDigest,
    canonicalization_version: u32,
}

fn phase0_local_source_snapshot_profile(
    artifacts: SourceArtifactIdentities,
    resolved_configuration: ConfigurationDigest,
) -> LocalSourceSnapshotProfile {
    let base = phase0_source_snapshot_profile();
    let mut hasher = Sha256::new();
    hasher.update(LOCAL_SNAPSHOT_PRODUCER_DOMAIN);
    hasher.update(LOCAL_SNAPSHOT_PRODUCER_VERSION.to_be_bytes());
    hasher.update(base.producer_manifest().as_bytes());
    let graph = phase1_rust_graph_artifact_identity();
    hasher.update(graph.producer_manifest().as_bytes());
    hasher.update(graph.configuration().as_bytes());
    hasher.update(graph.schema().as_bytes());
    hasher.update(graph.canonicalization_version().to_be_bytes());
    for input in local_snapshot_implementation_fingerprint_inputs() {
        update_length_prefixed(&mut hasher, input);
    }
    for language in [
        SourceLanguage::Rust,
        SourceLanguage::Go,
        SourceLanguage::TypeScript,
        SourceLanguage::Tsx,
        SourceLanguage::Python,
    ] {
        let artifact = artifacts.for_language(language);
        update_length_prefixed(&mut hasher, language.as_str().as_bytes());
        hasher.update(artifact.producer_manifest().as_bytes());
    }
    let mut configuration_hasher = Sha256::new();
    configuration_hasher.update(LOCAL_SNAPSHOT_CONFIGURATION_DOMAIN);
    configuration_hasher.update(LOCAL_SNAPSHOT_CONFIGURATION_VERSION.to_be_bytes());
    configuration_hasher.update(base.configuration().as_bytes());
    configuration_hasher.update(resolved_configuration.as_bytes());
    LocalSourceSnapshotProfile {
        configuration: ConfigurationDigest::new(configuration_hasher.finalize().into()),
        producer_manifest: ProducerManifestDigest::new(hasher.finalize().into()),
        analysis_schema: base.analysis_schema(),
        canonicalization_version: base.canonicalization_version(),
    }
}

pub(crate) fn local_source_snapshot_configuration(
    resolved_configuration: ConfigurationDigest,
) -> ConfigurationDigest {
    phase0_local_source_snapshot_profile(
        phase0_local_source_artifact_identities(),
        resolved_configuration,
    )
    .configuration
}

fn update_length_prefixed(hasher: &mut Sha256, value: &[u8]) {
    let length = u64::try_from(value.len()).expect("static adapter inputs fit in u64");
    hasher.update(length.to_be_bytes());
    hasher.update(value);
}

#[derive(Clone, Copy)]
struct ReportInput {
    discovered_paths: u64,
    indexed_files: u64,
    indexed_rust_files: u64,
    indexed_go_files: u64,
    indexed_typescript_files: u64,
    indexed_tsx_files: u64,
    indexed_python_files: u64,
    skipped_policy_paths: u64,
    skipped_unsupported_paths: u64,
    total_source_bytes: u64,
    total_facts: u64,
    syntax_error_nodes: u64,
    known_parser_limitation_nodes: u64,
    reused_rust_files: u64,
    analyzed_rust_files: u64,
    reused_go_files: u64,
    analyzed_go_files: u64,
    reused_typescript_files: u64,
    analyzed_typescript_files: u64,
    reused_tsx_files: u64,
    analyzed_tsx_files: u64,
    reused_python_files: u64,
    analyzed_python_files: u64,
}

impl ReportInput {
    fn from_preparation(preparation: &crate::LocalRustIndexPreparation) -> Self {
        Self {
            discovered_paths: preparation.discovered_paths(),
            indexed_files: preparation.prepared().manifest().count().get(),
            indexed_rust_files: preparation.selected_rust_files(),
            indexed_go_files: preparation.selected_go_files(),
            indexed_typescript_files: preparation.selected_typescript_files(),
            indexed_tsx_files: preparation.selected_tsx_files(),
            indexed_python_files: preparation.selected_python_files(),
            skipped_policy_paths: preparation.skipped_policy_paths(),
            skipped_unsupported_paths: preparation.skipped_unsupported_paths(),
            total_source_bytes: preparation.prepared().total_source_bytes(),
            total_facts: preparation.prepared().total_facts(),
            syntax_error_nodes: preparation.prepared().total_syntax_error_nodes(),
            known_parser_limitation_nodes: preparation
                .prepared()
                .total_known_parser_limitation_nodes(),
            reused_rust_files: preparation.prepared().reused_rust_files(),
            analyzed_rust_files: preparation.prepared().analyzed_rust_files(),
            reused_go_files: preparation.prepared().reused_go_files(),
            analyzed_go_files: preparation.prepared().analyzed_go_files(),
            reused_typescript_files: preparation.prepared().reused_typescript_files(),
            analyzed_typescript_files: preparation.prepared().analyzed_typescript_files(),
            reused_tsx_files: preparation.prepared().reused_tsx_files(),
            analyzed_tsx_files: preparation.prepared().analyzed_tsx_files(),
            reused_python_files: preparation.prepared().reused_python_files(),
            analyzed_python_files: preparation.prepared().analyzed_python_files(),
        }
    }

    fn skipped_paths(&self) -> Result<u64, LocalIndexError> {
        self.skipped_policy_paths
            .checked_add(self.skipped_unsupported_paths)
            .ok_or(LocalIndexError::Preparation {
                source: LocalRustIndexError::SourceByteCountOverflowed,
            })
    }
}

fn activated_report(
    generation: GenerationId,
    source_epoch: u64,
    recovered_generations: u64,
    input: ReportInput,
) -> LocalIndexReport {
    LocalIndexReport {
        generation,
        source_epoch,
        recovered_generations,
        discovered_paths: input.discovered_paths,
        indexed_rust_files: input.indexed_rust_files,
        indexed_go_files: input.indexed_go_files,
        indexed_typescript_files: input.indexed_typescript_files,
        indexed_tsx_files: input.indexed_tsx_files,
        indexed_python_files: input.indexed_python_files,
        skipped_policy_paths: input.skipped_policy_paths,
        skipped_unsupported_paths: input.skipped_unsupported_paths,
        total_source_bytes: input.total_source_bytes,
        total_facts: input.total_facts,
        syntax_error_nodes: input.syntax_error_nodes,
        known_parser_limitation_nodes: input.known_parser_limitation_nodes,
        reused_rust_files: input.reused_rust_files,
        analyzed_rust_files: input.analyzed_rust_files,
        reused_go_files: input.reused_go_files,
        analyzed_go_files: input.analyzed_go_files,
        reused_typescript_files: input.reused_typescript_files,
        analyzed_typescript_files: input.analyzed_typescript_files,
        reused_tsx_files: input.reused_tsx_files,
        analyzed_tsx_files: input.analyzed_tsx_files,
        reused_python_files: input.reused_python_files,
        analyzed_python_files: input.analyzed_python_files,
    }
}
