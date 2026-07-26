use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use repowitness_analysis::{
    RustAnalysisControl, RustAnalysisError, RustAnalysisLimits, RustSourceAnalysis,
    RustSourceAnalyzer,
};
use repowitness_domain::{
    AnalysisArtifactDigest, AnalysisArtifactKey, AnalysisSchemaDigest, ConfigurationDigest,
    ProducerManifestDigest, RepositoryPath, SourceContentDigest, SourceFileKind, SourceFileLimit,
    SourceManifest, SourceManifestDigest, SourceManifestEntry, SourceManifestError,
};

use crate::{
    CanonicalSourceManifest, hash_analysis_artifact_key, hash_source_content, hash_source_manifest,
};

/// Default maximum number of Rust files in one Phase 0 preparation.
pub const DEFAULT_RUST_INDEX_FILES: u64 = 200_000;
/// Default maximum aggregate immutable Rust source bytes.
pub const DEFAULT_RUST_INDEX_SOURCE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// Default maximum aggregate extracted symbol facts.
pub const DEFAULT_RUST_INDEX_FACTS: u64 = 5_000_000;
/// Hard ceiling for a configured file-count limit.
pub const MAX_RUST_INDEX_FILES: u64 = 1_000_000;
/// Hard ceiling for a configured aggregate source-byte limit.
pub const MAX_RUST_INDEX_SOURCE_BYTES: u64 = 64 * 1024 * 1024 * 1024;
/// Hard ceiling for a configured aggregate fact limit.
pub const MAX_RUST_INDEX_FACTS: u64 = 100_000_000;

/// Immutable source bytes paired with their exact repository identity.
pub struct ImmutableRustSource {
    path: RepositoryPath,
    content: Box<[u8]>,
}

impl ImmutableRustSource {
    /// Constructs one immutable Rust source input.
    #[must_use]
    pub const fn new(path: RepositoryPath, content: Box<[u8]>) -> Self {
        Self { path, content }
    }

    /// Returns the exact repository path.
    #[must_use]
    pub const fn path(&self) -> &RepositoryPath {
        &self.path
    }

    /// Returns the immutable bytes supplied to hashing and analysis.
    #[must_use]
    pub const fn content(&self) -> &[u8] {
        &self.content
    }
}

impl fmt::Debug for ImmutableRustSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ImmutableRustSource")
            .field("path", &self.path)
            .field("content", &"<redacted-source-bytes>")
            .field("content_bytes", &self.content.len())
            .finish()
    }
}

/// Semantics-affecting identities for one Rust analysis producer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RustArtifactIdentity {
    producer_manifest: ProducerManifestDigest,
    configuration: ConfigurationDigest,
    schema: AnalysisSchemaDigest,
    canonicalization_version: u32,
}

impl RustArtifactIdentity {
    /// Constructs the complete identity used by every per-file artifact key.
    #[must_use]
    pub const fn new(
        producer_manifest: ProducerManifestDigest,
        configuration: ConfigurationDigest,
        schema: AnalysisSchemaDigest,
        canonicalization_version: u32,
    ) -> Self {
        Self {
            producer_manifest,
            configuration,
            schema,
            canonicalization_version,
        }
    }

    /// Returns the producer and grammar manifest identity.
    #[must_use]
    pub const fn producer_manifest(self) -> ProducerManifestDigest {
        self.producer_manifest
    }

    /// Returns the semantics-affecting configuration identity.
    #[must_use]
    pub const fn configuration(self) -> ConfigurationDigest {
        self.configuration
    }

    /// Returns the persisted analysis schema identity.
    #[must_use]
    pub const fn schema(self) -> AnalysisSchemaDigest {
        self.schema
    }

    /// Returns the canonical fact-format version.
    #[must_use]
    pub const fn canonicalization_version(self) -> u32 {
        self.canonicalization_version
    }
}

/// Aggregate and per-file resource ceilings for Rust index preparation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RustIndexLimits {
    max_files: u64,
    max_total_source_bytes: u64,
    max_total_facts: u64,
    per_file: RustAnalysisLimits,
}

