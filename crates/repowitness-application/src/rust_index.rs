use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use repowitness_analysis::{
    GoSourceAnalyzer, PythonSourceAnalyzer, RustAnalysisControl, RustAnalysisError,
    RustAnalysisLimits, RustSourceAnalysis, RustSourceAnalyzer, TypeScriptDialect,
    TypeScriptSourceAnalyzer,
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
/// Default maximum aggregate immutable supported source bytes.
pub const DEFAULT_RUST_INDEX_SOURCE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// Default maximum aggregate extracted symbol facts.
pub const DEFAULT_RUST_INDEX_FACTS: u64 = 5_000_000;
/// Hard ceiling for a configured file-count limit.
pub const MAX_RUST_INDEX_FILES: u64 = 1_000_000;
/// Hard ceiling for a configured aggregate source-byte limit.
pub const MAX_RUST_INDEX_SOURCE_BYTES: u64 = 64 * 1024 * 1024 * 1024;
/// Hard ceiling for a configured aggregate fact limit.
pub const MAX_RUST_INDEX_FACTS: u64 = 100_000_000;

/// Built-in source languages supported by the Phase 0 index.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SourceLanguage {
    /// Rust source selected by an exact `.rs` extension.
    Rust,
    /// Go source selected by an exact `.go` extension.
    Go,
    /// Plain TypeScript source selected by an exact `.ts` extension.
    TypeScript,
    /// JSX-aware TypeScript source selected by an exact `.tsx` extension.
    Tsx,
    /// Python source selected by an exact `.py` or `.pyi` extension.
    Python,
}

impl SourceLanguage {
    /// Returns the stable persistence and wire spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::Go => "go",
            Self::TypeScript => "typescript",
            Self::Tsx => "tsx",
            Self::Python => "python",
        }
    }

    /// Decodes an exact stable persistence or wire spelling.
    #[must_use]
    pub fn from_stable_str(value: &str) -> Option<Self> {
        match value {
            "rust" => Some(Self::Rust),
            "go" => Some(Self::Go),
            "typescript" => Some(Self::TypeScript),
            "tsx" => Some(Self::Tsx),
            "python" => Some(Self::Python),
            _ => None,
        }
    }

    /// Returns whether this language agrees with the exact repository extension.
    #[must_use]
    pub fn matches_repository_path(self, path: &RepositoryPath) -> bool {
        match self {
            Self::Rust => path.as_bytes().ends_with(b".rs"),
            Self::Go => path.as_bytes().ends_with(b".go"),
            Self::TypeScript => path.as_bytes().ends_with(b".ts"),
            Self::Tsx => path.as_bytes().ends_with(b".tsx"),
            Self::Python => path.as_bytes().ends_with(b".py") || path.as_bytes().ends_with(b".pyi"),
        }
    }
}

/// Immutable source bytes paired with their exact repository identity.
pub struct ImmutableRustSource {
    path: RepositoryPath,
    content: Box<[u8]>,
    language: SourceLanguage,
}

impl ImmutableRustSource {
    /// Constructs one immutable Rust source input.
    #[must_use]
    pub const fn new(path: RepositoryPath, content: Box<[u8]>) -> Self {
        Self {
            path,
            content,
            language: SourceLanguage::Rust,
        }
    }

    /// Constructs one immutable Go source input.
    #[must_use]
    pub const fn new_go(path: RepositoryPath, content: Box<[u8]>) -> Self {
        Self {
            path,
            content,
            language: SourceLanguage::Go,
        }
    }

    /// Constructs one immutable plain TypeScript source input.
    #[must_use]
    pub const fn new_typescript(path: RepositoryPath, content: Box<[u8]>) -> Self {
        Self {
            path,
            content,
            language: SourceLanguage::TypeScript,
        }
    }

    /// Constructs one immutable JSX-aware TypeScript source input.
    #[must_use]
    pub const fn new_tsx(path: RepositoryPath, content: Box<[u8]>) -> Self {
        Self {
            path,
            content,
            language: SourceLanguage::Tsx,
        }
    }

    /// Constructs one immutable Python source input.
    #[must_use]
    pub const fn new_python(path: RepositoryPath, content: Box<[u8]>) -> Self {
        Self {
            path,
            content,
            language: SourceLanguage::Python,
        }
    }

    /// Constructs one immutable supported-language source input.
    #[must_use]
    pub const fn for_language(
        path: RepositoryPath,
        content: Box<[u8]>,
        language: SourceLanguage,
    ) -> Self {
        Self {
            path,
            content,
            language,
        }
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

    /// Returns the parser language selected at the repository-path boundary.
    #[must_use]
    pub const fn language(&self) -> SourceLanguage {
        self.language
    }
}

impl fmt::Debug for ImmutableRustSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ImmutableRustSource")
            .field("path", &self.path)
            .field("language", &self.language)
            .field("content", &"<redacted-source-bytes>")
            .field("content_bytes", &self.content.len())
            .finish()
    }
}

