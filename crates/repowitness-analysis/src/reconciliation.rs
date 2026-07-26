use std::{cmp::Ordering, collections::BTreeSet, error::Error, fmt};

use repowitness_domain::{SourceManifest, SourceManifestEntry};

/// Default maximum number of source changes in one reconciliation plan.
pub const DEFAULT_RECONCILIATION_CHANGES: u64 = 200_000;
/// Default maximum number of watcher hints admitted by one reconciliation.
pub const DEFAULT_RECONCILIATION_HINTS: u64 = 200_000;
/// Hard Phase 0 maximum number of source changes in one reconciliation plan.
pub const MAX_RECONCILIATION_CHANGES: u64 = 1_000_000;
/// Hard Phase 0 maximum number of watcher hints admitted by one reconciliation.
pub const MAX_RECONCILIATION_HINTS: u64 = 1_000_000;

/// A count reported by source-manifest reconciliation.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ReconciliationCount(u64);

impl ReconciliationCount {
    /// Returns the fixed-width count.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Explicit limits for one source-manifest reconciliation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ManifestReconciliationLimits {
    max_changes: u64,
    max_hints: u64,
}

impl ManifestReconciliationLimits {
    /// Constructs limits within the Phase 0 hard ceilings.
    pub const fn new(
        max_changes: u64,
        max_hints: u64,
    ) -> Result<Self, ManifestReconciliationError> {
        if max_changes > MAX_RECONCILIATION_CHANGES {
            return Err(ManifestReconciliationError::ChangeLimitTooLarge);
        }
        if max_hints > MAX_RECONCILIATION_HINTS {
            return Err(ManifestReconciliationError::HintLimitTooLarge);
        }
        Ok(Self {
            max_changes,
            max_hints,
        })
    }

    /// Returns the maximum number of emitted changes.
    #[must_use]
    pub const fn max_changes(self) -> u64 {
        self.max_changes
    }

    /// Returns the maximum number of received watcher hints.
    #[must_use]
    pub const fn max_hints(self) -> u64 {
        self.max_hints
    }
}

impl Default for ManifestReconciliationLimits {
    fn default() -> Self {
        Self {
            max_changes: DEFAULT_RECONCILIATION_CHANGES,
            max_hints: DEFAULT_RECONCILIATION_HINTS,
        }
    }
}

/// Stable category for one complete-manifest difference.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManifestChangeKind {
    /// The path exists only in the current manifest.
    Added,
    /// The path exists only in the previous manifest.
    Removed,
    /// The path remains but its file type or content digest changed.
    Changed,
}

/// One path-ordered difference between complete source manifests.
#[derive(Clone, Eq, PartialEq)]
pub enum ManifestChange<P, K, D> {
    /// A current entry with no previous path identity.
    Added(SourceManifestEntry<P, K, D>),
    /// A previous entry with no current path identity.
    Removed(SourceManifestEntry<P, K, D>),
    /// Two entries with the same path and different semantics.
    Changed {
        /// The exact previous entry.
        previous: SourceManifestEntry<P, K, D>,
        /// The exact current entry.
        current: SourceManifestEntry<P, K, D>,
    },
}

impl<P, K, D> ManifestChange<P, K, D> {
    /// Returns the stable change category.
    #[must_use]
    pub const fn kind(&self) -> ManifestChangeKind {
        match self {
            Self::Added(_) => ManifestChangeKind::Added,
            Self::Removed(_) => ManifestChangeKind::Removed,
            Self::Changed { .. } => ManifestChangeKind::Changed,
        }
    }

    /// Returns the repository path shared by the change.
    #[must_use]
    pub const fn path(&self) -> &P {
        match self {
            Self::Added(entry) | Self::Removed(entry) => entry.path(),
            Self::Changed { current, .. } => current.path(),
        }
    }

    /// Returns the previous entry when one exists.
    #[must_use]
    pub const fn previous(&self) -> Option<&SourceManifestEntry<P, K, D>> {
        match self {
            Self::Added(_) => None,
            Self::Removed(entry)
            | Self::Changed {
                previous: entry, ..
            } => Some(entry),
        }
    }