impl RustIndexLimits {
    /// Constructs validated aggregate preparation limits.
    pub const fn try_new(
        max_files: u64,
        max_total_source_bytes: u64,
        max_total_facts: u64,
        per_file: RustAnalysisLimits,
    ) -> Result<Self, RustIndexLimitError> {
        if max_files > MAX_RUST_INDEX_FILES {
            return Err(RustIndexLimitError::FileLimitTooLarge);
        }
        if max_total_source_bytes > MAX_RUST_INDEX_SOURCE_BYTES {
            return Err(RustIndexLimitError::SourceByteLimitTooLarge);
        }
        if max_total_facts > MAX_RUST_INDEX_FACTS {
            return Err(RustIndexLimitError::FactLimitTooLarge);
        }
        Ok(Self {
            max_files,
            max_total_source_bytes,
            max_total_facts,
            per_file,
        })
    }

    /// Returns the inclusive file-count limit.
    #[must_use]
    pub const fn max_files(self) -> u64 {
        self.max_files
    }

    /// Returns the inclusive aggregate source-byte limit.
    #[must_use]
    pub const fn max_total_source_bytes(self) -> u64 {
        self.max_total_source_bytes
    }

    /// Returns the inclusive aggregate fact limit.
    #[must_use]
    pub const fn max_total_facts(self) -> u64 {
        self.max_total_facts
    }

    /// Returns the per-file syntax analysis limits.
    #[must_use]
    pub const fn per_file(self) -> RustAnalysisLimits {
        self.per_file
    }
}

impl Default for RustIndexLimits {
    fn default() -> Self {
        Self {
            max_files: DEFAULT_RUST_INDEX_FILES,
            max_total_source_bytes: DEFAULT_RUST_INDEX_SOURCE_BYTES,
            max_total_facts: DEFAULT_RUST_INDEX_FACTS,
            per_file: RustAnalysisLimits::default(),
        }
    }
}

/// Invalid Rust index limit configuration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RustIndexLimitError {
    /// The file limit exceeded the Phase 0 hard ceiling.
    FileLimitTooLarge,
    /// The aggregate source-byte limit exceeded the Phase 0 hard ceiling.
    SourceByteLimitTooLarge,
    /// The aggregate fact limit exceeded the Phase 0 hard ceiling.
    FactLimitTooLarge,
}

impl fmt::Display for RustIndexLimitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FileLimitTooLarge => {
                formatter.write_str("Rust index file limit exceeds the Phase 0 maximum")
            }
            Self::SourceByteLimitTooLarge => {
                formatter.write_str("Rust index source-byte limit exceeds the Phase 0 maximum")
            }
            Self::FactLimitTooLarge => {
                formatter.write_str("Rust index fact limit exceeds the Phase 0 maximum")
            }
        }
    }
}

impl Error for RustIndexLimitError {}

/// Complete prepared output for one immutable Rust source file.
#[derive(Eq, PartialEq)]
pub struct PreparedRustFile {
    path: RepositoryPath,
    content_digest: SourceContentDigest,
    artifact_digest: AnalysisArtifactDigest,
    analysis: RustSourceAnalysis,
}

impl PreparedRustFile {
    /// Returns the exact repository path.
    #[must_use]
    pub const fn path(&self) -> &RepositoryPath {
        &self.path
    }

    /// Returns the digest of the exact analyzed source bytes.
    #[must_use]
    pub const fn content_digest(&self) -> SourceContentDigest {
        self.content_digest
    }

    /// Returns the canonical identity of the semantics-complete artifact key.
    #[must_use]
    pub const fn artifact_digest(&self) -> AnalysisArtifactDigest {
        self.artifact_digest
    }

    /// Returns deterministic syntax facts and explicit syntax-error coverage.
    #[must_use]
    pub const fn analysis(&self) -> &RustSourceAnalysis {
        &self.analysis
    }
}

impl fmt::Debug for PreparedRustFile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedRustFile")
            .field("path", &self.path)
            .field("content_digest", &self.content_digest)
            .field("artifact_digest", &self.artifact_digest)
            .field("fact_count", &self.analysis.facts().len())
            .field("visited_nodes", &self.analysis.visited_nodes())
            .field("syntax_error_nodes", &self.analysis.syntax_error_nodes())
            .finish()
    }
}

/// Complete, canonical, bounded Rust index preparation.
#[derive(Eq, PartialEq)]
pub struct PreparedRustIndex {
    manifest_digest: SourceManifestDigest,
    manifest: CanonicalSourceManifest,
    files: Box<[PreparedRustFile]>,
    total_source_bytes: u64,
    total_facts: u64,
    total_syntax_error_nodes: u64,
    reused_files: u64,
    analyzed_files: u64,
}