/// Exact independent artifact identities for every built-in language.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceArtifactIdentities {
    rust: RustArtifactIdentity,
    go: RustArtifactIdentity,
    typescript: RustArtifactIdentity,
    tsx: RustArtifactIdentity,
    python: RustArtifactIdentity,
}

impl SourceArtifactIdentities {
    /// Constructs the complete per-language identity set.
    #[must_use]
    pub const fn new(
        rust: RustArtifactIdentity,
        go: RustArtifactIdentity,
        typescript: RustArtifactIdentity,
        tsx: RustArtifactIdentity,
        python: RustArtifactIdentity,
    ) -> Self {
        Self {
            rust,
            go,
            typescript,
            tsx,
            python,
        }
    }

    /// Returns the identity required by one selected source language.
    #[must_use]
    pub const fn for_language(self, language: SourceLanguage) -> RustArtifactIdentity {
        match language {
            SourceLanguage::Rust => self.rust,
            SourceLanguage::Go => self.go,
            SourceLanguage::TypeScript => self.typescript,
            SourceLanguage::Tsx => self.tsx,
            SourceLanguage::Python => self.python,
        }
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

/// Aggregate and per-file resource ceilings for source index preparation.
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

/// Invalid source index limit configuration.
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
                formatter.write_str("source index file limit exceeds the Phase 0 maximum")
            }
            Self::SourceByteLimitTooLarge => {
                formatter.write_str("source index byte limit exceeds the Phase 0 maximum")
            }
            Self::FactLimitTooLarge => {
                formatter.write_str("source index fact limit exceeds the Phase 0 maximum")
            }
        }
    }
}

impl Error for RustIndexLimitError {}

pub(crate) fn source_preparation_implementation_fingerprint_inputs() -> [&'static [u8]; 2] {
    [
        include_bytes!("rust_index/preparation.rs"),
        include_bytes!("rust_index/report.rs"),
    ]
}

/// Complete prepared output for one immutable supported source file.
#[derive(Eq, PartialEq)]
pub struct PreparedRustFile {
    path: RepositoryPath,
    language: SourceLanguage,
    artifact_identity: RustArtifactIdentity,
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

    /// Returns the source language used to produce this file's facts.
    #[must_use]
    pub const fn language(&self) -> SourceLanguage {
        self.language
    }

    /// Returns the exact semantics-complete identity used for this artifact.
    #[must_use]
    pub const fn artifact_identity(&self) -> RustArtifactIdentity {
        self.artifact_identity
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
            .field("language", &self.language)
            .field("content_digest", &self.content_digest)
            .field("artifact_digest", &self.artifact_digest)
            .field("fact_count", &self.analysis.facts().len())
            .field("visited_nodes", &self.analysis.visited_nodes())
            .field("syntax_error_nodes", &self.analysis.syntax_error_nodes())
            .finish()
    }
}

/// Complete, canonical, bounded source index preparation.
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

/// Failure to prepare a complete canonical source index.
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
    /// A Rust-only compatibility entry point received another language.
    UnexpectedLanguage,
    /// A source language did not agree with its exact repository extension.
    LanguagePathMismatch,
    /// Two selected languages were assigned the same artifact identity.
    LanguageArtifactIdentityCollision,
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
    /// Per-file source analysis failed.
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
                formatter.write_str("source file count is not representable")
            }
            Self::FileLimitExceeded { limit } => {
                write!(formatter, "source file count exceeds the limit of {limit}")
            }
            Self::DuplicateRepositoryPath => {
                formatter.write_str("source inputs contain a duplicate repository path")
            }
            Self::UnexpectedLanguage => {
                formatter.write_str("Rust-only index preparation received another language")
            }
            Self::LanguagePathMismatch => {
                formatter.write_str("source language does not match repository path")
            }
            Self::LanguageArtifactIdentityCollision => {
                formatter.write_str("selected source languages share an artifact identity")
            }
            Self::SourceByteCountOverflowed => formatter.write_str("source byte count overflowed"),
            Self::SourceByteLimitExceeded { limit } => {
                write!(formatter, "source bytes exceed the limit of {limit}")
            }
            Self::Cancelled => formatter.write_str("source index preparation was cancelled"),
            Self::DeadlineExceeded => {
                formatter.write_str("source index preparation exceeded its deadline")
            }
            Self::Analysis { ordinal, .. } => {
                write!(
                    formatter,
                    "source analysis failed for source ordinal {ordinal}"
                )
            }
            Self::FactCountOverflowed => formatter.write_str("source fact count overflowed"),
            Self::FactLimitExceeded { limit } => {
                write!(formatter, "source facts exceed the limit of {limit}")
            }
            Self::SyntaxErrorCountOverflowed => {
                formatter.write_str("source syntax-error count overflowed")
            }
            Self::Manifest { .. } => {
                formatter.write_str("canonical source manifest could not be constructed")
            }
        }
    }
}

include!("rust_index/preparation.rs");

mod report;

#[cfg(test)]
mod tests;
