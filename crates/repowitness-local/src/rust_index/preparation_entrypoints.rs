/// Runs the Phase 0 local Rust discovery-to-facts vertical slice.
pub fn prepare_local_rust_index(
    requested_root: &Path,
    identity: RustArtifactIdentity,
    limits: LocalRustIndexLimits,
    cancelled: &AtomicBool,
) -> Result<LocalRustIndexPreparation, LocalRustIndexError> {
    prepare_local_rust_index_with_hook(requested_root, identity, limits, cancelled, || {})
}

/// Runs the local mixed supported-language discovery-to-facts vertical slice.
pub fn prepare_local_source_index(
    requested_root: &Path,
    identities: SourceArtifactIdentities,
    limits: LocalRustIndexLimits,
    cancelled: &AtomicBool,
) -> Result<LocalRustIndexPreparation, LocalRustIndexError> {
    prepare_local_source_index_with_exclusion_reuse_and_hook(
        LocalSourceIndexReuseRequest::new(
            requested_root,
            identities,
            SourceLanguageSelection::all(),
            limits,
            cancelled,
            None,
        ),
        |_, _, _| Ok(BTreeMap::new()),
        |_, _| Ok(BTreeMap::new()),
        || {},
    )
}

#[cfg(test)]
pub(crate) fn prepare_local_source_index_excluding_identity_with_reuse(
    requested_root: &Path,
    identities: SourceArtifactIdentities,
    languages: SourceLanguageSelection,
    limits: LocalRustIndexLimits,
    cancelled: &AtomicBool,
    excluded_identity: Option<&FileIdentity>,
    load_reusable: impl FnMut(
        SourceLanguage,
        &[AnalysisArtifactDigest],
        Instant,
    ) -> Result<
        BTreeMap<AnalysisArtifactDigest, RustSourceAnalysis>,
        SqliteStoreError,
    >,
) -> Result<LocalRustIndexPreparation, LocalRustIndexError> {
    prepare_local_source_index_excluding_identity_with_full_reuse(
        LocalSourceIndexReuseRequest::new(
            requested_root,
            identities,
            languages,
            limits,
            cancelled,
            excluded_identity,
        ),
        load_reusable,
        |_, _| Ok(BTreeMap::new()),
    )
}

pub(crate) struct LocalSourceIndexReuseRequest<'a> {
    requested_root: &'a Path,
    identities: SourceArtifactIdentities,
    graph_identity: RustArtifactIdentity,
    languages: SourceLanguageSelection,
    package_scope: Option<&'a PackageScope>,
    limits: LocalRustIndexLimits,
    cancelled: &'a AtomicBool,
    excluded_identity: Option<&'a FileIdentity>,
}

impl<'a> LocalSourceIndexReuseRequest<'a> {
    pub(crate) fn new(
        requested_root: &'a Path,
        identities: SourceArtifactIdentities,
        languages: SourceLanguageSelection,
        limits: LocalRustIndexLimits,
        cancelled: &'a AtomicBool,
        excluded_identity: Option<&'a FileIdentity>,
    ) -> Self {
        Self {
            requested_root,
            identities,
            graph_identity: phase1_rust_graph_artifact_identity(),
            languages,
            package_scope: None,
            limits,
            cancelled,
            excluded_identity,
        }
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "scoped source, graph, policy, control, and exclusion inputs remain explicit"
    )]
    pub(crate) const fn new_scoped(
        requested_root: &'a Path,
        identities: SourceArtifactIdentities,
        graph_identity: RustArtifactIdentity,
        languages: SourceLanguageSelection,
        package_scope: &'a PackageScope,
        limits: LocalRustIndexLimits,
        cancelled: &'a AtomicBool,
        excluded_identity: Option<&'a FileIdentity>,
    ) -> Self {
        Self {
            requested_root,
            identities,
            graph_identity,
            languages,
            package_scope: Some(package_scope),
            limits,
            cancelled,
            excluded_identity,
        }
    }
}