impl PreparedRustIndex {
    /// Returns the canonical content-manifest identity.
    #[must_use]
    pub const fn manifest_digest(&self) -> SourceManifestDigest {
        self.manifest_digest
    }

    /// Returns the canonical, sorted source manifest.
    #[must_use]
    pub const fn manifest(&self) -> &CanonicalSourceManifest {
        &self.manifest
    }

    /// Returns per-file output in canonical repository-path order.
    #[must_use]
    pub const fn files(&self) -> &[PreparedRustFile] {
        &self.files
    }

    /// Returns the exact aggregate immutable source-byte count.
    #[must_use]
    pub const fn total_source_bytes(&self) -> u64 {
        self.total_source_bytes
    }

    /// Returns the aggregate extracted symbol-fact count.
    #[must_use]
    pub const fn total_facts(&self) -> u64 {
        self.total_facts
    }

    /// Returns the aggregate explicit Tree-sitter error-node count.
    #[must_use]
    pub const fn total_syntax_error_nodes(&self) -> u64 {
        self.total_syntax_error_nodes
    }

    /// Returns files restored from exact, validated analysis artifacts.
    #[must_use]
    pub const fn reused_files(&self) -> u64 {
        self.reused_files
    }

    /// Returns files analyzed by the current syntax producer.
    #[must_use]
    pub const fn analyzed_files(&self) -> u64 {
        self.analyzed_files
    }
}

impl fmt::Debug for PreparedRustIndex {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedRustIndex")
            .field("manifest_digest", &self.manifest_digest)
            .field("file_count", &self.manifest.count())
            .field("total_source_bytes", &self.total_source_bytes)
            .field("total_facts", &self.total_facts)
            .field("total_syntax_error_nodes", &self.total_syntax_error_nodes)
            .field("reused_files", &self.reused_files)
            .field("analyzed_files", &self.analyzed_files)
            .finish()
    }
}

/// Failure to prepare a complete canonical Rust index.
#[derive(Debug)]
pub enum RustIndexPreparationError {
    /// The input file count cannot be represented as a `u64`.
    FileCountNotRepresentable,
    /// The input exceeded the inclusive file-count limit.
    FileLimitExceeded {
        /// Configured inclusive limit.
        limit: u64,
    },
    /// Two immutable inputs had the same exact repository identity.
    DuplicateRepositoryPath,
    /// An aggregate source-byte count overflowed.
    SourceByteCountOverflowed,
    /// Inputs exceeded the inclusive aggregate source-byte limit.
    SourceByteLimitExceeded {
        /// Configured inclusive limit.
        limit: u64,
    },
    /// The operation was cancelled before producing output.
    Cancelled,
    /// The absolute operation deadline elapsed before producing output.
    DeadlineExceeded,
    /// Per-file Rust analysis failed.
    Analysis {
        /// One-based canonical file ordinal.
        ordinal: u64,
        /// Stable redacted analysis failure.
        source: RustAnalysisError,
    },
    /// An aggregate fact count overflowed.
    FactCountOverflowed,
    /// Analysis exceeded the inclusive aggregate fact limit.
    FactLimitExceeded {
        /// Configured inclusive limit.
        limit: u64,
    },
    /// An aggregate syntax-error count overflowed.
    SyntaxErrorCountOverflowed,
    /// Canonical manifest construction failed.
    Manifest {
        /// Stable manifest invariant failure.
        source: SourceManifestError,
    },
}

impl fmt::Display for RustIndexPreparationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FileCountNotRepresentable => {
                formatter.write_str("Rust source file count is not representable")
            }
            Self::FileLimitExceeded { limit } => {
                write!(
                    formatter,
                    "Rust source file count exceeds the limit of {limit}"
                )
            }
            Self::DuplicateRepositoryPath => {
                formatter.write_str("Rust source inputs contain a duplicate repository path")
            }
            Self::SourceByteCountOverflowed => {
                formatter.write_str("Rust source byte count overflowed")
            }
            Self::SourceByteLimitExceeded { limit } => {
                write!(formatter, "Rust source bytes exceed the limit of {limit}")
            }
            Self::Cancelled => formatter.write_str("Rust index preparation was cancelled"),
            Self::DeadlineExceeded => {
                formatter.write_str("Rust index preparation exceeded its deadline")
            }
            Self::Analysis { ordinal, .. } => {
                write!(
                    formatter,
                    "Rust analysis failed for source ordinal {ordinal}"
                )
            }
            Self::FactCountOverflowed => formatter.write_str("Rust fact count overflowed"),
            Self::FactLimitExceeded { limit } => {
                write!(formatter, "Rust facts exceed the limit of {limit}")
            }
            Self::SyntaxErrorCountOverflowed => {
                formatter.write_str("Rust syntax-error count overflowed")
            }
            Self::Manifest { .. } => {
                formatter.write_str("canonical Rust source manifest could not be constructed")
            }
        }
    }
}

