//! Deterministic planning for immutable analysis-artifact reuse.

use core::fmt;
use std::collections::BTreeSet;

use repowitness_domain::{AnalysisArtifactKey, SourceManifest, SourceManifestEntry};

type PlannedEntries<P, D, A, C, S, V> = Box<[PlannedAnalysisArtifact<P, D, A, C, S, V>]>;

/// Per-entry semantic inputs other than the source digest.
///
/// `A` is the complete adapter, grammar, and producer identity, `C` the
/// resolved semantics-affecting configuration, `S` the extraction schema
/// identity, and `V` the canonicalization version.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArtifactKeySemantics<A, C, S, V> {
    analyzer_identity: A,
    configuration_identity: C,
    schema_identity: S,
    canonicalization_version: V,
}

impl<A, C, S, V> ArtifactKeySemantics<A, C, S, V> {
    /// Creates already-validated per-entry semantic inputs.
    #[must_use]
    pub const fn new(
        analyzer_identity: A,
        configuration_identity: C,
        schema_identity: S,
        canonicalization_version: V,
    ) -> Self {
        Self {
            analyzer_identity,
            configuration_identity,
            schema_identity,
            canonicalization_version,
        }
    }

    /// Returns the complete adapter, grammar, and producer identity.
    #[must_use]
    pub const fn analyzer_identity(&self) -> &A {
        &self.analyzer_identity
    }

    /// Returns the resolved semantics-affecting configuration identity.
    #[must_use]
    pub const fn configuration_identity(&self) -> &C {
        &self.configuration_identity
    }

    /// Returns the extraction schema identity.
    #[must_use]
    pub const fn schema_identity(&self) -> &S {
        &self.schema_identity
    }

    /// Returns the canonicalization version.
    #[must_use]
    pub const fn canonicalization_version(&self) -> &V {
        &self.canonicalization_version
    }
}

/// Whether one source entry can reuse an existing immutable analysis artifact.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArtifactPlanAction {
    /// A verified immutable artifact with the complete logical key exists.
    Reuse,
    /// The complete logical key is not available and analysis must run.
    Analyze,
}

/// A fixed-width count of entries in an artifact-reuse plan.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArtifactPlanCount(u64);

impl ArtifactPlanCount {
    /// Returns the fixed-width count.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// One canonically ordered analysis-artifact decision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlannedAnalysisArtifact<P, D, A, C, S, V> {
    path: P,
    key: AnalysisArtifactKey<D, A, C, S, V>,
    action: ArtifactPlanAction,
}

impl<P, D, A, C, S, V> PlannedAnalysisArtifact<P, D, A, C, S, V> {
    /// Returns the validated repository-relative path.
    #[must_use]
    pub const fn path(&self) -> &P {
        &self.path
    }

    /// Returns the complete logical artifact key.
    #[must_use]
    pub const fn key(&self) -> &AnalysisArtifactKey<D, A, C, S, V> {
        &self.key
    }

    /// Returns whether this entry reuses an artifact or requires analysis.
    #[must_use]
    pub const fn action(&self) -> ArtifactPlanAction {
        self.action
    }
}

/// A complete bounded artifact-reuse plan in canonical source-path order.
///
/// The plan owns only paths and logical keys. It does not contain source
/// content or persisted artifacts and performs no I/O.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArtifactReusePlan<P, D, A, C, S, V> {
    entries: PlannedEntries<P, D, A, C, S, V>,
    reuse_count: ArtifactPlanCount,
    analysis_count: ArtifactPlanCount,
}

impl<P, D, A, C, S, V> ArtifactReusePlan<P, D, A, C, S, V> {
    /// Returns decisions in canonical source-path order.
    #[must_use]
    pub fn as_slice(&self) -> &[PlannedAnalysisArtifact<P, D, A, C, S, V>] {
        &self.entries
    }

    /// Returns the number of entries that can reuse verified artifacts.
    #[must_use]
    pub const fn reuse_count(&self) -> ArtifactPlanCount {
        self.reuse_count
    }

    /// Returns the number of entries that require analysis.
    #[must_use]
    pub const fn analysis_count(&self) -> ArtifactPlanCount {
        self.analysis_count
    }

    /// Returns whether the plan contains no source entries.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.reuse_count.get() == 0 && self.analysis_count.get() == 0
    }
}

/// A cooperative stop observed while planning artifact reuse.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArtifactPlanningError {
    /// The owning operation was cancelled.
    Cancelled,
    /// The owning operation's deadline elapsed.
    DeadlineExceeded,
}