    /// Returns the current entry when one exists.
    #[must_use]
    pub const fn current(&self) -> Option<&SourceManifestEntry<P, K, D>> {
        match self {
            Self::Removed(_) => None,
            Self::Added(entry) | Self::Changed { current: entry, .. } => Some(entry),
        }
    }
}

impl<P, K, D> fmt::Debug for ManifestChange<P, K, D> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ManifestChange")
            .field("kind", &self.kind())
            .finish_non_exhaustive()
    }
}

/// Complete bounded result of reconciling two canonical source manifests.
#[derive(Clone, Eq, PartialEq)]
pub struct ManifestReconciliation<P, K, D> {
    changes: Box<[ManifestChange<P, K, D>]>,
    unchanged: ReconciliationCount,
    received_hints: ReconciliationCount,
    unique_hints: ReconciliationCount,
    unmatched_hints: ReconciliationCount,
}

impl<P, K, D> ManifestReconciliation<P, K, D> {
    /// Returns path-ordered added, removed, and changed entries.
    #[must_use]
    pub fn changes(&self) -> &[ManifestChange<P, K, D>] {
        &self.changes
    }

    /// Returns the number of unchanged paths.
    #[must_use]
    pub const fn unchanged(&self) -> ReconciliationCount {
        self.unchanged
    }

    /// Returns the number of watcher hints received, including duplicates.
    #[must_use]
    pub const fn received_hints(&self) -> ReconciliationCount {
        self.received_hints
    }

    /// Returns the number of distinct watcher hints.
    #[must_use]
    pub const fn unique_hints(&self) -> ReconciliationCount {
        self.unique_hints
    }

    /// Returns distinct hints absent from both complete manifests.
    #[must_use]
    pub const fn unmatched_hints(&self) -> ReconciliationCount {
        self.unmatched_hints
    }
}

impl<P, K, D> fmt::Debug for ManifestReconciliation<P, K, D> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ManifestReconciliation")
            .field("change_count", &self.changes.len())
            .field("unchanged", &self.unchanged)
            .field("received_hints", &self.received_hints)
            .field("unique_hints", &self.unique_hints)
            .field("unmatched_hints", &self.unmatched_hints)
            .finish()
    }
}

/// Stable failure from bounded manifest reconciliation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManifestReconciliationError {
    /// The configured change limit exceeds the Phase 0 hard ceiling.
    ChangeLimitTooLarge,
    /// The configured hint limit exceeds the Phase 0 hard ceiling.
    HintLimitTooLarge,
    /// More watcher hints were supplied than the configured bound.
    HintLimitExceeded,
    /// More source changes were found than the configured bound.
    ChangeLimitExceeded,
    /// A platform collection length cannot be represented as a fixed-width count.
    CountNotRepresentable,
    /// The caller requested cooperative cancellation.
    Cancelled,
    /// The caller's absolute deadline expired.
    DeadlineExceeded,
}

impl fmt::Display for ManifestReconciliationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ChangeLimitTooLarge => {
                "manifest reconciliation change limit exceeds the Phase 0 maximum"
            }
            Self::HintLimitTooLarge => {
                "manifest reconciliation hint limit exceeds the Phase 0 maximum"
            }
            Self::HintLimitExceeded => "manifest reconciliation received too many watcher hints",
            Self::ChangeLimitExceeded => "manifest reconciliation produced too many source changes",
            Self::CountNotRepresentable => "manifest reconciliation count is not representable",
            Self::Cancelled => "manifest reconciliation was cancelled",
            Self::DeadlineExceeded => "manifest reconciliation deadline expired",
        })
    }
}

impl Error for ManifestReconciliationError {}

struct ReconciliationBuilder<P, K, D> {
    changes: Vec<ManifestChange<P, K, D>>,
    unchanged: u64,
    max_changes: u64,
}

impl<P, K, D> ReconciliationBuilder<P, K, D> {
    fn new(max_changes: u64) -> Self {
        Self {
            changes: Vec::new(),
            unchanged: 0,
            max_changes,
        }
    }

    fn push(&mut self, change: ManifestChange<P, K, D>) -> Result<(), ManifestReconciliationError> {
        let count = fixed_count(self.changes.len())?;
        if count >= self.max_changes {
            return Err(ManifestReconciliationError::ChangeLimitExceeded);
        }
        self.changes.push(change);
        Ok(())
    }

