use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    sync::atomic::AtomicBool,
    time::Instant,
};

use repowitness_analysis::{
    RustGraphAnalysisControl, RustGraphAnalysisError, RustGraphAnalysisLimits,
    RustGraphSiteAnalysis, RustGraphSiteAnalyzer,
};
use repowitness_application::{
    CanonicalAnalysisArtifactKey, ImmutableRustSource, SourceLanguage, hash_analysis_artifact_key,
    hash_source_content,
};
use repowitness_domain::{AnalysisArtifactDigest, RepositoryPath};

/// One content-local raw Rust graph-site artifact prepared beside source facts.
pub(crate) struct PreparedLocalRustGraphArtifact {
    path: RepositoryPath,
    key: CanonicalAnalysisArtifactKey,
    analysis: RustGraphSiteAnalysis,
}

impl PreparedLocalRustGraphArtifact {
    #[cfg(test)]
    pub(crate) const fn path(&self) -> &RepositoryPath {
        &self.path
    }

    #[cfg(test)]
    pub(crate) const fn key(&self) -> CanonicalAnalysisArtifactKey {
        self.key
    }

    #[cfg(test)]
    pub(crate) const fn analysis(&self) -> &RustGraphSiteAnalysis {
        &self.analysis
    }

    pub(crate) fn into_parts(
        self,
    ) -> (
        RepositoryPath,
        CanonicalAnalysisArtifactKey,
        RustGraphSiteAnalysis,
    ) {
        (self.path, self.key, self.analysis)
    }
}

impl fmt::Debug for PreparedLocalRustGraphArtifact {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedLocalRustGraphArtifact")
            .field("path", &"<redacted-path>")
            .field("site_count", &self.analysis.sites().len())
            .field("syntax_error_nodes", &self.analysis.syntax_error_nodes())
            .finish()
    }
}

pub(super) fn prepare_local_rust_graph_artifacts(
    sources: &[ImmutableRustSource],
    identity: repowitness_application::RustArtifactIdentity,
    reusable: &BTreeMap<AnalysisArtifactDigest, RustGraphSiteAnalysis>,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<Box<[PreparedLocalRustGraphArtifact]>, RustGraphAnalysisError> {
    let mut rust_sources = sources
        .iter()
        .filter(|source| source.language() == SourceLanguage::Rust)
        .collect::<Vec<_>>();
    rust_sources.sort_unstable_by(|left, right| left.path().cmp(right.path()));
    check_control(cancelled, deadline)?;
    if rust_sources.is_empty() {
        if !reusable.is_empty() {
            return Err(RustGraphAnalysisError::InvalidAnalysisShape);
        }
        return Ok(Box::new([]));
    }

    let requested = rust_sources
        .iter()
        .map(|source| hash_analysis_artifact_key(&graph_artifact_key(source, identity)))
        .collect::<BTreeSet<_>>();
    if reusable.keys().any(|digest| !requested.contains(digest)) {
        return Err(RustGraphAnalysisError::InvalidAnalysisShape);
    }
    let mut analyzer = None;
    let mut artifacts = Vec::with_capacity(rust_sources.len());
    for source in rust_sources {
        check_control(cancelled, deadline)?;
        let key = graph_artifact_key(source, identity);
        let digest = hash_analysis_artifact_key(&key);
        let analysis = match reusable.get(&digest) {
            Some(analysis) => analysis.clone(),
            None => {
                if analyzer.is_none() {
                    analyzer = Some(RustGraphSiteAnalyzer::new()?);
                }
                analyzer
                    .as_mut()
                    .ok_or(RustGraphAnalysisError::GrammarUnavailable)?
                    .analyze(
                        source.content(),
                        RustGraphAnalysisLimits::DEFAULT,
                        RustGraphAnalysisControl::new(cancelled, deadline),
                    )?
            }
        };
        artifacts.push(PreparedLocalRustGraphArtifact {
            path: source.path().clone(),
            key,
            analysis,
        });
    }
    check_control(cancelled, deadline)?;
    Ok(artifacts.into_boxed_slice())
}

pub(super) fn requested_local_rust_graph_artifact_digests(
    sources: &[ImmutableRustSource],
    identity: repowitness_application::RustArtifactIdentity,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<Box<[AnalysisArtifactDigest]>, RustGraphAnalysisError> {
    let mut requested = BTreeSet::new();
    for source in sources
        .iter()
        .filter(|source| source.language() == SourceLanguage::Rust)
    {
        check_control(cancelled, deadline)?;
        requested.insert(hash_analysis_artifact_key(&graph_artifact_key(
            source, identity,
        )));
    }
    check_control(cancelled, deadline)?;
    Ok(requested.into_iter().collect())
}

fn graph_artifact_key(
    source: &ImmutableRustSource,
    identity: repowitness_application::RustArtifactIdentity,
) -> CanonicalAnalysisArtifactKey {
    CanonicalAnalysisArtifactKey::new(
        hash_source_content(source.content()),
        identity.producer_manifest(),
        identity.configuration(),
        identity.schema(),
        identity.canonicalization_version(),
    )
}

fn check_control(cancelled: &AtomicBool, deadline: Instant) -> Result<(), RustGraphAnalysisError> {
    if cancelled.load(std::sync::atomic::Ordering::Acquire) {
        return Err(RustGraphAnalysisError::Cancelled);
    }
    if Instant::now() >= deadline {
        return Err(RustGraphAnalysisError::DeadlineExceeded);
    }
    Ok(())
}
