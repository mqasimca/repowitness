//! Immutable all-language raw syntax-site artifacts and generation receipts.

use std::{
    error::Error,
    fmt,
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

use repowitness_analysis::{
    RAW_SYNTAX_SITE_PROFILE_VERSION, RawSyntaxSiteAnalysis, RawSyntaxSiteKind,
};
use repowitness_application::{
    CanonicalAnalysisArtifactKey, SourceLanguage, hash_analysis_artifact_key,
};
use repowitness_domain::{AnalysisArtifactDigest, RepositoryPath};
use sha2::{Digest, Sha256};

const ARTIFACT_PAYLOAD_VERSION: u16 = 1;

/// Stable failure while preparing one raw syntax-site projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RawSyntaxPreparationError {
    /// Artifact inputs contain a duplicate, wrong language, or invalid ordinal.
    InvalidArtifacts,
    /// A count or owned-byte total was not representable.
    CountOverflow,
    /// Cancellation was observed before complete projection output existed.
    Cancelled,
    /// The monotonic deadline elapsed before complete output existed.
    DeadlineExceeded,
}

impl fmt::Display for RawSyntaxPreparationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidArtifacts => "raw syntax-site artifacts are invalid",
            Self::CountOverflow => "raw syntax-site count overflowed",
            Self::Cancelled => "raw syntax-site preparation cancelled",
            Self::DeadlineExceeded => "raw syntax-site preparation deadline exceeded",
        })
    }
}

impl Error for RawSyntaxPreparationError {}

/// Cooperative control for raw syntax-site projection preparation.
#[derive(Clone, Copy)]
pub struct RawSyntaxPreparationControl<'a> {
    cancelled: &'a AtomicBool,
    deadline: Instant,
}

impl<'a> RawSyntaxPreparationControl<'a> {
    /// Creates a cancellation and monotonic-deadline control boundary.
    #[must_use]
    pub const fn new(cancelled: &'a AtomicBool, deadline: Instant) -> Self {
        Self {
            cancelled,
            deadline,
        }
    }

    fn check(self) -> Result<(), RawSyntaxPreparationError> {
        if self.cancelled.load(Ordering::Acquire) {
            Err(RawSyntaxPreparationError::Cancelled)
        } else if Instant::now() >= self.deadline {
            Err(RawSyntaxPreparationError::DeadlineExceeded)
        } else {
            Ok(())
        }
    }
}

impl fmt::Debug for RawSyntaxPreparationControl<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RawSyntaxPreparationControl")
            .field("cancelled", &self.cancelled.load(Ordering::Acquire))
            .field("deadline", &"<monotonic>")
            .finish()
    }
}

/// One reusable raw syntax-site artifact at one exact path/language occurrence.
#[derive(Clone, Eq, PartialEq)]
pub struct PreparedRawSyntaxArtifact {
    language: SourceLanguage,
    path: RepositoryPath,
    key: CanonicalAnalysisArtifactKey,
    artifact_digest: AnalysisArtifactDigest,
    payload_digest: [u8; 32],
    analysis: RawSyntaxSiteAnalysis,
}

impl PreparedRawSyntaxArtifact {
    /// Returns the selected source language/dialect.
    #[must_use]
    pub const fn language(&self) -> SourceLanguage {
        self.language
    }

    /// Returns the exact repository-relative source path.
    #[must_use]
    pub const fn path(&self) -> &RepositoryPath {
        &self.path
    }

    /// Returns every semantics-affecting reusable artifact-key input.
    #[must_use]
    pub const fn key(&self) -> CanonicalAnalysisArtifactKey {
        self.key
    }

    /// Returns the canonical immutable artifact identity.
    #[must_use]
    pub const fn artifact_digest(&self) -> AnalysisArtifactDigest {
        self.artifact_digest
    }

    /// Returns the canonical immutable payload digest.
    #[must_use]
    pub const fn payload_digest(&self) -> &[u8; 32] {
        &self.payload_digest
    }

    /// Returns complete bounded raw syntax-site analysis.
    #[must_use]
    pub const fn analysis(&self) -> &RawSyntaxSiteAnalysis {
        &self.analysis
    }
}

impl fmt::Debug for PreparedRawSyntaxArtifact {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedRawSyntaxArtifact")
            .field("language", &self.language)
            .field("path", &self.path)
            .field("artifact_digest", &self.artifact_digest)
            .field("payload_digest", &"<redacted-digest>")
            .field("site_count", &self.analysis.sites().len())
            .finish()
    }
}