impl fmt::Display for ArtifactPlanningError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => formatter.write_str("artifact planning was cancelled"),
            Self::DeadlineExceeded => formatter.write_str("artifact planning deadline exceeded"),
        }
    }
}

impl std::error::Error for ArtifactPlanningError {}

/// Plans reusable and missing immutable analysis artifacts.
///
/// `manifest` already bounds the traversal and supplies canonical source-path
/// order. `reusable_keys` is a caller-bounded inventory containing only
/// artifacts whose persisted bytes and producer metadata have been verified
/// by the owning boundary.
/// `semantics_for` supplies the already-validated per-entry analyzer,
/// configuration, schema, and canonicalization inputs. The planner always
/// takes the source digest directly from the manifest, so a caller cannot
/// accidentally omit changed content from the key. `control` returns a stop
/// reason when cancellation or a deadline is observed. Both callbacks must be
/// bounded, deterministic, and free of I/O.
///
/// This Phase 0 slice expects every supplied entry to be eligible for an
/// analyzer. It does not silently filter unsupported file types; generation
/// assembly must classify those entries and account for them in coverage
/// before invoking this planner.
///
/// The function performs no I/O, checks `control` before allocation and after
/// every entry, and returns no partial plan. Runtime is `O(n log m)`, where `n`
/// is the bounded manifest size and `m` is the reusable-key inventory size.
///
/// # Errors
///
/// Returns the first cooperative stop reported by `control`.
pub fn plan_artifact_reuse<P, K, D, A, C, S, V, SemanticsFor, Control>(
    manifest: &SourceManifest<P, K, D>,
    reusable_keys: &BTreeSet<AnalysisArtifactKey<D, A, C, S, V>>,
    mut semantics_for: SemanticsFor,
    mut control: Control,
) -> Result<ArtifactReusePlan<P, D, A, C, S, V>, ArtifactPlanningError>
where
    P: Clone,
    D: Clone + Ord,
    A: Ord,
    C: Ord,
    S: Ord,
    V: Ord,
    SemanticsFor: FnMut(&SourceManifestEntry<P, K, D>) -> ArtifactKeySemantics<A, C, S, V>,
    Control: FnMut() -> Option<ArtifactPlanningError>,
{
    check_control(&mut control)?;

    let mut entries = Vec::with_capacity(manifest.as_slice().len());
    let mut reuse_count = 0_u64;

    for source_entry in manifest.as_slice() {
        let semantics = semantics_for(source_entry);
        let key = AnalysisArtifactKey::new(
            source_entry.content_digest().clone(),
            semantics.analyzer_identity,
            semantics.configuration_identity,
            semantics.schema_identity,
            semantics.canonicalization_version,
        );
        let action = if reusable_keys.contains(&key) {
            reuse_count += 1;
            ArtifactPlanAction::Reuse
        } else {
            ArtifactPlanAction::Analyze
        };
        entries.push(PlannedAnalysisArtifact {
            path: source_entry.path().clone(),
            key,
            action,
        });
        check_control(&mut control)?;
    }

    let analysis_count = manifest.count().get() - reuse_count;
    Ok(ArtifactReusePlan {
        entries: entries.into_boxed_slice(),
        reuse_count: ArtifactPlanCount(reuse_count),
        analysis_count: ArtifactPlanCount(analysis_count),
    })
}

