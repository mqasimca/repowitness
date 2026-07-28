impl Error for RustIndexPreparationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Analysis { source, .. } => Some(source),
            Self::FileCountNotRepresentable
            | Self::FileLimitExceeded { .. }
            | Self::DuplicateRepositoryPath
            | Self::UnexpectedLanguage
            | Self::LanguagePathMismatch
            | Self::LanguageArtifactIdentityCollision
            | Self::SourceByteCountOverflowed
            | Self::SourceByteLimitExceeded { .. }
            | Self::Cancelled
            | Self::DeadlineExceeded
            | Self::FactCountOverflowed
            | Self::FactLimitExceeded { .. }
            | Self::SyntaxErrorCountOverflowed
            | Self::KnownParserLimitationCountOverflowed
            | Self::Manifest { .. } => None,
        }
    }
}

/// Prepares deterministic facts and artifact identities from immutable Rust bytes.
pub fn prepare_rust_index(
    sources: Vec<ImmutableRustSource>,
    identity: RustArtifactIdentity,
    limits: RustIndexLimits,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<PreparedRustIndex, RustIndexPreparationError> {
    if sources
        .iter()
        .any(|source| source.language() != SourceLanguage::Rust)
    {
        return Err(RustIndexPreparationError::UnexpectedLanguage);
    }
    prepare_rust_index_with_reuse(
        sources,
        identity,
        limits,
        &BTreeMap::new(),
        cancelled,
        deadline,
    )
}

/// Prepares a deterministic index while reusing only exact validated artifacts.
pub fn prepare_rust_index_with_reuse(
    sources: Vec<ImmutableRustSource>,
    identity: RustArtifactIdentity,
    limits: RustIndexLimits,
    reusable: &BTreeMap<AnalysisArtifactDigest, RustSourceAnalysis>,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<PreparedRustIndex, RustIndexPreparationError> {
    if sources
        .iter()
        .any(|source| source.language() != SourceLanguage::Rust)
    {
        return Err(RustIndexPreparationError::UnexpectedLanguage);
    }
    prepare_source_index_with_reuse(
        sources,
        SourceArtifactIdentities::new(identity, identity, identity, identity, identity),
        limits,
        reusable,
        cancelled,
        deadline,
    )
}

/// Prepares deterministic Go and Rust facts from immutable source bytes.
pub fn prepare_source_index(
    sources: Vec<ImmutableRustSource>,
    identities: SourceArtifactIdentities,
    limits: RustIndexLimits,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<PreparedRustIndex, RustIndexPreparationError> {
    prepare_source_index_with_reuse(
        sources,
        identities,
        limits,
        &BTreeMap::new(),
        cancelled,
        deadline,
    )
}

/// Prepares a mixed-language index with exact validated artifact reuse.
pub fn prepare_source_index_with_reuse(
    mut sources: Vec<ImmutableRustSource>,
    identities: SourceArtifactIdentities,
    limits: RustIndexLimits,
    reusable: &BTreeMap<AnalysisArtifactDigest, RustSourceAnalysis>,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<PreparedRustIndex, RustIndexPreparationError> {
    validate_selected_language_identities(&sources, identities)?;
    let total_source_bytes = validate_and_sort_sources(&mut sources, limits, cancelled, deadline)?;
    let analyzed = analyze_sources(sources, identities, limits, reusable, cancelled, deadline)?;
    check_control(cancelled, deadline)?;
    let manifest =
        SourceManifest::try_from_vec(analyzed.entries, SourceFileLimit::new(limits.max_files()))
            .map_err(|source| RustIndexPreparationError::Manifest { source })?;
    let manifest_digest = hash_source_manifest(&manifest);
    Ok(PreparedRustIndex {
        manifest_digest,
        manifest,
        files: analyzed.files.into_boxed_slice(),
        total_source_bytes,
        total_facts: analyzed.total_facts,
        total_syntax_error_nodes: analyzed.total_syntax_error_nodes,
        total_known_parser_limitation_nodes: analyzed.total_known_parser_limitation_nodes,
        reused_files: analyzed.reused_files,
        analyzed_files: analyzed.analyzed_files,
        indexed_rust_files: analyzed.indexed_rust_files,
        indexed_go_files: analyzed.indexed_go_files,
        indexed_typescript_files: analyzed.indexed_typescript_files,
        indexed_tsx_files: analyzed.indexed_tsx_files,
        indexed_python_files: analyzed.indexed_python_files,
        reused_rust_files: analyzed.reused_rust_files,
        reused_go_files: analyzed.reused_go_files,
        reused_typescript_files: analyzed.reused_typescript_files,
        reused_tsx_files: analyzed.reused_tsx_files,
        reused_python_files: analyzed.reused_python_files,
        analyzed_rust_files: analyzed.analyzed_rust_files,
        analyzed_go_files: analyzed.analyzed_go_files,
        analyzed_typescript_files: analyzed.analyzed_typescript_files,
        analyzed_tsx_files: analyzed.analyzed_tsx_files,
        analyzed_python_files: analyzed.analyzed_python_files,
    })
}

fn validate_selected_language_identities(
    sources: &[ImmutableRustSource],
    identities: SourceArtifactIdentities,
) -> Result<(), RustIndexPreparationError> {
    let languages = sources
        .iter()
        .map(ImmutableRustSource::language)
        .collect::<BTreeSet<_>>();
    for (index, &left) in languages.iter().enumerate() {
        if languages
            .iter()
            .skip(index + 1)
            .any(|&right| identities.for_language(left) == identities.for_language(right))
        {
            return Err(RustIndexPreparationError::LanguageArtifactIdentityCollision);
        }
    }
    Ok(())
}

fn validate_and_sort_sources(
    sources: &mut [ImmutableRustSource],
    limits: RustIndexLimits,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<u64, RustIndexPreparationError> {
    check_control(cancelled, deadline)?;
    let file_count = u64::try_from(sources.len())
        .map_err(|_| RustIndexPreparationError::FileCountNotRepresentable)?;
    if file_count > limits.max_files() {
        return Err(RustIndexPreparationError::FileLimitExceeded {
            limit: limits.max_files(),
        });
    }
    if sources
        .iter()
        .any(|source| !source.language().matches_repository_path(source.path()))
    {
        return Err(RustIndexPreparationError::LanguagePathMismatch);
    }

    sources.sort_unstable_by(|left, right| left.path().cmp(right.path()));
    check_control(cancelled, deadline)?;
    if sources
        .windows(2)
        .any(|pair| pair[0].path() == pair[1].path())
    {
        return Err(RustIndexPreparationError::DuplicateRepositoryPath);
    }

    let total_source_bytes = sources.iter().try_fold(0_u64, |total, source| {
        let source_bytes = u64::try_from(source.content().len())
            .map_err(|_| RustIndexPreparationError::SourceByteCountOverflowed)?;
        total
            .checked_add(source_bytes)
            .ok_or(RustIndexPreparationError::SourceByteCountOverflowed)
    })?;
    if total_source_bytes > limits.max_total_source_bytes() {
        return Err(RustIndexPreparationError::SourceByteLimitExceeded {
            limit: limits.max_total_source_bytes(),
        });
    }
    check_control(cancelled, deadline)?;
    Ok(total_source_bytes)
}

struct AnalyzedRustSources {
    entries: Vec<SourceManifestEntry<RepositoryPath, SourceFileKind, SourceContentDigest>>,
    files: Vec<PreparedRustFile>,
    total_facts: u64,
    total_syntax_error_nodes: u64,
    total_known_parser_limitation_nodes: u64,
    reused_files: u64,
    analyzed_files: u64,
    indexed_rust_files: u64,
    indexed_go_files: u64,
    indexed_typescript_files: u64,
    indexed_tsx_files: u64,
    indexed_python_files: u64,
    reused_rust_files: u64,
    reused_go_files: u64,
    reused_typescript_files: u64,
    reused_tsx_files: u64,
    reused_python_files: u64,
    analyzed_rust_files: u64,
    analyzed_go_files: u64,
    analyzed_typescript_files: u64,
    analyzed_tsx_files: u64,
    analyzed_python_files: u64,
}

#[derive(Default)]
struct AnalyzedLanguageCounts {
    indexed_rust_files: u64,
    indexed_go_files: u64,
    indexed_typescript_files: u64,
    indexed_tsx_files: u64,
    indexed_python_files: u64,
    reused_rust_files: u64,
    reused_go_files: u64,
    reused_typescript_files: u64,
    reused_tsx_files: u64,
    reused_python_files: u64,
    analyzed_rust_files: u64,
    analyzed_go_files: u64,
    analyzed_typescript_files: u64,
    analyzed_tsx_files: u64,
    analyzed_python_files: u64,
}

impl AnalyzedLanguageCounts {
    fn record(
        &mut self,
        language: SourceLanguage,
        reused: bool,
    ) -> Result<(), RustIndexPreparationError> {
        let (indexed, classified) = match (language, reused) {
            (SourceLanguage::Rust, true) => {
                (&mut self.indexed_rust_files, &mut self.reused_rust_files)
            }
            (SourceLanguage::Rust, false) => {
                (&mut self.indexed_rust_files, &mut self.analyzed_rust_files)
            }
            (SourceLanguage::Go, true) => (&mut self.indexed_go_files, &mut self.reused_go_files),
            (SourceLanguage::Go, false) => {
                (&mut self.indexed_go_files, &mut self.analyzed_go_files)
            }
            (SourceLanguage::TypeScript, true) => (
                &mut self.indexed_typescript_files,
                &mut self.reused_typescript_files,
            ),
            (SourceLanguage::TypeScript, false) => (
                &mut self.indexed_typescript_files,
                &mut self.analyzed_typescript_files,
            ),
            (SourceLanguage::Tsx, true) => {
                (&mut self.indexed_tsx_files, &mut self.reused_tsx_files)
            }
            (SourceLanguage::Tsx, false) => {
                (&mut self.indexed_tsx_files, &mut self.analyzed_tsx_files)
            }
            (SourceLanguage::Python, true) => (
                &mut self.indexed_python_files,
                &mut self.reused_python_files,
            ),
            (SourceLanguage::Python, false) => (
                &mut self.indexed_python_files,
                &mut self.analyzed_python_files,
            ),
        };
        *indexed = increment_file_count(*indexed)?;
        *classified = increment_file_count(*classified)?;
        Ok(())
    }
}

fn analyze_sources(
    sources: Vec<ImmutableRustSource>,
    identities: SourceArtifactIdentities,
    limits: RustIndexLimits,
    reusable: &BTreeMap<AnalysisArtifactDigest, RustSourceAnalysis>,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<AnalyzedRustSources, RustIndexPreparationError> {
    let capacity = sources.len();
    let mut analyzers = SourceAnalyzers::default();
    let mut entries = Vec::with_capacity(capacity);
    let mut files = Vec::with_capacity(capacity);
    let mut total_facts = 0_u64;
    let mut total_syntax_error_nodes = 0_u64;
    let mut total_known_parser_limitation_nodes = 0_u64;
    let mut reused_files = 0_u64;
    let mut analyzed_files = 0_u64;
    let mut language_counts = AnalyzedLanguageCounts::default();
    let context = SourceAnalysisContext {
        identities,
        limits: limits.per_file(),
        cancelled,
        deadline,
    };

    for (index, source) in sources.into_iter().enumerate() {
        check_control(cancelled, deadline)?;
        let ordinal = stable_ordinal(index)?;
        let analyzed = analyze_source(&mut analyzers, source, reusable, context, ordinal)?;
        total_facts = total_facts
            .checked_add(analyzed.fact_count)
            .ok_or(RustIndexPreparationError::FactCountOverflowed)?;
        if total_facts > limits.max_total_facts() {
            return Err(RustIndexPreparationError::FactLimitExceeded {
                limit: limits.max_total_facts(),
            });
        }
        total_syntax_error_nodes = total_syntax_error_nodes
            .checked_add(analyzed.syntax_error_nodes)
            .ok_or(RustIndexPreparationError::SyntaxErrorCountOverflowed)?;
        total_known_parser_limitation_nodes = total_known_parser_limitation_nodes
            .checked_add(analyzed.known_parser_limitation_nodes)
            .ok_or(RustIndexPreparationError::KnownParserLimitationCountOverflowed)?;
        if analyzed.reused {
            reused_files = reused_files
                .checked_add(1)
                .ok_or(RustIndexPreparationError::FileCountNotRepresentable)?;
        } else {
            analyzed_files = analyzed_files
                .checked_add(1)
                .ok_or(RustIndexPreparationError::FileCountNotRepresentable)?;
        }
        language_counts.record(analyzed.file.language(), analyzed.reused)?;
        entries.push(analyzed.entry);
        files.push(analyzed.file);
    }

    Ok(AnalyzedRustSources {
        entries,
        files,
        total_facts,
        total_syntax_error_nodes,
        total_known_parser_limitation_nodes,
        reused_files,
        analyzed_files,
        indexed_rust_files: language_counts.indexed_rust_files,
        indexed_go_files: language_counts.indexed_go_files,
        indexed_typescript_files: language_counts.indexed_typescript_files,
        indexed_tsx_files: language_counts.indexed_tsx_files,
        indexed_python_files: language_counts.indexed_python_files,
        reused_rust_files: language_counts.reused_rust_files,
        reused_go_files: language_counts.reused_go_files,
        reused_typescript_files: language_counts.reused_typescript_files,
        reused_tsx_files: language_counts.reused_tsx_files,
        reused_python_files: language_counts.reused_python_files,
        analyzed_rust_files: language_counts.analyzed_rust_files,
        analyzed_go_files: language_counts.analyzed_go_files,
        analyzed_typescript_files: language_counts.analyzed_typescript_files,
        analyzed_tsx_files: language_counts.analyzed_tsx_files,
        analyzed_python_files: language_counts.analyzed_python_files,
    })
}

fn increment_file_count(count: u64) -> Result<u64, RustIndexPreparationError> {
    count
        .checked_add(1)
        .ok_or(RustIndexPreparationError::FileCountNotRepresentable)
}

struct AnalyzedRustSource {
    entry: SourceManifestEntry<RepositoryPath, SourceFileKind, SourceContentDigest>,
    file: PreparedRustFile,
    fact_count: u64,
    syntax_error_nodes: u64,
    known_parser_limitation_nodes: u64,
    reused: bool,
}

#[derive(Clone, Copy)]
struct SourceAnalysisContext<'a> {
    identities: SourceArtifactIdentities,
    limits: RustAnalysisLimits,
    cancelled: &'a AtomicBool,
    deadline: Instant,
}

#[derive(Default)]
struct SourceAnalyzers {
    rust: Option<RustSourceAnalyzer>,
    go: Option<GoSourceAnalyzer>,
    typescript: Option<TypeScriptSourceAnalyzer>,
    tsx: Option<TypeScriptSourceAnalyzer>,
    python: Option<PythonSourceAnalyzer>,
}

fn analyze_source(
    analyzers: &mut SourceAnalyzers,
    source: ImmutableRustSource,
    reusable: &BTreeMap<AnalysisArtifactDigest, RustSourceAnalysis>,
    context: SourceAnalysisContext<'_>,
    ordinal: u64,
) -> Result<AnalyzedRustSource, RustIndexPreparationError> {
    let language = source.language();
    let identity = context.identities.for_language(language);
    let content_digest = hash_source_content(source.content());
    check_control(context.cancelled, context.deadline)?;
    let artifact_key = AnalysisArtifactKey::new(
        content_digest,
        identity.producer_manifest(),
        identity.configuration(),
        identity.schema(),
        identity.canonicalization_version(),
    );
    let artifact_digest = hash_analysis_artifact_key(&artifact_key);
    let (analysis, reused) = match reusable.get(&artifact_digest) {
        Some(analysis) => {
            analysis
                .validate_for_reuse(source.content(), context.limits)
                .map_err(|source| RustIndexPreparationError::Analysis { ordinal, source })?;
            (analysis.clone(), true)
        }
        None => {
            let analysis =
                analyze_fresh_source(analyzers, language, source.content(), context, ordinal)?;
            (analysis, false)
        }
    };
    let fact_count = u64::try_from(analysis.facts().len())
        .map_err(|_| RustIndexPreparationError::FactCountOverflowed)?;
    let syntax_error_nodes = u64::from(analysis.syntax_error_nodes());
    let known_parser_limitation_nodes = u64::from(analysis.known_parser_limitation_nodes());
    let entry =
        SourceManifestEntry::new(source.path.clone(), SourceFileKind::Regular, content_digest);
    let file = PreparedRustFile {
        path: source.path,
        language,
        artifact_identity: identity,
        content_digest,
        artifact_digest,
        analysis,
    };
    Ok(AnalyzedRustSource {
        entry,
        file,
        fact_count,
        syntax_error_nodes,
        known_parser_limitation_nodes,
        reused,
    })
}

fn analyze_fresh_source(
    analyzers: &mut SourceAnalyzers,
    language: SourceLanguage,
    content: &[u8],
    context: SourceAnalysisContext<'_>,
    ordinal: u64,
) -> Result<RustSourceAnalysis, RustIndexPreparationError> {
    let control = RustAnalysisControl::new(context.cancelled, context.deadline);
    let result =
        match language {
            SourceLanguage::Rust => {
                if analyzers.rust.is_none() {
                    analyzers.rust = Some(RustSourceAnalyzer::new().map_err(|source| {
                        RustIndexPreparationError::Analysis { ordinal, source }
                    })?);
                }
                analyzers
                    .rust
                    .as_mut()
                    .ok_or(RustIndexPreparationError::Analysis {
                        ordinal,
                        source: RustAnalysisError::GrammarUnavailable,
                    })?
                    .analyze(content, context.limits, control)
            }
            SourceLanguage::Go => {
                if analyzers.go.is_none() {
                    analyzers.go = Some(GoSourceAnalyzer::new().map_err(|source| {
                        RustIndexPreparationError::Analysis { ordinal, source }
                    })?);
                }
                analyzers
                    .go
                    .as_mut()
                    .ok_or(RustIndexPreparationError::Analysis {
                        ordinal,
                        source: RustAnalysisError::GrammarUnavailable,
                    })?
                    .analyze(content, context.limits, control)
            }
            SourceLanguage::TypeScript => {
                if analyzers.typescript.is_none() {
                    analyzers.typescript = Some(
                        TypeScriptSourceAnalyzer::new(TypeScriptDialect::TypeScript).map_err(
                            |source| RustIndexPreparationError::Analysis { ordinal, source },
                        )?,
                    );
                }
                analyzers
                    .typescript
                    .as_mut()
                    .ok_or(RustIndexPreparationError::Analysis {
                        ordinal,
                        source: RustAnalysisError::GrammarUnavailable,
                    })?
                    .analyze(content, context.limits, control)
            }
            SourceLanguage::Tsx => {
                if analyzers.tsx.is_none() {
                    analyzers.tsx = Some(
                        TypeScriptSourceAnalyzer::new(TypeScriptDialect::Tsx).map_err(
                            |source| RustIndexPreparationError::Analysis { ordinal, source },
                        )?,
                    );
                }
                analyzers
                    .tsx
                    .as_mut()
                    .ok_or(RustIndexPreparationError::Analysis {
                        ordinal,
                        source: RustAnalysisError::GrammarUnavailable,
                    })?
                    .analyze(content, context.limits, control)
            }
            SourceLanguage::Python => {
                if analyzers.python.is_none() {
                    analyzers.python = Some(PythonSourceAnalyzer::new().map_err(|source| {
                        RustIndexPreparationError::Analysis { ordinal, source }
                    })?);
                }
                analyzers
                    .python
                    .as_mut()
                    .ok_or(RustIndexPreparationError::Analysis {
                        ordinal,
                        source: RustAnalysisError::GrammarUnavailable,
                    })?
                    .analyze(content, context.limits, control)
            }
        };
    result.map_err(|source| match source {
        RustAnalysisError::Cancelled => RustIndexPreparationError::Cancelled,
        RustAnalysisError::DeadlineExceeded => RustIndexPreparationError::DeadlineExceeded,
        source => RustIndexPreparationError::Analysis { ordinal, source },
    })
}

fn stable_ordinal(index: usize) -> Result<u64, RustIndexPreparationError> {
    u64::try_from(index)
        .ok()
        .and_then(|value| value.checked_add(1))
        .ok_or(RustIndexPreparationError::FileCountNotRepresentable)
}

fn check_control(
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), RustIndexPreparationError> {
    if cancelled.load(Ordering::Relaxed) {
        return Err(RustIndexPreparationError::Cancelled);
    }
    if Instant::now() >= deadline {
        return Err(RustIndexPreparationError::DeadlineExceeded);
    }
    Ok(())
}