pub(crate) fn prepare_local_source_index_excluding_identity_with_full_reuse(
    request: LocalSourceIndexReuseRequest<'_>,
    load_reusable: impl FnMut(
        SourceLanguage,
        &[AnalysisArtifactDigest],
        Instant,
    ) -> Result<
        BTreeMap<AnalysisArtifactDigest, RustSourceAnalysis>,
        SqliteStoreError,
    >,
    load_reusable_graph: impl FnMut(
        &[AnalysisArtifactDigest],
        Instant,
    ) -> Result<
        BTreeMap<AnalysisArtifactDigest, RustGraphSiteAnalysis>,
        SqliteStoreError,
    >,
) -> Result<LocalRustIndexPreparation, LocalRustIndexError> {
    prepare_local_source_index_with_exclusion_reuse_and_hook(
        request,
        load_reusable,
        load_reusable_graph,
        || {},
    )
}

fn prepare_local_rust_index_with_hook(
    requested_root: &Path,
    identity: RustArtifactIdentity,
    limits: LocalRustIndexLimits,
    cancelled: &AtomicBool,
    before_revalidation: impl FnMut(),
) -> Result<LocalRustIndexPreparation, LocalRustIndexError> {
    prepare_local_rust_index_with_exclusion_and_hook(
        requested_root,
        identity,
        limits,
        cancelled,
        None,
        before_revalidation,
    )
}

fn prepare_local_rust_index_with_exclusion_and_hook(
    requested_root: &Path,
    identity: RustArtifactIdentity,
    limits: LocalRustIndexLimits,
    cancelled: &AtomicBool,
    excluded_identity: Option<&FileIdentity>,
    before_revalidation: impl FnMut(),
) -> Result<LocalRustIndexPreparation, LocalRustIndexError> {
    prepare_local_rust_index_with_exclusion_reuse_and_hook(
        requested_root,
        identity,
        limits,
        cancelled,
        excluded_identity,
        |_, _| Ok(BTreeMap::new()),
        before_revalidation,
    )
}

fn prepare_local_rust_index_with_exclusion_reuse_and_hook(
    requested_root: &Path,
    identity: RustArtifactIdentity,
    limits: LocalRustIndexLimits,
    cancelled: &AtomicBool,
    excluded_identity: Option<&FileIdentity>,
    mut load_reusable: impl FnMut(
        &[AnalysisArtifactDigest],
        Instant,
    ) -> Result<
        BTreeMap<AnalysisArtifactDigest, RustSourceAnalysis>,
        SqliteStoreError,
    >,
    before_revalidation: impl FnMut(),
) -> Result<LocalRustIndexPreparation, LocalRustIndexError> {
    prepare_local_index_with_exclusion_reuse_and_hook(
        LocalPreparationContext {
            requested_root,
            identities: SourceArtifactIdentities::new(
                identity, identity, identity, identity, identity,
            ),
            graph_identity: phase1_rust_graph_artifact_identity(),
            selection: SelectionPolicy::RustOnly,
            package_scope: None,
            limits,
            cancelled,
            excluded_identity,
        },
        |_, requested, deadline| load_reusable(requested, deadline),
        |_, _| Ok(BTreeMap::new()),
        before_revalidation,
    )
}

fn prepare_local_source_index_with_exclusion_reuse_and_hook(
    request: LocalSourceIndexReuseRequest<'_>,
    load_reusable: impl FnMut(
        SourceLanguage,
        &[AnalysisArtifactDigest],
        Instant,
    ) -> Result<
        BTreeMap<AnalysisArtifactDigest, RustSourceAnalysis>,
        SqliteStoreError,
    >,
    load_reusable_graph: impl FnMut(
        &[AnalysisArtifactDigest],
        Instant,
    ) -> Result<
        BTreeMap<AnalysisArtifactDigest, RustGraphSiteAnalysis>,
        SqliteStoreError,
    >,
    before_revalidation: impl FnMut(),
) -> Result<LocalRustIndexPreparation, LocalRustIndexError> {
    let LocalSourceIndexReuseRequest {
        requested_root,
        identities,
        graph_identity,
        languages,
        package_scope,
        limits,
        cancelled,
        excluded_identity,
    } = request;
    prepare_local_index_with_exclusion_reuse_and_hook(
        LocalPreparationContext {
            requested_root,
            identities,
            graph_identity,
            selection: SelectionPolicy::SupportedLanguages(languages),
            package_scope,
            limits,
            cancelled,
            excluded_identity,
        },
        load_reusable,
        load_reusable_graph,
        before_revalidation,
    )
}