/// Complete raw syntax-site projection ready for staging at one generation.
#[derive(Clone, Eq, PartialEq)]
pub struct PreparedRawSyntaxGeneration {
    artifacts: Box<[PreparedRawSyntaxArtifact]>,
    site_count: u64,
    syntax_error_nodes: u64,
    visited_nodes: u64,
    owned_text_bytes: u64,
    import_sites: u64,
    reference_sites: u64,
    call_sites: u64,
    test_marker_sites: u64,
}

impl PreparedRawSyntaxGeneration {
    /// Returns artifacts in canonical `(path, language)` order.
    #[must_use]
    pub const fn artifacts(&self) -> &[PreparedRawSyntaxArtifact] {
        &self.artifacts
    }

    /// Returns total raw observations.
    #[must_use]
    pub const fn site_count(&self) -> u64 {
        self.site_count
    }
    /// Returns explicit parser syntax-error nodes.
    #[must_use]
    pub const fn syntax_error_nodes(&self) -> u64 {
        self.syntax_error_nodes
    }
    /// Returns total visited syntax nodes.
    #[must_use]
    pub const fn visited_nodes(&self) -> u64 {
        self.visited_nodes
    }
    /// Returns total artifact-owned target bytes.
    #[must_use]
    pub const fn owned_text_bytes(&self) -> u64 {
        self.owned_text_bytes
    }
    /// Returns raw import sites.
    #[must_use]
    pub const fn import_sites(&self) -> u64 {
        self.import_sites
    }
    /// Returns raw reference candidates.
    #[must_use]
    pub const fn reference_sites(&self) -> u64 {
        self.reference_sites
    }
    /// Returns raw call sites.
    #[must_use]
    pub const fn call_sites(&self) -> u64 {
        self.call_sites
    }
    /// Returns raw test-marker sites.
    #[must_use]
    pub const fn test_marker_sites(&self) -> u64 {
        self.test_marker_sites
    }
}

impl fmt::Debug for PreparedRawSyntaxGeneration {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedRawSyntaxGeneration")
            .field("artifact_count", &self.artifacts.len())
            .field("site_count", &self.site_count)
            .field("syntax_error_nodes", &self.syntax_error_nodes)
            .finish()
    }
}

/// Validates and canonically assembles raw-site artifacts for one generation.
pub fn prepare_raw_syntax_generation(
    inputs: Vec<(
        SourceLanguage,
        RepositoryPath,
        CanonicalAnalysisArtifactKey,
        RawSyntaxSiteAnalysis,
    )>,
    control: RawSyntaxPreparationControl<'_>,
) -> Result<PreparedRawSyntaxGeneration, RawSyntaxPreparationError> {
    control.check()?;
    let mut artifacts = Vec::with_capacity(inputs.len());
    for (language, path, key, analysis) in inputs {
        control.check()?;
        if analysis.language().as_str() != language.as_str() {
            return Err(RawSyntaxPreparationError::InvalidArtifacts);
        }
        validate_ordinals(&analysis, control)?;
        artifacts.push(PreparedRawSyntaxArtifact {
            language,
            path,
            artifact_digest: hash_analysis_artifact_key(&key),
            payload_digest: artifact_payload_digest(&analysis, control)?,
            key,
            analysis,
        });
    }
    artifacts.sort_unstable_by(|left, right| {
        (left.path.as_bytes(), left.language).cmp(&(right.path.as_bytes(), right.language))
    });
    if artifacts
        .windows(2)
        .any(|pair| pair[0].path == pair[1].path)
    {
        return Err(RawSyntaxPreparationError::InvalidArtifacts);
    }
    let mut totals = ProjectionTotals::default();
    for artifact in &artifacts {
        control.check()?;
        totals.add(artifact)?;
    }
    control.check()?;
    Ok(PreparedRawSyntaxGeneration {
        artifacts: artifacts.into_boxed_slice(),
        site_count: totals.site_count,
        syntax_error_nodes: totals.syntax_error_nodes,
        visited_nodes: totals.visited_nodes,
        owned_text_bytes: totals.owned_text_bytes,
        import_sites: totals.import_sites,
        reference_sites: totals.reference_sites,
        call_sites: totals.call_sites,
        test_marker_sites: totals.test_marker_sites,
    })
}

#[derive(Default)]
struct ProjectionTotals {
    site_count: u64,
    syntax_error_nodes: u64,
    visited_nodes: u64,
    owned_text_bytes: u64,
    import_sites: u64,
    reference_sites: u64,
    call_sites: u64,
    test_marker_sites: u64,
}

