//! Coverage reported alongside a material result.

/// A count of items in one coverage category.
///
/// Counts use a fixed-width representation so persisted and wire formats never
/// inherit the platform-dependent width of `usize`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CoverageItemCount(u64);

impl CoverageItemCount {
    /// No items.
    pub const ZERO: Self = Self(0);

    /// Creates a count from its fixed-width representation.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the fixed-width representation.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    const fn is_zero(self) -> bool {
        self.0 == 0
    }
}

impl From<u64> for CoverageItemCount {
    fn from(value: u64) -> Self {
        Self::new(value)
    }
}

/// How completely a request covered its declared scope.
///
/// This is categorical metadata, not a probability or confidence score.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoverageCompleteness {
    /// The request reports no skipped, unresolved, or truncated work.
    Complete,
    /// The request skipped work or could not resolve part of the scope.
    Partial,
    /// A resource bound stopped work before the declared scope was exhausted.
    Truncated,
}

/// Counts describing which parts of a request were handled or omitted.
///
/// The categories are independent: for example, a searched item may also be
/// unresolved. Consumers must not add the fields to infer a total scope.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CoverageSummary {
    searched: CoverageItemCount,
    skipped: CoverageItemCount,
    unresolved: CoverageItemCount,
    truncated: CoverageItemCount,
}

impl CoverageSummary {
    /// Creates a summary from independent coverage-category counts.
    #[must_use]
    pub const fn new(
        searched: CoverageItemCount,
        skipped: CoverageItemCount,
        unresolved: CoverageItemCount,
        truncated: CoverageItemCount,
    ) -> Self {
        Self {
            searched,
            skipped,
            unresolved,
            truncated,
        }
    }

    /// Returns the number of items searched.
    #[must_use]
    pub const fn searched(self) -> CoverageItemCount {
        self.searched
    }

    /// Returns the number of items skipped.
    #[must_use]
    pub const fn skipped(self) -> CoverageItemCount {
        self.skipped
    }

    /// Returns the number of searched items that could not be resolved.
    #[must_use]
    pub const fn unresolved(self) -> CoverageItemCount {
        self.unresolved
    }

    /// Returns the number of items omitted because a resource bound was hit.
    #[must_use]
    pub const fn truncated(self) -> CoverageItemCount {
        self.truncated
    }

    /// Classifies whether the declared request scope was covered completely.
    #[must_use]
    pub const fn completeness(self) -> CoverageCompleteness {
        if !self.truncated.is_zero() {
            CoverageCompleteness::Truncated
        } else if !self.skipped.is_zero() || !self.unresolved.is_zero() {
            CoverageCompleteness::Partial
        } else {
            CoverageCompleteness::Complete
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CoverageCompleteness, CoverageItemCount, CoverageSummary};

    #[test]
    fn empty_scope_is_complete() {
        let coverage = CoverageSummary::default();

        assert_eq!(coverage.completeness(), CoverageCompleteness::Complete);
    }

    #[test]
    fn item_count_converts_from_its_fixed_width_representation() {
        let count = CoverageItemCount::from(7_u64);

        assert_eq!(count.get(), 7);
    }

    #[test]
    fn skipped_work_makes_coverage_partial() {
        let coverage = CoverageSummary::new(
            CoverageItemCount::new(3),
            CoverageItemCount::new(1),
            CoverageItemCount::ZERO,
            CoverageItemCount::ZERO,
        );

        assert_eq!(coverage.completeness(), CoverageCompleteness::Partial);
    }

    #[test]
    fn unresolved_work_makes_coverage_partial() {
        let coverage = CoverageSummary::new(
            CoverageItemCount::new(3),
            CoverageItemCount::ZERO,
            CoverageItemCount::new(1),
            CoverageItemCount::ZERO,
        );

        assert_eq!(coverage.completeness(), CoverageCompleteness::Partial);
    }

    #[test]
    fn truncation_is_reported_even_when_other_work_is_partial() {
        let coverage = CoverageSummary::new(
            CoverageItemCount::new(3),
            CoverageItemCount::new(1),
            CoverageItemCount::new(1),
            CoverageItemCount::new(2),
        );

        assert_eq!(coverage.completeness(), CoverageCompleteness::Truncated);
    }

    #[test]
    fn category_counts_remain_independent() {
        let coverage = CoverageSummary::new(
            CoverageItemCount::new(5),
            CoverageItemCount::new(4),
            CoverageItemCount::new(3),
            CoverageItemCount::new(2),
        );

        assert_eq!(coverage.searched().get(), 5);
        assert_eq!(coverage.skipped().get(), 4);
        assert_eq!(coverage.unresolved().get(), 3);
        assert_eq!(coverage.truncated().get(), 2);
    }
}