    fn mark_unchanged(&mut self) -> Result<(), ManifestReconciliationError> {
        self.unchanged = self
            .unchanged
            .checked_add(1)
            .ok_or(ManifestReconciliationError::CountNotRepresentable)?;
        Ok(())
    }
}

/// Reconciles complete canonical manifests while treating watcher hints only
/// as bounded diagnostic metadata.
///
/// Correctness never depends on hint presence, ordering, or uniqueness.
/// `control` is checked before work and after each hint and manifest entry;
/// any error returns no partial plan.
pub fn reconcile_source_manifests<P, K, D, Control>(
    previous: &SourceManifest<P, K, D>,
    current: &SourceManifest<P, K, D>,
    hints: &[P],
    limits: ManifestReconciliationLimits,
    mut control: Control,
) -> Result<ManifestReconciliation<P, K, D>, ManifestReconciliationError>
where
    P: Clone + Ord,
    K: Clone + Eq,
    D: Clone + Eq,
    Control: FnMut() -> Option<ManifestReconciliationError>,
{
    check_control(&mut control)?;
    let received_hints = fixed_count(hints.len())?;
    if received_hints > limits.max_hints {
        return Err(ManifestReconciliationError::HintLimitExceeded);
    }

    let unique_hints = collect_unique_hints(hints, &mut control)?;
    let unmatched_hints = count_unmatched_hints(&unique_hints, previous, current, &mut control)?;
    let mut builder = ReconciliationBuilder::new(limits.max_changes);
    reconcile_entries(previous, current, &mut builder, &mut control)?;

    Ok(ManifestReconciliation {
        changes: builder.changes.into_boxed_slice(),
        unchanged: ReconciliationCount(builder.unchanged),
        received_hints: ReconciliationCount(received_hints),
        unique_hints: ReconciliationCount(fixed_count(unique_hints.len())?),
        unmatched_hints: ReconciliationCount(unmatched_hints),
    })
}

fn collect_unique_hints<P, Control>(
    hints: &[P],
    control: &mut Control,
) -> Result<BTreeSet<P>, ManifestReconciliationError>
where
    P: Clone + Ord,
    Control: FnMut() -> Option<ManifestReconciliationError>,
{
    let mut unique = BTreeSet::new();
    for hint in hints {
        unique.insert(hint.clone());
        check_control(control)?;
    }
    Ok(unique)
}

fn count_unmatched_hints<P, K, D, Control>(
    hints: &BTreeSet<P>,
    previous: &SourceManifest<P, K, D>,
    current: &SourceManifest<P, K, D>,
    control: &mut Control,
) -> Result<u64, ManifestReconciliationError>
where
    P: Ord,
    Control: FnMut() -> Option<ManifestReconciliationError>,
{
    let mut unmatched = 0_u64;
    for hint in hints {
        if !contains_path(previous, hint) && !contains_path(current, hint) {
            unmatched = unmatched
                .checked_add(1)
                .ok_or(ManifestReconciliationError::CountNotRepresentable)?;
        }
        check_control(control)?;
    }
    Ok(unmatched)
}

fn contains_path<P: Ord, K, D>(manifest: &SourceManifest<P, K, D>, path: &P) -> bool {
    manifest
        .as_slice()
        .binary_search_by(|entry| entry.path().cmp(path))
        .is_ok()
}

fn reconcile_entries<P, K, D, Control>(
    previous: &SourceManifest<P, K, D>,
    current: &SourceManifest<P, K, D>,
    builder: &mut ReconciliationBuilder<P, K, D>,
    control: &mut Control,
) -> Result<(), ManifestReconciliationError>
where
    P: Clone + Ord,
    K: Clone + Eq,
    D: Clone + Eq,
    Control: FnMut() -> Option<ManifestReconciliationError>,
{
    let previous = previous.as_slice();
    let current = current.as_slice();
    let mut previous_index = 0;
    let mut current_index = 0;

    while previous_index < previous.len() && current_index < current.len() {
        match previous[previous_index]
            .path()
            .cmp(current[current_index].path())
        {
            Ordering::Less => {
                builder.push(ManifestChange::Removed(previous[previous_index].clone()))?;
                previous_index += 1;
            }
            Ordering::Greater => {
                builder.push(ManifestChange::Added(current[current_index].clone()))?;
                current_index += 1;
            }
            Ordering::Equal => {
                reconcile_equal_path(&previous[previous_index], &current[current_index], builder)?;
                previous_index += 1;
                current_index += 1;
            }
        }
        check_control(control)?;
    }

    append_remaining(previous, previous_index, builder, control, |entry| {
        ManifestChange::Removed(entry)
    })?;
    append_remaining(current, current_index, builder, control, |entry| {
        ManifestChange::Added(entry)
    })
}