impl ProjectionTotals {
    fn add(
        &mut self,
        artifact: &PreparedRawSyntaxArtifact,
    ) -> Result<(), RawSyntaxPreparationError> {
        let analysis = artifact.analysis();
        self.site_count = add(
            self.site_count,
            u64::try_from(analysis.sites().len())
                .map_err(|_| RawSyntaxPreparationError::CountOverflow)?,
        )?;
        self.syntax_error_nodes = add(
            self.syntax_error_nodes,
            u64::from(analysis.syntax_error_nodes()),
        )?;
        self.visited_nodes = add(self.visited_nodes, u64::from(analysis.visited_nodes()))?;
        self.owned_text_bytes = add(self.owned_text_bytes, analysis.owned_text_bytes())?;
        for site in analysis.sites() {
            match site.kind() {
                RawSyntaxSiteKind::Import => self.import_sites = add(self.import_sites, 1)?,
                RawSyntaxSiteKind::Reference => {
                    self.reference_sites = add(self.reference_sites, 1)?
                }
                RawSyntaxSiteKind::Call => self.call_sites = add(self.call_sites, 1)?,
                RawSyntaxSiteKind::TestMarker => {
                    self.test_marker_sites = add(self.test_marker_sites, 1)?
                }
            }
        }
        Ok(())
    }
}

fn add(left: u64, right: u64) -> Result<u64, RawSyntaxPreparationError> {
    left.checked_add(right)
        .ok_or(RawSyntaxPreparationError::CountOverflow)
}

fn validate_ordinals(
    analysis: &RawSyntaxSiteAnalysis,
    control: RawSyntaxPreparationControl<'_>,
) -> Result<(), RawSyntaxPreparationError> {
    for (index, site) in analysis.sites().iter().enumerate() {
        control.check()?;
        if site.ordinal().get()
            != u32::try_from(index).map_err(|_| RawSyntaxPreparationError::CountOverflow)?
        {
            return Err(RawSyntaxPreparationError::InvalidArtifacts);
        }
    }
    Ok(())
}

pub(super) fn artifact_payload_digest(
    analysis: &RawSyntaxSiteAnalysis,
    control: RawSyntaxPreparationControl<'_>,
) -> Result<[u8; 32], RawSyntaxPreparationError> {
    control.check()?;
    let mut hash = Sha256::new();
    hash.update(b"repowitness:raw-syntax-site-payload\0");
    hash.update(ARTIFACT_PAYLOAD_VERSION.to_be_bytes());
    hash.update(RAW_SYNTAX_SITE_PROFILE_VERSION.to_be_bytes());
    put_text(&mut hash, analysis.language().as_str());
    hash.update(analysis.visited_nodes().to_be_bytes());
    hash.update(analysis.syntax_error_nodes().to_be_bytes());
    hash.update(analysis.max_observed_depth().to_be_bytes());
    hash.update(analysis.owned_text_bytes().to_be_bytes());
    for kind in [
        RawSyntaxSiteKind::Import,
        RawSyntaxSiteKind::Reference,
        RawSyntaxSiteKind::Call,
        RawSyntaxSiteKind::TestMarker,
    ] {
        let coverage = analysis.coverage().for_kind(kind);
        put_text(&mut hash, kind.as_str());
        put_text(
            &mut hash,
            match coverage.support() {
                repowitness_analysis::RawSyntaxSiteSupport::Available => "available",
                repowitness_analysis::RawSyntaxSiteSupport::Unsupported => "unsupported",
            },
        );
        hash.update(coverage.emitted().to_be_bytes());
    }
    put_len(&mut hash, analysis.sites().len());
    for site in analysis.sites() {
        control.check()?;
        hash.update(site.ordinal().get().to_be_bytes());
        put_text(&mut hash, site.kind().as_str());
        put_text(&mut hash, site.evidence().as_str());
        put_span(&mut hash, site.occurrence_span());
        put_span(&mut hash, site.target_span());
        put_text(&mut hash, site.raw_target());
    }
    control.check()?;
    Ok(hash.finalize().into())
}

fn put_span(hash: &mut Sha256, span: repowitness_domain::ByteSpan) {
    hash.update(span.start().get().to_be_bytes());
    hash.update(span.end().get().to_be_bytes());
}

fn put_text(hash: &mut Sha256, value: &str) {
    put_bytes(hash, value.as_bytes());
}

fn put_bytes(hash: &mut Sha256, value: &[u8]) {
    hash.update(u64::try_from(value.len()).unwrap_or(u64::MAX).to_be_bytes());
    hash.update(value);
}

fn put_len(hash: &mut Sha256, value: usize) {
    hash.update(u64::try_from(value).unwrap_or(u64::MAX).to_be_bytes());
}
