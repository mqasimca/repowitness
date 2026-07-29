//! Local package-scope filtering over validated Git discovery output.

use core::fmt;
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

use repowitness_application::PackageScope;
use repowitness_domain::{CoverageItemCount, CoverageSummary, RepositoryPath};

use crate::{DiscoveredRepositoryPaths, GitPathDiscoveryStats};

/// Non-sensitive counts from one package-scope filtering pass.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PackageScopeFilterStats {
    discovered_paths: u64,
    selected_paths: u64,
    policy_omitted_paths: u64,
}

impl PackageScopeFilterStats {
    /// Returns the number of validated discovery inputs.
    #[must_use]
    pub(crate) const fn discovered_paths(self) -> u64 {
        self.discovered_paths
    }

    /// Returns the number of paths retained by the package scope.
    #[must_use]
    #[cfg(test)]
    pub(crate) const fn selected_paths(self) -> u64 {
        self.selected_paths
    }

    /// Returns the number of paths explicitly omitted by policy.
    #[must_use]
    pub(crate) const fn policy_omitted_paths(self) -> u64 {
        self.policy_omitted_paths
    }
}

/// Deterministically ordered repository paths selected by a package scope.
#[derive(Clone, Eq, PartialEq)]
pub(crate) struct PackageScopeFilterResult {
    paths: Box<[RepositoryPath]>,
    discovery_stats: GitPathDiscoveryStats,
    stats: PackageScopeFilterStats,
    coverage: CoverageSummary,
}

impl PackageScopeFilterResult {
    /// Returns selected paths in their original canonical discovery order.
    #[must_use]
    #[cfg(test)]
    pub(crate) fn paths(&self) -> &[RepositoryPath] {
        &self.paths
    }

    /// Consumes the result without copying repository-path bytes.
    #[must_use]
    pub(crate) fn into_paths(self) -> Box<[RepositoryPath]> {
        self.paths
    }

    /// Returns the original aggregate Git discovery statistics.
    #[must_use]
    #[cfg(test)]
    pub(crate) const fn discovery_stats(&self) -> GitPathDiscoveryStats {
        self.discovery_stats
    }

    /// Returns package-scope selection counts.
    #[must_use]
    pub(crate) const fn stats(&self) -> PackageScopeFilterStats {
        self.stats
    }

    /// Returns explicit complete-or-partial coverage.
    #[must_use]
    #[cfg(test)]
    pub(crate) const fn coverage(&self) -> CoverageSummary {
        self.coverage
    }
}

impl fmt::Debug for PackageScopeFilterResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PackageScopeFilterResult")
            .field("path_count", &self.stats.selected_paths)
            .field("discovery_stats", &self.discovery_stats)
            .field("stats", &self.stats)
            .field("coverage", &self.coverage)
            .finish_non_exhaustive()
    }
}

/// A bounded package-scope filtering failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PackageScopeFilterError {
    /// Filtering was cancelled.
    Cancelled,
    /// Filtering did not complete before its monotonic deadline.
    DeadlineExceeded,
    /// The in-memory path count could not be represented as a `u64`.
    PathCountNotRepresentable,
    /// Validated discovery statistics did not match the path allocation.
    DiscoveryPathCountMismatch {
        /// Count reported by discovery.
        reported: u64,
        /// Count observed by filtering.
        observed: u64,
    },
    /// The selected count overflowed its fixed-width representation.
    SelectedPathCountOverflowed,
    /// The policy-omission count overflowed its fixed-width representation.
    PolicyOmissionCountOverflowed,
}