fn reconcile_equal_path<P, K, D>(
    previous: &SourceManifestEntry<P, K, D>,
    current: &SourceManifestEntry<P, K, D>,
    builder: &mut ReconciliationBuilder<P, K, D>,
) -> Result<(), ManifestReconciliationError>
where
    P: Clone,
    K: Clone + Eq,
    D: Clone + Eq,
{
    if previous.file_type() == current.file_type()
        && previous.content_digest() == current.content_digest()
    {
        builder.mark_unchanged()
    } else {
        builder.push(ManifestChange::Changed {
            previous: previous.clone(),
            current: current.clone(),
        })
    }
}

fn append_remaining<P, K, D, Control, MakeChange>(
    entries: &[SourceManifestEntry<P, K, D>],
    start: usize,
    builder: &mut ReconciliationBuilder<P, K, D>,
    control: &mut Control,
    mut make_change: MakeChange,
) -> Result<(), ManifestReconciliationError>
where
    P: Clone,
    K: Clone,
    D: Clone,
    Control: FnMut() -> Option<ManifestReconciliationError>,
    MakeChange: FnMut(SourceManifestEntry<P, K, D>) -> ManifestChange<P, K, D>,
{
    for entry in &entries[start..] {
        builder.push(make_change(entry.clone()))?;
        check_control(control)?;
    }
    Ok(())
}

fn fixed_count(length: usize) -> Result<u64, ManifestReconciliationError> {
    u64::try_from(length).map_err(|_| ManifestReconciliationError::CountNotRepresentable)
}

