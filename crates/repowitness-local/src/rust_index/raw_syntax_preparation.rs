use std::{
    collections::BTreeMap,
    fmt,
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

use repowitness_analysis::{
    RawSyntaxLanguage, RawSyntaxSiteAnalysis, RawSyntaxSiteAnalysisControl,
    RawSyntaxSiteAnalysisError, RawSyntaxSiteAnalysisLimits, RawSyntaxSiteAnalyzer,
};
use repowitness_application::hash_analysis_artifact_key;
use repowitness_application::{
    CanonicalAnalysisArtifactKey, ImmutableRustSource, SourceArtifactIdentities, SourceLanguage,
    hash_source_content,
};
use repowitness_domain::AnalysisArtifactDigest;
use repowitness_domain::RepositoryPath;

/// One content-local raw all-language syntax-site artifact prepared beside facts.
pub(crate) struct PreparedLocalRawSyntaxArtifact {
    language: SourceLanguage,
    path: RepositoryPath,
    key: CanonicalAnalysisArtifactKey,
    analysis: RawSyntaxSiteAnalysis,
}

impl PreparedLocalRawSyntaxArtifact {
    pub(crate) fn into_parts(
        self,
    ) -> (
        SourceLanguage,
        RepositoryPath,
        CanonicalAnalysisArtifactKey,
        RawSyntaxSiteAnalysis,
    ) {
        (self.language, self.path, self.key, self.analysis)
    }
}

impl fmt::Debug for PreparedLocalRawSyntaxArtifact {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedLocalRawSyntaxArtifact")
            .field("language", &self.language)
            .field("path", &"<redacted-path>")
            .field("site_count", &self.analysis.sites().len())
            .field("syntax_error_nodes", &self.analysis.syntax_error_nodes())
            .finish()
    }
}

pub(super) fn prepare_local_raw_syntax_artifacts(
    sources: &[ImmutableRustSource],
    identities: SourceArtifactIdentities,
    reusable: &BTreeMap<AnalysisArtifactDigest, RawSyntaxSiteAnalysis>,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<Box<[PreparedLocalRawSyntaxArtifact]>, RawSyntaxSiteAnalysisError> {
    let mut sources = sources.iter().collect::<Vec<_>>();
    sources.sort_unstable_by(|left, right| left.path().cmp(right.path()));
    check_control(cancelled, deadline)?;
    let requested = sources
        .iter()
        .map(|source| {
            hash_analysis_artifact_key(&raw_syntax_artifact_key(
                source,
                identities.for_language(source.language()),
            ))
        })
        .collect::<std::collections::BTreeSet<_>>();
    if reusable.keys().any(|digest| !requested.contains(digest)) {
        return Err(RawSyntaxSiteAnalysisError::InvalidSourceSpan);
    }
    let mut analyzers = BTreeMap::new();
    let mut artifacts = Vec::with_capacity(sources.len());
    for source in sources {
        check_control(cancelled, deadline)?;
        let language = raw_language(source.language());
        let key = raw_syntax_artifact_key(source, identities.for_language(source.language()));
        let digest = hash_analysis_artifact_key(&key);
        let analysis = match reusable.get(&digest) {
            Some(analysis) => analysis.clone(),
            None => {
                let analyzer = match analyzers.entry(language) {
                    std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        entry.insert(RawSyntaxSiteAnalyzer::new(language)?)
                    }
                };
                analyzer.analyze(
                    source.content(),
                    RawSyntaxSiteAnalysisLimits::DEFAULT,
                    RawSyntaxSiteAnalysisControl::new(cancelled, deadline),
                )?
            }
        };
        artifacts.push(PreparedLocalRawSyntaxArtifact {
            language: source.language(),
            path: source.path().clone(),
            key,
            analysis,
        });
    }
    check_control(cancelled, deadline)?;
    Ok(artifacts.into_boxed_slice())
}

pub(super) fn requested_local_raw_syntax_artifact_digests(
    sources: &[ImmutableRustSource],
    identities: SourceArtifactIdentities,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<Box<[AnalysisArtifactDigest]>, RawSyntaxSiteAnalysisError> {
    let mut requested = std::collections::BTreeSet::new();
    for source in sources {
        check_control(cancelled, deadline)?;
        requested.insert(hash_analysis_artifact_key(&raw_syntax_artifact_key(
            source,
            identities.for_language(source.language()),
        )));
    }
    check_control(cancelled, deadline)?;
    Ok(requested.into_iter().collect())
}

fn raw_syntax_artifact_key(
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

fn raw_language(language: SourceLanguage) -> RawSyntaxLanguage {
    match language {
        SourceLanguage::Rust => RawSyntaxLanguage::Rust,
        SourceLanguage::Go => RawSyntaxLanguage::Go,
        SourceLanguage::TypeScript => RawSyntaxLanguage::TypeScript,
        SourceLanguage::Tsx => RawSyntaxLanguage::Tsx,
        SourceLanguage::Python => RawSyntaxLanguage::Python,
    }
}

fn check_control(
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), RawSyntaxSiteAnalysisError> {
    if cancelled.load(Ordering::Acquire) {
        Err(RawSyntaxSiteAnalysisError::Cancelled)
    } else if Instant::now() >= deadline {
        Err(RawSyntaxSiteAnalysisError::DeadlineExceeded)
    } else {
        Ok(())
    }
}