impl fmt::Display for PackageScopeFilterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => formatter.write_str("package-scope filtering was cancelled"),
            Self::DeadlineExceeded => {
                formatter.write_str("package-scope filtering deadline exceeded")
            }
            Self::PathCountNotRepresentable => {
                formatter.write_str("package-scope path count cannot be represented")
            }
            Self::DiscoveryPathCountMismatch { reported, observed } => write!(
                formatter,
                "package-scope discovery count mismatch: reported {reported}, observed {observed}"
            ),
            Self::SelectedPathCountOverflowed => {
                formatter.write_str("package-scope selected path count overflowed")
            }
            Self::PolicyOmissionCountOverflowed => {
                formatter.write_str("package-scope policy-omission count overflowed")
            }
        }
    }
}

impl std::error::Error for PackageScopeFilterError {}

/// Filters one validated discovery result without filesystem or Git I/O.
pub(crate) fn filter_discovered_repository_paths(
    discovered: DiscoveredRepositoryPaths,
    scope: &PackageScope,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<PackageScopeFilterResult, PackageScopeFilterError> {
    let discovery_stats = discovered.stats();
    filter_paths_with_control(
        discovered.into_paths(),
        discovery_stats,
        scope,
        deadline,
        || cancelled.load(Ordering::Acquire),
        Instant::now,
    )
}

fn filter_paths_with_control(
    paths: Box<[RepositoryPath]>,
    discovery_stats: GitPathDiscoveryStats,
    scope: &PackageScope,
    deadline: Instant,
    mut is_cancelled: impl FnMut() -> bool,
    mut now: impl FnMut() -> Instant,
) -> Result<PackageScopeFilterResult, PackageScopeFilterError> {
    check_control(deadline, &mut is_cancelled, &mut now)?;
    let discovered_paths = u64::try_from(paths.len())
        .map_err(|_| PackageScopeFilterError::PathCountNotRepresentable)?;
    if discovery_stats.path_count() != discovered_paths {
        return Err(PackageScopeFilterError::DiscoveryPathCountMismatch {
            reported: discovery_stats.path_count(),
            observed: discovered_paths,
        });
    }
    if scope.is_whole_repository() {
        return Ok(filter_result(
            paths,
            discovery_stats,
            discovered_paths,
            discovered_paths,
            0,
        ));
    }

    let mut selected = Vec::with_capacity(paths.len());
    let mut selected_paths = 0_u64;
    let mut policy_omitted_paths = 0_u64;
    for path in paths {
        check_control(deadline, &mut is_cancelled, &mut now)?;
        if scope.contains(&path) {
            selected_paths = selected_paths
                .checked_add(1)
                .ok_or(PackageScopeFilterError::SelectedPathCountOverflowed)?;
            selected.push(path);
        } else {
            policy_omitted_paths = policy_omitted_paths
                .checked_add(1)
                .ok_or(PackageScopeFilterError::PolicyOmissionCountOverflowed)?;
        }
    }
    check_control(deadline, &mut is_cancelled, &mut now)?;

    Ok(filter_result(
        selected.into_boxed_slice(),
        discovery_stats,
        discovered_paths,
        selected_paths,
        policy_omitted_paths,
    ))
}

fn filter_result(
    paths: Box<[RepositoryPath]>,
    discovery_stats: GitPathDiscoveryStats,
    discovered_paths: u64,
    selected_paths: u64,
    policy_omitted_paths: u64,
) -> PackageScopeFilterResult {
    PackageScopeFilterResult {
        paths,
        discovery_stats,
        stats: PackageScopeFilterStats {
            discovered_paths,
            selected_paths,
            policy_omitted_paths,
        },
        coverage: CoverageSummary::new(
            CoverageItemCount::new(selected_paths),
            CoverageItemCount::new(policy_omitted_paths),
            CoverageItemCount::ZERO,
            CoverageItemCount::ZERO,
        ),
    }
}

fn check_control(
    deadline: Instant,
    is_cancelled: &mut impl FnMut() -> bool,
    now: &mut impl FnMut() -> Instant,
) -> Result<(), PackageScopeFilterError> {
    if is_cancelled() {
        Err(PackageScopeFilterError::Cancelled)
    } else if now() >= deadline {
        Err(PackageScopeFilterError::DeadlineExceeded)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