impl Error for RustIndexPreparationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Analysis { source, .. } => Some(source),
            Self::FileCountNotRepresentable
            | Self::FileLimitExceeded { .. }
            | Self::DuplicateRepositoryPath
            | Self::SourceByteCountOverflowed
            | Self::SourceByteLimitExceeded { .. }
            | Self::Cancelled
            | Self::DeadlineExceeded
            | Self::FactCountOverflowed
            | Self::FactLimitExceeded { .. }
            | Self::SyntaxErrorCountOverflowed
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
    mut sources: Vec<ImmutableRustSource>,
    identity: RustArtifactIdentity,
    limits: RustIndexLimits,
    reusable: &BTreeMap<AnalysisArtifactDigest, RustSourceAnalysis>,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<PreparedRustIndex, RustIndexPreparationError> {
    let total_source_bytes = validate_and_sort_sources(&mut sources, limits, cancelled, deadline)?;
    let analyzed = analyze_sources(sources, identity, limits, reusable, cancelled, deadline)?;
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
        reused_files: analyzed.reused_files,
        analyzed_files: analyzed.analyzed_files,
    })
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
    reused_files: u64,
    analyzed_files: u64,
}

fn analyze_sources(
    sources: Vec<ImmutableRustSource>,
    identity: RustArtifactIdentity,
    limits: RustIndexLimits,
    reusable: &BTreeMap<AnalysisArtifactDigest, RustSourceAnalysis>,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<AnalyzedRustSources, RustIndexPreparationError> {
    let capacity = sources.len();
    let mut analyzer = None;
    let mut entries = Vec::with_capacity(capacity);
    let mut files = Vec::with_capacity(capacity);
    let mut total_facts = 0_u64;
    let mut total_syntax_error_nodes = 0_u64;
    let mut reused_files = 0_u64;
    let mut analyzed_files = 0_u64;
    let context = SourceAnalysisContext {
        identity,
        limits: limits.per_file(),
        cancelled,
        deadline,
    };

    for (index, source) in sources.into_iter().enumerate() {
        check_control(cancelled, deadline)?;
        let ordinal = stable_ordinal(index)?;
        let analyzed = analyze_source(&mut analyzer, source, reusable, context, ordinal)?;
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
        if analyzed.reused {
            reused_files = reused_files
                .checked_add(1)
                .ok_or(RustIndexPreparationError::FileCountNotRepresentable)?;
        } else {
            analyzed_files = analyzed_files
                .checked_add(1)
                .ok_or(RustIndexPreparationError::FileCountNotRepresentable)?;
        }
        entries.push(analyzed.entry);
        files.push(analyzed.file);
    }

    Ok(AnalyzedRustSources {
        entries,
        files,
        total_facts,
        total_syntax_error_nodes,
        reused_files,
        analyzed_files,
    })
}

struct AnalyzedRustSource {
    entry: SourceManifestEntry<RepositoryPath, SourceFileKind, SourceContentDigest>,
    file: PreparedRustFile,
    fact_count: u64,
    syntax_error_nodes: u64,
    reused: bool,
}

#[derive(Clone, Copy)]
struct SourceAnalysisContext<'a> {
    identity: RustArtifactIdentity,
    limits: RustAnalysisLimits,
    cancelled: &'a AtomicBool,
    deadline: Instant,
}