fn check_control<Control>(control: &mut Control) -> Result<(), ManifestReconciliationError>
where
    Control: FnMut() -> Option<ManifestReconciliationError>,
{
    control().map_or(Ok(()), Err)
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_RECONCILIATION_CHANGES, MAX_RECONCILIATION_HINTS, ManifestChange, ManifestChangeKind,
        ManifestReconciliationError, ManifestReconciliationLimits, reconcile_source_manifests,
    };
    use repowitness_domain::{SourceFileLimit, SourceManifest, SourceManifestEntry};

    type TestManifest = SourceManifest<&'static str, u8, u8>;

    fn entry(
        path: &'static str,
        file_type: u8,
        digest: u8,
    ) -> SourceManifestEntry<&'static str, u8, u8> {
        SourceManifestEntry::new(path, file_type, digest)
    }

    fn manifest(entries: Vec<SourceManifestEntry<&'static str, u8, u8>>) -> TestManifest {
        SourceManifest::try_from_vec(entries, SourceFileLimit::new(16))
            .expect("fixture manifest should be valid")
    }

    fn limits(max_changes: u64, max_hints: u64) -> ManifestReconciliationLimits {
        ManifestReconciliationLimits::new(max_changes, max_hints)
            .expect("fixture limits should be valid")
    }

    #[test]
    fn complete_manifests_determine_stable_path_ordered_changes() {
        let previous = manifest(vec![
            entry("a.rs", 1, 1),
            entry("b.rs", 1, 1),
            entry("d.rs", 1, 1),
            entry("e.rs", 1, 1),
        ]);
        let current = manifest(vec![
            entry("a.rs", 1, 1),
            entry("b.rs", 1, 2),
            entry("c.rs", 1, 1),
            entry("e.rs", 2, 1),
        ]);
        let result = reconcile_source_manifests(
            &previous,
            &current,
            &["b.rs", "b.rs", "c.rs", "phantom.rs"],
            limits(8, 8),
            || None,
        )
        .expect("reconciliation should succeed");

        assert_eq!(
            result
                .changes()
                .iter()
                .map(|change| (*change.path(), change.kind()))
                .collect::<Vec<_>>(),
            vec![
                ("b.rs", ManifestChangeKind::Changed),
                ("c.rs", ManifestChangeKind::Added),
                ("d.rs", ManifestChangeKind::Removed),
                ("e.rs", ManifestChangeKind::Changed),
            ]
        );
        assert_eq!(result.unchanged().get(), 1);
        assert_eq!(result.received_hints().get(), 4);
        assert_eq!(result.unique_hints().get(), 3);
        assert_eq!(result.unmatched_hints().get(), 1);
        assert_eq!(result.changes()[0].previous(), Some(&entry("b.rs", 1, 1)));
        assert_eq!(result.changes()[0].current(), Some(&entry("b.rs", 1, 2)));
    }

    #[test]
    fn dropped_duplicated_and_reordered_hints_cannot_change_logical_output() {
        let previous = manifest(vec![entry("a.rs", 1, 1), entry("b.rs", 1, 1)]);
        let current = manifest(vec![entry("a.rs", 1, 2), entry("c.rs", 1, 1)]);
        let no_hints = reconcile_source_manifests(&previous, &current, &[], limits(8, 8), || None)
            .expect("reconciliation without hints should succeed");
        let noisy_hints = reconcile_source_manifests(
            &previous,
            &current,
            &["c.rs", "a.rs", "a.rs", "stale.rs", "c.rs"],
            limits(8, 8),
            || None,
        )
        .expect("reconciliation with noisy hints should succeed");

        assert_eq!(no_hints.changes(), noisy_hints.changes());
        assert_eq!(no_hints.unchanged(), noisy_hints.unchanged());
        assert_eq!(no_hints.received_hints().get(), 0);
        assert_eq!(noisy_hints.received_hints().get(), 5);
    }

    #[test]
    fn zero_and_exact_limits_are_enforced_before_partial_output() {
        let empty = manifest(vec![]);
        let one = manifest(vec![entry("a.rs", 1, 1)]);
        let exact = reconcile_source_manifests(&empty, &one, &["a.rs"], limits(1, 1), || None)
            .expect("inclusive limits should succeed");
        assert_eq!(exact.changes().len(), 1);

        assert_eq!(
            reconcile_source_manifests(&empty, &one, &[], limits(0, 0), || None),
            Err(ManifestReconciliationError::ChangeLimitExceeded)
        );
        assert_eq!(
            reconcile_source_manifests(&empty, &empty, &["a.rs"], limits(0, 0), || None),
            Err(ManifestReconciliationError::HintLimitExceeded)
        );
        assert_eq!(
            ManifestReconciliationLimits::new(MAX_RECONCILIATION_CHANGES + 1, 0),
            Err(ManifestReconciliationError::ChangeLimitTooLarge)
        );
        assert_eq!(
            ManifestReconciliationLimits::new(0, MAX_RECONCILIATION_HINTS + 1),
            Err(ManifestReconciliationError::HintLimitTooLarge)
        );
    }

    #[test]
    fn cooperative_stop_returns_no_plan_and_errors_are_redacted() {
        let previous = manifest(vec![entry("secret-name.rs", 1, 1)]);
        let current = manifest(vec![entry("secret-name.rs", 1, 2)]);
        let mut checks = 0;
        let error = reconcile_source_manifests(
            &previous,
            &current,
            &["secret-name.rs"],
            limits(4, 4),
            || {
                checks += 1;
                (checks == 3).then_some(ManifestReconciliationError::Cancelled)
            },
        )
        .expect_err("cancellation should discard the plan");

        assert_eq!(error, ManifestReconciliationError::Cancelled);
        assert!(!error.to_string().contains("secret-name"));
        assert!(!format!("{error:?}").contains("secret-name"));
        assert_eq!(
            ManifestReconciliationError::DeadlineExceeded.to_string(),
            "manifest reconciliation deadline expired"
        );
    }

    #[test]
    fn change_debug_output_does_not_expose_repository_paths() {
        let change = ManifestChange::Added(entry("secret-name.rs", 1, 1));
        let debug = format!("{change:?}");

        assert!(debug.contains("Added"));
        assert!(!debug.contains("secret-name"));
    }
}