fn check_control(
    control: &mut impl FnMut() -> Option<ArtifactPlanningError>,
) -> Result<(), ArtifactPlanningError> {
    control().map_or(Ok(()), Err)
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use repowitness_domain::{
        AnalysisArtifactKey, SourceFileLimit, SourceManifest, SourceManifestEntry,
    };

    use super::{
        ArtifactKeySemantics, ArtifactPlanAction, ArtifactPlanningError, ArtifactReusePlan,
        plan_artifact_reuse,
    };

    type TestKey = AnalysisArtifactKey<&'static str, &'static str, &'static str, &'static str, u16>;
    type TestPlan = ArtifactReusePlan<
        &'static str,
        &'static str,
        &'static str,
        &'static str,
        &'static str,
        u16,
    >;
    type TestSemantics = ArtifactKeySemantics<&'static str, &'static str, &'static str, u16>;

    fn entry(
        path: &'static str,
        digest: &'static str,
    ) -> SourceManifestEntry<&'static str, &'static str, &'static str> {
        SourceManifestEntry::new(path, "regular", digest)
    }

    fn manifest(
        entries: Vec<SourceManifestEntry<&'static str, &'static str, &'static str>>,
    ) -> SourceManifest<&'static str, &'static str, &'static str> {
        SourceManifest::try_from_vec(entries, SourceFileLimit::new(8)).expect("valid manifest")
    }

    fn key(
        entry: &SourceManifestEntry<&'static str, &'static str, &'static str>,
        analyzer: &'static str,
    ) -> TestKey {
        AnalysisArtifactKey::new(
            *entry.content_digest(),
            analyzer,
            "configuration:1",
            "schema:1",
            1,
        )
    }

    fn semantics(analyzer: &'static str) -> TestSemantics {
        ArtifactKeySemantics::new(analyzer, "configuration:1", "schema:1", 1)
    }

    fn plan(
        manifest: &SourceManifest<&'static str, &'static str, &'static str>,
        reusable: &BTreeSet<TestKey>,
        analyzer: &'static str,
    ) -> TestPlan {
        plan_with_semantics(manifest, reusable, semantics(analyzer))
    }

    fn plan_with_semantics(
        manifest: &SourceManifest<&'static str, &'static str, &'static str>,
        reusable: &BTreeSet<TestKey>,
        semantics: TestSemantics,
    ) -> TestPlan {
        plan_artifact_reuse(manifest, reusable, |_entry| semantics, || None)
            .expect("planning remains active")
    }

    fn inventory(plan: &TestPlan) -> BTreeSet<TestKey> {
        plan.as_slice().iter().map(|entry| *entry.key()).collect()
    }

    fn materialize(
        plan: &TestPlan,
        available: &mut BTreeMap<TestKey, String>,
    ) -> BTreeMap<&'static str, String> {
        plan.as_slice()
            .iter()
            .map(|entry| {
                let key = *entry.key();
                let value = match entry.action() {
                    ArtifactPlanAction::Reuse => available
                        .get(&key)
                        .expect("reused key must have a verified artifact")
                        .clone(),
                    ArtifactPlanAction::Analyze => {
                        let generated = format!("facts:{}", key.source_digest());
                        available.insert(key, generated.clone());
                        generated
                    }
                };
                (*entry.path(), value)
            })
            .collect()
    }

    #[test]
    fn empty_manifest_produces_an_empty_bounded_plan() {
        let manifest = manifest(Vec::new());
        let plan = plan(&manifest, &BTreeSet::new(), "analyzer:1");

        assert!(plan.is_empty());
        assert!(plan.as_slice().is_empty());
        assert_eq!(plan.reuse_count().get(), 0);
        assert_eq!(plan.analysis_count().get(), 0);
    }

    #[test]
    fn planning_preserves_canonical_path_order_and_counts() {
        let manifest = manifest(vec![
            entry("src/z.rs", "digest:z"),
            entry("src/a.rs", "digest:a"),
        ]);
        let reusable = BTreeSet::from([key(&entry("src/a.rs", "digest:a"), "analyzer:1")]);
        let plan = plan(&manifest, &reusable, "analyzer:1");

        assert_eq!(plan.reuse_count().get(), 1);
        assert_eq!(plan.analysis_count().get(), 1);
        assert_eq!(plan.as_slice()[0].path(), &"src/a.rs");
        assert_eq!(plan.as_slice()[0].action(), ArtifactPlanAction::Reuse);
        assert_eq!(plan.as_slice()[1].path(), &"src/z.rs");
        assert_eq!(plan.as_slice()[1].action(), ArtifactPlanAction::Analyze);
    }

    #[test]
    fn unchanged_files_reuse_artifacts_and_changed_files_do_not() {
        let original = manifest(vec![
            entry("src/a.rs", "digest:a1"),
            entry("src/b.rs", "digest:b1"),
        ]);
        let first = plan(&original, &BTreeSet::new(), "analyzer:1");
        let reusable = inventory(&first);

        let unchanged = plan(&original, &reusable, "analyzer:1");
        assert_eq!(unchanged.reuse_count().get(), 2);
        assert_eq!(unchanged.analysis_count().get(), 0);

        let changed = manifest(vec![
            entry("src/a.rs", "digest:a1"),
            entry("src/b.rs", "digest:b2"),
        ]);
        let incremental = plan(&changed, &reusable, "analyzer:1");
        assert_eq!(incremental.reuse_count().get(), 1);
        assert_eq!(incremental.analysis_count().get(), 1);
        assert_eq!(
            incremental.as_slice()[0].action(),
            ArtifactPlanAction::Reuse
        );
        assert_eq!(
            incremental.as_slice()[1].action(),
            ArtifactPlanAction::Analyze
        );
    }

    #[test]
    fn semantic_input_changes_invalidate_artifact_reuse() {
        let manifest = manifest(vec![
            entry("src/a.rs", "digest:a"),
            entry("src/b.rs", "digest:b"),
        ]);
        let baseline = plan(&manifest, &BTreeSet::new(), "analyzer:1");
        let reusable = inventory(&baseline);
        let changed_semantics = [
            ArtifactKeySemantics::new("analyzer:2", "configuration:1", "schema:1", 1),
            ArtifactKeySemantics::new("analyzer:1", "configuration:2", "schema:1", 1),
            ArtifactKeySemantics::new("analyzer:1", "configuration:1", "schema:2", 1),
            ArtifactKeySemantics::new("analyzer:1", "configuration:1", "schema:1", 2),
        ];

        for changed_semantics in changed_semantics {
            let changed = plan_with_semantics(&manifest, &reusable, changed_semantics);
            assert_eq!(changed.reuse_count().get(), 0);
            assert_eq!(changed.analysis_count().get(), 2);
            assert!(
                changed
                    .as_slice()
                    .iter()
                    .all(|entry| entry.action() == ArtifactPlanAction::Analyze)
            );
        }
    }

    #[test]
    fn planner_uses_the_manifest_digest_and_preserves_key_semantics() {
        let manifest = manifest(vec![entry("src/a.rs", "digest:manifest")]);
        let plan = plan(&manifest, &BTreeSet::new(), "analyzer:1");
        let planned = &plan.as_slice()[0];
        let semantics = semantics("analyzer:1");

        assert_eq!(*semantics.analyzer_identity(), "analyzer:1");
        assert_eq!(*semantics.configuration_identity(), "configuration:1");
        assert_eq!(*semantics.schema_identity(), "schema:1");
        assert_eq!(*semantics.canonicalization_version(), 1);
        assert_eq!(*planned.key().source_digest(), "digest:manifest");
        assert_eq!(*planned.key().analyzer_identity(), "analyzer:1");
        assert_eq!(*planned.key().configuration_identity(), "configuration:1");
        assert_eq!(*planned.key().schema_identity(), "schema:1");
        assert_eq!(*planned.key().canonicalization_version(), 1);
    }

    #[test]
    fn clean_and_incremental_materialization_are_logically_equivalent() {
        let original = manifest(vec![
            entry("src/a.rs", "digest:a1"),
            entry("src/b.rs", "digest:b1"),
        ]);
        let original_plan = plan(&original, &BTreeSet::new(), "analyzer:1");
        let mut incremental_artifacts = BTreeMap::new();
        let _original_output = materialize(&original_plan, &mut incremental_artifacts);

        let changed = manifest(vec![
            entry("src/a.rs", "digest:a1"),
            entry("src/b.rs", "digest:b2"),
        ]);
        let incremental_plan = plan(
            &changed,
            &incremental_artifacts.keys().copied().collect(),
            "analyzer:1",
        );
        let incremental_output = materialize(&incremental_plan, &mut incremental_artifacts);

        let clean_plan = plan(&changed, &BTreeSet::new(), "analyzer:1");
        let clean_output = materialize(&clean_plan, &mut BTreeMap::new());

        assert_eq!(incremental_plan.reuse_count().get(), 1);
        assert_eq!(incremental_plan.analysis_count().get(), 1);
        assert_eq!(clean_plan.reuse_count().get(), 0);
        assert_eq!(clean_plan.analysis_count().get(), 2);
        assert_eq!(incremental_output, clean_output);
    }

    #[test]
    fn cancellation_and_deadline_return_no_partial_plan() {
        let manifest = manifest(vec![
            entry("src/a.rs", "digest:a"),
            entry("src/b.rs", "digest:b"),
        ]);
        let mut key_calls = 0_u8;
        let cancelled = plan_artifact_reuse(
            &manifest,
            &BTreeSet::new(),
            |_entry| {
                key_calls += 1;
                semantics("analyzer:1")
            },
            || Some(ArtifactPlanningError::Cancelled),
        );
        assert_eq!(cancelled, Err(ArtifactPlanningError::Cancelled));
        assert_eq!(key_calls, 0);

        let mut checks = 0_u8;
        let deadline = plan_artifact_reuse(
            &manifest,
            &BTreeSet::new(),
            |_entry| semantics("analyzer:1"),
            || {
                checks += 1;
                (checks == 2).then_some(ArtifactPlanningError::DeadlineExceeded)
            },
        );
        assert_eq!(deadline, Err(ArtifactPlanningError::DeadlineExceeded));
        assert_eq!(checks, 2);
    }

    #[test]
    fn planning_errors_have_stable_redacted_diagnostics() {
        assert_eq!(
            ArtifactPlanningError::Cancelled.to_string(),
            "artifact planning was cancelled"
        );
        assert_eq!(
            ArtifactPlanningError::DeadlineExceeded.to_string(),
            "artifact planning deadline exceeded"
        );
    }
}
