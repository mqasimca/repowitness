//! Versioned, byte-preserving package-scope selection.

use core::fmt;

use repowitness_domain::{RepositoryPath, RepositoryPathError, RepositoryPathLimits};
use sha2::{Digest, Sha256};

const PACKAGE_SCOPE_DIGEST_DOMAIN: &[u8] = b"RepoWitness\0package-scope\0";
const WHOLE_REPOSITORY_TAG: u8 = 0;
const EXPLICIT_ROOTS_TAG: u8 = 1;

/// Version of the canonical package-scope identity.
pub const PACKAGE_SCOPE_VERSION: u16 = 1;
/// Maximum number of explicit package roots in one source slot.
pub const MAX_PACKAGE_SCOPE_ROOTS: PackageRootCount = PackageRootCount::new(64);

/// A fixed-width package-root count.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct PackageRootCount(u64);

impl PackageRootCount {
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
}

/// A fixed-width one-based package-root ordinal.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct PackageRootOrdinal(u64);

impl PackageRootOrdinal {
    /// Creates an ordinal from its fixed-width representation.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the fixed-width representation.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Canonical semantic identity of one package scope.
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub struct PackageScopeDigest([u8; 32]);

impl PackageScopeDigest {
    /// Returns the exact SHA-256 digest bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Consumes the identity and returns its exact bytes.
    #[must_use]
    pub const fn into_bytes(self) -> [u8; 32] {
        self.0
    }
}

impl fmt::Debug for PackageScopeDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PackageScopeDigest(..)")
    }
}

/// A whole-repository or explicit-root source scope.
#[derive(Clone, Eq, PartialEq)]
pub struct PackageScope {
    roots: Option<Box<[RepositoryPath]>>,
}

impl PackageScope {
    /// Selects every repository path.
    #[must_use]
    pub const fn whole_repository() -> Self {
        Self { roots: None }
    }

    /// Validates raw explicit roots with the repository-path contract.
    ///
    /// The iterator is consumed only through the first over-limit item.
    ///
    /// # Errors
    ///
    /// Returns a redacted error for an empty set, an invalid root, too many
    /// roots, duplicate roots, or component-boundary overlap.
    pub fn try_explicit_root_bytes<I, B>(
        roots: I,
        path_limits: RepositoryPathLimits,
    ) -> Result<Self, PackageScopeError>
    where
        I: IntoIterator<Item = B>,
        B: AsRef<[u8]>,
    {
        let mut validated = Vec::new();
        for root in roots {
            if validated.len()
                >= usize::try_from(MAX_PACKAGE_SCOPE_ROOTS.get())
                    .expect("package root bound fits usize")
            {
                return Err(PackageScopeError::RootLimitExceeded {
                    limit: MAX_PACKAGE_SCOPE_ROOTS,
                });
            }
            let ordinal = PackageRootOrdinal::new(
                u64::try_from(validated.len() + 1)
                    .expect("bounded package root ordinal fits in u64"),
            );
            let root = RepositoryPath::try_from_bytes(root.as_ref(), path_limits)
                .map_err(|source| PackageScopeError::InvalidRoot { ordinal, source })?;
            validated.push(root);
        }
        Self::try_explicit_roots(validated)
    }

    /// Canonicalizes already-validated explicit roots.
    ///
    /// # Errors
    ///
    /// Returns a redacted error for an empty set, too many roots, duplicate
    /// roots, or component-boundary overlap.
    pub fn try_explicit_roots(mut roots: Vec<RepositoryPath>) -> Result<Self, PackageScopeError> {
        if roots.is_empty() {
            return Err(PackageScopeError::EmptyExplicitRoots);
        }
        if roots.len()
            > usize::try_from(MAX_PACKAGE_SCOPE_ROOTS.get()).expect("package root bound fits usize")
        {
            return Err(PackageScopeError::RootLimitExceeded {
                limit: MAX_PACKAGE_SCOPE_ROOTS,
            });
        }

        roots.sort_unstable();
        validate_canonical_roots(&roots)?;
        Ok(Self {
            roots: Some(roots.into_boxed_slice()),
        })
    }

    /// Returns whether this scope selects the whole repository.
    #[must_use]
    pub const fn is_whole_repository(&self) -> bool {
        self.roots.is_none()
    }

