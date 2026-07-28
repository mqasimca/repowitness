fn prepare_with_artifact_reuse(
    worktree: &Path,
    database: &Path,
    database_identity: Option<&FileIdentity>,
    artifacts: SourceArtifactIdentities,
    preparation_limits: LocalRustIndexLimits,
    cancelled: &Arc<AtomicBool>,
    deadline: Instant,
) -> Result<crate::LocalRustIndexPreparation, LocalIndexError> {
    let reuse_reader = if database.is_file() {
        match OwnedSqliteReader::start(database, deadline) {
            Ok(reader) => Some(reader),
            Err(SqliteStoreError::SchemaVersionMismatch) => None,
            Err(source) => return Err(LocalIndexError::ArtifactReuse { source }),
        }
    } else {
        None
    };
    let preparation = prepare_local_source_index_excluding_identity_with_reuse(
        worktree,
        artifacts,
        preparation_limits,
        cancelled.as_ref(),
        database_identity,
        |language, requested, load_deadline| match &reuse_reader {
            Some(reader) => reader.load_reusable_artifacts_for_language(
                requested,
                language,
                artifacts.for_language(language),
                preparation_limits.preparation(),
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

#[cfg(unix)]
pub(crate) fn database_alias_identity(
    database: &Path,
) -> Result<Option<FileIdentity>, LocalIndexError> {
    use std::os::unix::fs::MetadataExt;

    match fs::metadata(database) {
        Ok(metadata) if !metadata.is_file() => Err(LocalIndexError::DatabasePathUnavailable),
        Ok(metadata) if metadata.nlink() > 1 => Err(LocalIndexError::DatabaseHasMultipleLinks),
        Ok(_) => FileIdentity::from_path(database)
            .map(Some)
            .map_err(|_| LocalIndexError::DatabasePathUnavailable),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(LocalIndexError::DatabasePathUnavailable),
    }
}

#[cfg(windows)]
pub(crate) fn database_alias_identity(
    database: &Path,
) -> Result<Option<FileIdentity>, LocalIndexError> {
    match fs::metadata(database) {
        Ok(metadata) if metadata.is_file() => FileIdentity::from_path(database)
            .map(Some)
            .map_err(|_| LocalIndexError::DatabasePathUnavailable),
        Ok(_) => Err(LocalIndexError::DatabasePathUnavailable),
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
    let base = phase0_source_artifact_identities();
    SourceArtifactIdentities::new(
        extend_local_artifact_identity(
            SourceLanguage::Rust,
            base.for_language(SourceLanguage::Rust),
        ),
        extend_local_artifact_identity(SourceLanguage::Go, base.for_language(SourceLanguage::Go)),
        extend_local_artifact_identity(
            SourceLanguage::TypeScript,
            base.for_language(SourceLanguage::TypeScript),
        ),
        extend_local_artifact_identity(SourceLanguage::Tsx, base.for_language(SourceLanguage::Tsx)),
        extend_local_artifact_identity(
            SourceLanguage::Python,
            base.for_language(SourceLanguage::Python),
        ),
    )
}

#[cfg(test)]
fn phase0_local_rust_artifact_identity() -> RustArtifactIdentity {
    phase0_local_source_artifact_identities().for_language(SourceLanguage::Rust)
}

fn extend_local_artifact_identity(
    language: SourceLanguage,
    base: RustArtifactIdentity,
) -> RustArtifactIdentity {
    let mut hasher = Sha256::new();
    hasher.update(LOCAL_PRODUCER_DOMAIN);
    hasher.update(LOCAL_PRODUCER_VERSION.to_be_bytes());
    update_length_prefixed(&mut hasher, language.as_str().as_bytes());
    hasher.update(base.producer_manifest().as_bytes());
    for input in local_producer_implementation_fingerprint_inputs() {
        update_length_prefixed(&mut hasher, input);
    }
    RustArtifactIdentity::new(
        ProducerManifestDigest::new(hasher.finalize().into()),
        base.configuration(),
        base.schema(),
        base.canonicalization_version(),
    )
}

pub(super) fn local_producer_implementation_fingerprint_inputs() -> [&'static [u8]; 11] {
    [
        include_bytes!("../contained_source.rs"),
        include_bytes!("../contained_source/exact_session.rs"),
        include_bytes!("../contained_source/io.rs"),
        include_bytes!("../git_paths.rs"),
        include_bytes!("../git_paths/process.rs"),
        include_bytes!("../rust_index.rs"),
        include_bytes!("../rust_index/source_io.rs"),
        include_bytes!("../source_state.rs"),
        include_bytes!("../source_state/parsing.rs"),
        include_bytes!("../local_index.rs"),
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
) -> LocalSourceSnapshotProfile {
    let base = phase0_source_snapshot_profile();
    let mut hasher = Sha256::new();
    hasher.update(LOCAL_SNAPSHOT_PRODUCER_DOMAIN);
    hasher.update(LOCAL_PRODUCER_VERSION.to_be_bytes());
    hasher.update(base.producer_manifest().as_bytes());
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
    LocalSourceSnapshotProfile {
        configuration: base.configuration(),
        producer_manifest: ProducerManifestDigest::new(hasher.finalize().into()),
        analysis_schema: base.analysis_schema(),
        canonicalization_version: base.canonicalization_version(),
    }
}

fn update_length_prefixed(hasher: &mut Sha256, value: &[u8]) {
    let length = u64::try_from(value.len()).expect("static adapter inputs fit in u64");
    hasher.update(length.to_be_bytes());
    hasher.update(value);
}

struct ReportInput {
    discovered_paths: u64,
    indexed_files: u64,
    indexed_rust_files: u64,
    indexed_go_files: u64,
    indexed_typescript_files: u64,
    indexed_tsx_files: u64,
    indexed_python_files: u64,
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