fn analyze_source(
    analyzer: &mut Option<RustSourceAnalyzer>,
    source: ImmutableRustSource,
    reusable: &BTreeMap<AnalysisArtifactDigest, RustSourceAnalysis>,
    context: SourceAnalysisContext<'_>,
    ordinal: u64,
) -> Result<AnalyzedRustSource, RustIndexPreparationError> {
    let content_digest = hash_source_content(source.content());
    check_control(context.cancelled, context.deadline)?;
    let artifact_key = AnalysisArtifactKey::new(
        content_digest,
        context.identity.producer_manifest(),
        context.identity.configuration(),
        context.identity.schema(),
        context.identity.canonicalization_version(),
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
            if analyzer.is_none() {
                *analyzer =
                    Some(RustSourceAnalyzer::new().map_err(|source| {
                        RustIndexPreparationError::Analysis { ordinal, source }
                    })?);
            }
            let Some(analyzer) = analyzer.as_mut() else {
                return Err(RustIndexPreparationError::Analysis {
                    ordinal,
                    source: RustAnalysisError::GrammarUnavailable,
                });
            };
            let analysis = analyzer
                .analyze(
                    source.content(),
                    context.limits,
                    RustAnalysisControl::new(context.cancelled, context.deadline),
                )
                .map_err(|source| match source {
                    RustAnalysisError::Cancelled => RustIndexPreparationError::Cancelled,
                    RustAnalysisError::DeadlineExceeded => {
                        RustIndexPreparationError::DeadlineExceeded
                    }
                    source => RustIndexPreparationError::Analysis { ordinal, source },
                })?;
            (analysis, false)
        }
    };
    let fact_count = u64::try_from(analysis.facts().len())
        .map_err(|_| RustIndexPreparationError::FactCountOverflowed)?;
    let syntax_error_nodes = u64::from(analysis.syntax_error_nodes());
    let entry =
        SourceManifestEntry::new(source.path.clone(), SourceFileKind::Regular, content_digest);
    let file = PreparedRustFile {
        path: source.path,
        content_digest,
        artifact_digest,
        analysis,
    };
    Ok(AnalyzedRustSource {
        entry,
        file,
        fact_count,
        syntax_error_nodes,
        reused,
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

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;
    use std::time::Duration;

    use repowitness_domain::RepositoryPathLimits;

    use super::*;

    const PATH_LIMITS: RepositoryPathLimits = RepositoryPathLimits::new(1024, 32);

    fn path(bytes: &[u8]) -> RepositoryPath {
        RepositoryPath::try_from_bytes(bytes, PATH_LIMITS)
            .expect("fixture repository path must be valid")
    }

    fn source(path_bytes: &[u8], content: &[u8]) -> ImmutableRustSource {
        ImmutableRustSource::new(path(path_bytes), content.to_vec().into_boxed_slice())
    }

    fn identity() -> RustArtifactIdentity {
        RustArtifactIdentity::new(
            ProducerManifestDigest::new([1; 32]),
            ConfigurationDigest::new([2; 32]),
            AnalysisSchemaDigest::new([3; 32]),
            1,
        )
    }

    fn deadline() -> Instant {
        Instant::now() + Duration::from_secs(1)
    }

    #[test]
    fn unordered_inputs_produce_one_canonical_complete_index() {
        let cancelled = AtomicBool::new(false);
        let prepared = prepare_rust_index(
            vec![
                source(b"src/b.rs", b"fn b() {}\n"),
                source(b"src/a.rs", b"struct A;\n"),
            ],
            identity(),
            RustIndexLimits::default(),
            &cancelled,
            deadline(),
        )
        .expect("valid immutable Rust inputs must prepare");

        assert_eq!(
            prepared
                .files()
                .iter()
                .map(|file| file.path().as_bytes())
                .collect::<Vec<_>>(),
            [b"src/a.rs".as_slice(), b"src/b.rs".as_slice()]
        );
        assert_eq!(prepared.manifest().count().get(), 2);
        assert_eq!(prepared.total_source_bytes(), 20);
        assert_eq!(prepared.total_facts(), 2);
        assert_eq!(prepared.total_syntax_error_nodes(), 0);
        assert_eq!(
            prepared.manifest_digest(),
            hash_source_manifest(prepared.manifest())
        );
        assert!(prepared.files().iter().all(|file| file.artifact_digest()
            == hash_analysis_artifact_key(&AnalysisArtifactKey::new(
                file.content_digest(),
                identity().producer_manifest(),
                identity().configuration(),
                identity().schema(),
                identity().canonicalization_version(),
            ))));
    }

    #[test]
    fn input_order_does_not_change_observable_output() {
        let cancelled = AtomicBool::new(false);
        let forward = prepare_rust_index(
            vec![
                source(b"a.rs", b"fn a() {}\n"),
                source(b"b.rs", b"fn b() {}\n"),
            ],
            identity(),
            RustIndexLimits::default(),
            &cancelled,
            deadline(),
        )
        .expect("forward input must prepare");
        let reverse = prepare_rust_index(
            vec![
                source(b"b.rs", b"fn b() {}\n"),
                source(b"a.rs", b"fn a() {}\n"),
            ],
            identity(),
            RustIndexLimits::default(),
            &cancelled,
            deadline(),
        )
        .expect("reverse input must prepare");

        assert_eq!(forward, reverse);
    }

    #[test]
    fn exact_reuse_matches_clean_output_and_semantic_changes_analyze_only_affected_files() {
        let cancelled = AtomicBool::new(false);
        let clean = prepare_rust_index(
            vec![
                source(b"a.rs", b"fn alpha() {}\n"),
                source(b"b.rs", b"struct Beta;\n"),
            ],
            identity(),
            RustIndexLimits::default(),
            &cancelled,
            deadline(),
        )
        .expect("clean preparation must succeed");
        assert_eq!(clean.reused_files(), 0);
        assert_eq!(clean.analyzed_files(), 2);
        let reusable = clean
            .files()
            .iter()
            .map(|file| (file.artifact_digest(), file.analysis().clone()))
            .collect::<BTreeMap<_, _>>();

        let incremental = prepare_rust_index_with_reuse(
            vec![
                source(b"b.rs", b"struct Beta;\n"),
                source(b"a.rs", b"fn alpha() {}\n"),
            ],
            identity(),
            RustIndexLimits::default(),
            &reusable,
            &cancelled,
            deadline(),
        )
        .expect("exact reusable artifacts must prepare");
        assert_eq!(incremental.manifest(), clean.manifest());
        assert_eq!(incremental.files(), clean.files());
        assert_eq!(incremental.total_facts(), clean.total_facts());
        assert_eq!(incremental.reused_files(), 2);
        assert_eq!(incremental.analyzed_files(), 0);

        let changed = prepare_rust_index_with_reuse(
            vec![
                source(b"a.rs", b"fn alpha() {}\n"),
                source(b"b.rs", b"struct Changed;\n"),
            ],
            identity(),
            RustIndexLimits::default(),
            &reusable,
            &cancelled,
            deadline(),
        )
        .expect("one changed input must prepare");
        assert_eq!(changed.reused_files(), 1);
        assert_eq!(changed.analyzed_files(), 1);

        let changed_identity = RustArtifactIdentity::new(
            ProducerManifestDigest::new([9; 32]),
            identity().configuration(),
            identity().schema(),
            identity().canonicalization_version(),
        );
        let invalidated = prepare_rust_index_with_reuse(
            vec![
                source(b"a.rs", b"fn alpha() {}\n"),
                source(b"b.rs", b"struct Beta;\n"),
            ],
            changed_identity,
            RustIndexLimits::default(),
            &reusable,
            &cancelled,
            deadline(),
        )
        .expect("identity changes must fall back to clean analysis");
        assert_eq!(invalidated.reused_files(), 0);
        assert_eq!(invalidated.analyzed_files(), 2);
    }

    #[test]
    fn reusable_analysis_must_match_the_exact_current_source() {
        let cancelled = AtomicBool::new(false);
        let other = prepare_rust_index(
            vec![source(b"other.rs", b"fn beta() {}\n")],
            identity(),
            RustIndexLimits::default(),
            &cancelled,
            deadline(),
        )
        .expect("fixture analysis must prepare");
        let current_content = b"fn alpha() {}\n";
        let current_key = AnalysisArtifactKey::new(
            hash_source_content(current_content),
            identity().producer_manifest(),
            identity().configuration(),
            identity().schema(),
            identity().canonicalization_version(),
        );
        let reusable = BTreeMap::from([(
            hash_analysis_artifact_key(&current_key),
            other.files()[0].analysis().clone(),
        )]);

        assert!(matches!(
            prepare_rust_index_with_reuse(
                vec![source(b"current.rs", current_content)],
                identity(),
                RustIndexLimits::default(),
                &reusable,
                &cancelled,
                deadline(),
            ),
            Err(RustIndexPreparationError::Analysis {
                source: RustAnalysisError::InvalidAnalysisArtifact,
                ..
            })
        ));
    }

    #[test]
    fn duplicates_and_aggregate_limits_fail_before_partial_output() {
        let cancelled = AtomicBool::new(false);
        assert!(matches!(
            prepare_rust_index(
                vec![
                    source(b"same.rs", b"fn a() {}"),
                    source(b"same.rs", b"fn b() {}")
                ],
                identity(),
                RustIndexLimits::default(),
                &cancelled,
                deadline(),
            ),
            Err(RustIndexPreparationError::DuplicateRepositoryPath)
        ));

        let file_limited = RustIndexLimits::try_new(1, 1024, 100, RustAnalysisLimits::default())
            .expect("fixture limits must be valid");
        assert!(matches!(
            prepare_rust_index(
                vec![source(b"a.rs", b""), source(b"b.rs", b"")],
                identity(),
                file_limited,
                &cancelled,
                deadline(),
            ),
            Err(RustIndexPreparationError::FileLimitExceeded { limit: 1 })
        ));

        let byte_limited = RustIndexLimits::try_new(1, 2, 100, RustAnalysisLimits::default())
            .expect("fixture limits must be valid");
        assert!(matches!(
            prepare_rust_index(
                vec![source(b"a.rs", b"abc")],
                identity(),
                byte_limited,
                &cancelled,
                deadline(),
            ),
            Err(RustIndexPreparationError::SourceByteLimitExceeded { limit: 2 })
        ));

        let fact_limited = RustIndexLimits::try_new(1, 1024, 0, RustAnalysisLimits::default())
            .expect("fixture limits must be valid");
        assert!(matches!(
            prepare_rust_index(
                vec![source(b"a.rs", b"fn a() {}")],
                identity(),
                fact_limited,
                &cancelled,
                deadline(),
            ),
            Err(RustIndexPreparationError::FactLimitExceeded { limit: 0 })
        ));
    }

    #[test]
    fn cancellation_deadline_and_syntax_errors_are_explicit() {
        let cancelled = AtomicBool::new(true);
        assert!(matches!(
            prepare_rust_index(
                vec![source(b"a.rs", b"fn a() {}")],
                identity(),
                RustIndexLimits::default(),
                &cancelled,
                deadline(),
            ),
            Err(RustIndexPreparationError::Cancelled)
        ));
        let not_cancelled = AtomicBool::new(false);
        assert!(matches!(
            prepare_rust_index(
                vec![source(b"a.rs", b"fn a() {}")],
                identity(),
                RustIndexLimits::default(),
                &not_cancelled,
                Instant::now(),
            ),
            Err(RustIndexPreparationError::DeadlineExceeded)
        ));

        let prepared = prepare_rust_index(
            vec![source(b"broken.rs", b"fn broken( { struct Kept;")],
            identity(),
            RustIndexLimits::default(),
            &not_cancelled,
            deadline(),
        )
        .expect("syntax errors must remain a successful explicit analysis outcome");
        assert!(prepared.total_syntax_error_nodes() > 0);
        assert!(prepared.files()[0].analysis().has_syntax_errors());
    }

    #[test]
    fn limit_and_error_diagnostics_are_stable_and_redacted() {
        assert_eq!(
            RustIndexLimits::try_new(
                MAX_RUST_INDEX_FILES + 1,
                1,
                1,
                RustAnalysisLimits::default()
            ),
            Err(RustIndexLimitError::FileLimitTooLarge)
        );
        assert_eq!(
            RustIndexLimits::try_new(
                1,
                MAX_RUST_INDEX_SOURCE_BYTES + 1,
                1,
                RustAnalysisLimits::default()
            ),
            Err(RustIndexLimitError::SourceByteLimitTooLarge)
        );
        assert_eq!(
            RustIndexLimits::try_new(
                1,
                1,
                MAX_RUST_INDEX_FACTS + 1,
                RustAnalysisLimits::default()
            ),
            Err(RustIndexLimitError::FactLimitTooLarge)
        );

        let private = source(b"private-name.rs", b"private source contents");
        let diagnostic = format!("{private:?}");
        assert!(!diagnostic.contains("private-name"));
        assert!(!diagnostic.contains("private source"));

        let cancelled = AtomicBool::new(false);
        let prepared = prepare_rust_index(
            vec![source(b"private-name.rs", b"fn private_symbol_name() {}")],
            identity(),
            RustIndexLimits::default(),
            &cancelled,
            deadline(),
        )
        .expect("redaction fixture must prepare");
        let diagnostic = format!("{prepared:?} {:?}", prepared.files()[0]);
        assert!(!diagnostic.contains("private-name"));
        assert!(!diagnostic.contains("private_symbol_name"));
    }
}