    /// Returns explicit roots in canonical exact-byte order.
    #[must_use]
    pub fn explicit_roots(&self) -> Option<&[RepositoryPath]> {
        self.roots.as_deref()
    }

    /// Returns the number of explicit roots.
    #[must_use]
    pub fn root_count(&self) -> PackageRootCount {
        let count = self.roots.as_ref().map_or(0, |roots| roots.len());
        PackageRootCount::new(u64::try_from(count).expect("bounded package root count fits in u64"))
    }

    /// Tests exact repository-component membership.
    #[must_use]
    pub fn contains(&self, path: &RepositoryPath) -> bool {
        self.roots
            .as_ref()
            .is_none_or(|roots| roots.iter().any(|root| root_contains(root, path)))
    }

    /// Computes a domain-separated, versioned semantic identity.
    ///
    /// This fixed-width identity is suitable as an input to configuration and
    /// source-snapshot digest composition.
    #[must_use]
    pub fn semantic_digest(&self) -> PackageScopeDigest {
        let mut hasher = Sha256::new();
        hasher.update(PACKAGE_SCOPE_DIGEST_DOMAIN);
        hasher.update(PACKAGE_SCOPE_VERSION.to_be_bytes());
        hasher.update(RepositoryPath::VERSION.get().to_be_bytes());
        match &self.roots {
            None => {
                hasher.update([WHOLE_REPOSITORY_TAG]);
                hasher.update(0_u64.to_be_bytes());
            }
            Some(roots) => {
                hasher.update([EXPLICIT_ROOTS_TAG]);
                hasher.update(self.root_count().get().to_be_bytes());
                for root in roots {
                    hasher.update(root.byte_count().get().to_be_bytes());
                    hasher.update(root.as_bytes());
                }
            }
        }
        PackageScopeDigest(hasher.finalize().into())
    }
}

impl fmt::Debug for PackageScope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_whole_repository() {
            formatter.write_str("PackageScope::WholeRepository")
        } else {
            formatter
                .debug_struct("PackageScope::ExplicitRoots")
                .field("root_count", &self.root_count())
                .finish_non_exhaustive()
        }
    }
}

/// Failure to construct a package scope.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PackageScopeError {
    /// The explicit-root category contained no roots.
    EmptyExplicitRoots,
    /// More than the bounded number of roots was supplied.
    RootLimitExceeded {
        /// The inclusive root bound.
        limit: PackageRootCount,
    },
    /// One root failed byte-preserving repository-path validation.
    InvalidRoot {
        /// The one-based root ordinal.
        ordinal: PackageRootOrdinal,
        /// The redacted path validation error.
        source: RepositoryPathError,
    },
    /// Two roots had identical exact bytes.
    DuplicateRoot,
    /// One root was an exact component-boundary ancestor of another.
    OverlappingRoots,
}

impl fmt::Display for PackageScopeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyExplicitRoots => {
                formatter.write_str("explicit package scope requires at least one root")
            }
            Self::RootLimitExceeded { limit } => {
                write!(
                    formatter,
                    "package root count exceeds limit {}",
                    limit.get()
                )
            }
            Self::InvalidRoot { ordinal, source } => {
                write!(
                    formatter,
                    "package root {} is invalid: {source}",
                    ordinal.get()
                )
            }
            Self::DuplicateRoot => formatter.write_str("package scope contains a duplicate root"),
            Self::OverlappingRoots => {
                formatter.write_str("package scope contains overlapping roots")
            }
        }
    }
}

impl std::error::Error for PackageScopeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidRoot { source, .. } => Some(source),
            _ => None,
        }
    }
}

fn validate_canonical_roots(roots: &[RepositoryPath]) -> Result<(), PackageScopeError> {
    for (index, root) in roots.iter().enumerate() {
        for prior in &roots[..index] {
            if prior == root {
                return Err(PackageScopeError::DuplicateRoot);
            }
            if root_contains(prior, root) {
                return Err(PackageScopeError::OverlappingRoots);
            }
        }
    }
    Ok(())
}

fn root_contains(root: &RepositoryPath, path: &RepositoryPath) -> bool {
    path == root
        || path
            .as_bytes()
            .strip_prefix(root.as_bytes())
            .is_some_and(|suffix| suffix.first() == Some(&b'/'))
}

#[cfg(test)]
mod tests;
