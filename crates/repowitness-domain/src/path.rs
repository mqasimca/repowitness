//! Bounded, byte-preserving repository path identities.

use core::{
    cmp::Ordering,
    fmt,
    hash::{Hash, Hasher},
};

/// The semantic version of the repository-path domain contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RepositoryPathVersion(u16);

impl RepositoryPathVersion {
    /// The initial byte-preserving repository-path contract.
    pub const V1: Self = Self(1);

    /// Returns the fixed-width version number.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

/// A fixed-width repository-path byte count.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RepositoryPathByteCount(u64);

impl RepositoryPathByteCount {
    /// Creates a byte count from its fixed-width representation.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the fixed-width byte count.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// A fixed-width repository-path component count.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RepositoryPathComponentCount(u64);

impl RepositoryPathComponentCount {
    /// Creates a component count from its fixed-width representation.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the fixed-width component count.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Inclusive byte and component bounds for repository-path construction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RepositoryPathLimits {
    max_bytes: RepositoryPathByteCount,
    max_components: RepositoryPathComponentCount,
}

impl RepositoryPathLimits {
    /// Creates construction limits from fixed-width byte and component bounds.
    #[must_use]
    pub const fn new(max_bytes: u64, max_components: u64) -> Self {
        Self {
            max_bytes: RepositoryPathByteCount::new(max_bytes),
            max_components: RepositoryPathComponentCount::new(max_components),
        }
    }

    /// Returns the inclusive byte bound.
    #[must_use]
    pub const fn max_bytes(self) -> RepositoryPathByteCount {
        self.max_bytes
    }

    /// Returns the inclusive component bound.
    #[must_use]
    pub const fn max_components(self) -> RepositoryPathComponentCount {
        self.max_components
    }
}

/// Failure to construct a validated repository path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RepositoryPathError {
    /// The platform byte length cannot be represented as a `u64`.
    ByteCountNotRepresentable,
    /// The input exceeds its declared byte bound.
    ByteLimitExceeded {
        /// The input's actual byte count.
        actual: RepositoryPathByteCount,
        /// The inclusive byte bound.
        limit: RepositoryPathByteCount,
    },
    /// The path is empty.
    Empty,
    /// The path begins with `/`.
    LeadingSlash,
    /// The path ends with `/`.
    TrailingSlash,
    /// The path contains two adjacent `/` separators.
    EmptyComponent,
    /// The path contains a NUL byte.
    ContainsNul,
    /// The path contains an exact `.` component.
    CurrentDirectoryComponent,
    /// The path contains an exact `..` component.
    ParentDirectoryComponent,
    /// The path contains an exact `.git` component.
    DotGitComponent,
    /// The component count cannot be represented as a `u64`.
    ComponentCountNotRepresentable,
    /// The input exceeds its declared component bound.
    ComponentLimitExceeded {
        /// The input's actual component count when the limit was exceeded.
        actual: RepositoryPathComponentCount,
        /// The inclusive component bound.
        limit: RepositoryPathComponentCount,
    },
}

impl fmt::Display for RepositoryPathError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ByteCountNotRepresentable => {
                formatter.write_str("repository path byte count cannot be represented as a u64")
            }
            Self::ByteLimitExceeded { actual, limit } => write!(
                formatter,
                "repository path byte count {} exceeds limit {}",
                actual.get(),
                limit.get()
            ),
            Self::Empty => formatter.write_str("repository path cannot be empty"),
            Self::LeadingSlash => formatter.write_str("repository path cannot start with '/'"),
            Self::TrailingSlash => formatter.write_str("repository path cannot end with '/'"),
            Self::EmptyComponent => {
                formatter.write_str("repository path cannot contain an empty component")
            }
            Self::ContainsNul => formatter.write_str("repository path cannot contain NUL"),
            Self::CurrentDirectoryComponent => {
                formatter.write_str("repository path cannot contain a '.' component")
            }
            Self::ParentDirectoryComponent => {
                formatter.write_str("repository path cannot contain a '..' component")
            }
            Self::DotGitComponent => {
                formatter.write_str("repository path cannot contain a '.git' component")
            }
            Self::ComponentCountNotRepresentable => formatter
                .write_str("repository path component count cannot be represented as a u64"),
            Self::ComponentLimitExceeded { actual, limit } => write!(
                formatter,
                "repository path component count {} exceeds limit {}",
                actual.get(),
                limit.get()
            ),
        }
    }
}

impl std::error::Error for RepositoryPathError {}

/// An exact, repository-root-relative Git path.
///
/// The identity is a non-empty byte sequence whose only separator is ASCII
/// `/`. Construction rejects NUL, empty components, and the exact components
/// `.`, `..`, and `.git`. Every other byte is preserved without case folding,
/// Unicode normalization, or host-path parsing.
#[derive(Clone)]
pub struct RepositoryPath {
    bytes: Box<[u8]>,
    byte_count: RepositoryPathByteCount,
    component_count: RepositoryPathComponentCount,
}

impl RepositoryPath {
    /// The repository-path contract implemented by this value.
    pub const VERSION: RepositoryPathVersion = RepositoryPathVersion::V1;

    /// Validates and copies a borrowed repository path.
    ///
    /// Validation and limit checks complete before the owned byte allocation.
    ///
    /// # Errors
    ///
    /// Returns [`RepositoryPathError`] when the input exceeds either limit or
    /// violates the repository-relative path grammar.
    pub fn try_from_bytes(
        bytes: &[u8],
        limits: RepositoryPathLimits,
    ) -> Result<Self, RepositoryPathError> {
        let counts = validate(bytes, limits)?;
        Ok(Self::from_validated(bytes.into(), counts))
    }

    /// Validates an owned repository path without cloning its bytes.
    ///
    /// # Errors
    ///
    /// Returns [`RepositoryPathError`] when the input exceeds either limit or
    /// violates the repository-relative path grammar.
    pub fn try_from_vec(
        bytes: Vec<u8>,
        limits: RepositoryPathLimits,
    ) -> Result<Self, RepositoryPathError> {
        let counts = validate(&bytes, limits)?;
        Ok(Self::from_validated(bytes.into_boxed_slice(), counts))
    }

    fn from_validated(bytes: Box<[u8]>, counts: ValidatedCounts) -> Self {
        Self {
            bytes,
            byte_count: counts.bytes,
            component_count: counts.components,
        }
    }

    /// Returns the semantic version of this repository-path contract.
    #[must_use]
    pub const fn version(&self) -> RepositoryPathVersion {
        Self::VERSION
    }

    /// Returns the exact repository path bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Returns the fixed-width byte count.
    #[must_use]
    pub const fn byte_count(&self) -> RepositoryPathByteCount {
        self.byte_count
    }

    /// Returns the fixed-width component count.
    #[must_use]
    pub const fn component_count(&self) -> RepositoryPathComponentCount {
        self.component_count
    }

    /// Iterates over exact components in repository order.
    pub fn components(&self) -> impl DoubleEndedIterator<Item = &[u8]> {
        self.bytes.split(|byte| *byte == b'/')
    }

    /// Consumes the identity and returns its exact bytes.
    #[must_use]
    pub fn into_vec(self) -> Vec<u8> {
        self.bytes.into_vec()
    }
}

impl AsRef<[u8]> for RepositoryPath {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl fmt::Debug for RepositoryPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RepositoryPath")
            .field("byte_count", &self.byte_count)
            .field("component_count", &self.component_count)
            .finish_non_exhaustive()
    }
}

impl PartialEq for RepositoryPath {
    fn eq(&self, other: &Self) -> bool {
        self.bytes == other.bytes
    }
}

impl Eq for RepositoryPath {}

impl PartialOrd for RepositoryPath {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for RepositoryPath {
    fn cmp(&self, other: &Self) -> Ordering {
        self.bytes.cmp(&other.bytes)
    }
}

impl Hash for RepositoryPath {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.bytes.hash(state);
    }
}

#[derive(Clone, Copy)]
struct ValidatedCounts {
    bytes: RepositoryPathByteCount,
    components: RepositoryPathComponentCount,
}

fn validate(
    bytes: &[u8],
    limits: RepositoryPathLimits,
) -> Result<ValidatedCounts, RepositoryPathError> {
    let byte_count = validate_byte_count(bytes, limits.max_bytes())?;
    validate_shape(bytes)?;
    let component_count = validate_components(bytes, limits.max_components())?;

    Ok(ValidatedCounts {
        bytes: byte_count,
        components: component_count,
    })
}

fn validate_byte_count(
    bytes: &[u8],
    limit: RepositoryPathByteCount,
) -> Result<RepositoryPathByteCount, RepositoryPathError> {
    let actual = u64::try_from(bytes.len())
        .map(RepositoryPathByteCount::new)
        .map_err(|_| RepositoryPathError::ByteCountNotRepresentable)?;
    if actual.get() > limit.get() {
        return Err(RepositoryPathError::ByteLimitExceeded { actual, limit });
    }

    Ok(actual)
}

fn validate_shape(bytes: &[u8]) -> Result<(), RepositoryPathError> {
    if bytes.is_empty() {
        return Err(RepositoryPathError::Empty);
    }
    if bytes.first() == Some(&b'/') {
        return Err(RepositoryPathError::LeadingSlash);
    }
    if bytes.last() == Some(&b'/') {
        return Err(RepositoryPathError::TrailingSlash);
    }

    Ok(())
}

fn validate_components(
    bytes: &[u8],
    limit: RepositoryPathComponentCount,
) -> Result<RepositoryPathComponentCount, RepositoryPathError> {
    let mut component_count = 0_u64;
    for component in bytes.split(|byte| *byte == b'/') {
        component_count = component_count
            .checked_add(1)
            .ok_or(RepositoryPathError::ComponentCountNotRepresentable)?;
        let actual = RepositoryPathComponentCount::new(component_count);
        if actual.get() > limit.get() {
            return Err(RepositoryPathError::ComponentLimitExceeded { actual, limit });
        }
        validate_component(component)?;
    }

    Ok(RepositoryPathComponentCount::new(component_count))
}

fn validate_component(component: &[u8]) -> Result<(), RepositoryPathError> {
    if component.is_empty() {
        return Err(RepositoryPathError::EmptyComponent);
    }
    if component.contains(&0) {
        return Err(RepositoryPathError::ContainsNul);
    }
    match component {
        b"." => Err(RepositoryPathError::CurrentDirectoryComponent),
        b".." => Err(RepositoryPathError::ParentDirectoryComponent),
        b".git" => Err(RepositoryPathError::DotGitComponent),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use core::hash::{Hash, Hasher};
    use std::collections::hash_map::DefaultHasher;

    use super::{RepositoryPath, RepositoryPathError, RepositoryPathLimits, RepositoryPathVersion};

    const GENEROUS: RepositoryPathLimits = RepositoryPathLimits::new(1_024, 32);

    #[test]
    fn valid_path_preserves_exact_bytes_components_and_version() {
        let path = RepositoryPath::try_from_bytes(b"src/domain/lib.rs", GENEROUS)
            .expect("ordinary repository path must be valid");

        assert_eq!(path.version(), RepositoryPathVersion::V1);
        assert_eq!(RepositoryPathVersion::V1.get(), 1);
        assert_eq!(path.as_bytes(), b"src/domain/lib.rs");
        assert_eq!(path.byte_count().get(), 17);
        assert_eq!(path.component_count().get(), 3);
        assert_eq!(
            path.components().collect::<Vec<_>>(),
            [
                b"src".as_slice(),
                b"domain".as_slice(),
                b"lib.rs".as_slice()
            ]
        );
        assert_eq!(path.as_ref(), b"src/domain/lib.rs");
    }

    #[test]
    fn owned_construction_round_trips_without_capacity_or_encoding_changes() {
        let mut input = Vec::with_capacity(128);
        input.extend_from_slice(b"src/\xff.rs");

        let path = RepositoryPath::try_from_vec(input, GENEROUS)
            .expect("non-UTF-8 Git bytes are valid repository identity");

        assert_eq!(path.into_vec(), b"src/\xff.rs");
    }

    #[test]
    fn preserves_backslash_control_bytes_and_windows_looking_components() {
        for input in [
            b"unix\\name.rs".as_slice(),
            b"line\nbreak.rs".as_slice(),
            b"C:/source.rs".as_slice(),
            br"server\share/file.rs".as_slice(),
        ] {
            let path = RepositoryPath::try_from_bytes(input, GENEROUS)
                .expect("host-specific interpretation must not alter Git identity");
            assert_eq!(path.as_bytes(), input);
        }
    }

    #[test]
    fn accepts_every_single_byte_component_except_git_prohibited_bytes() {
        for byte in 1_u8..=u8::MAX {
            if matches!(byte, b'/' | b'.') {
                continue;
            }
            let path = RepositoryPath::try_from_bytes(&[byte], GENEROUS)
                .expect("all other single bytes are valid exact components");
            assert_eq!(path.as_bytes(), &[byte]);
        }
    }

    #[test]
    fn byte_and_component_limits_are_inclusive() {
        let exact = RepositoryPath::try_from_bytes(b"a/b", RepositoryPathLimits::new(3, 2))
            .expect("a path exactly at both limits must be valid");
        assert_eq!(exact.byte_count().get(), 3);
        assert_eq!(exact.component_count().get(), 2);

        assert_eq!(
            RepositoryPath::try_from_bytes(b"a/b", RepositoryPathLimits::new(2, 2))
                .expect_err("byte limit must reject an oversized path"),
            RepositoryPathError::ByteLimitExceeded {
                actual: super::RepositoryPathByteCount(3),
                limit: super::RepositoryPathByteCount(2),
            }
        );
        assert_eq!(
            RepositoryPath::try_from_bytes(b"a/b", RepositoryPathLimits::new(3, 1))
                .expect_err("component limit must reject an oversized path"),
            RepositoryPathError::ComponentLimitExceeded {
                actual: super::RepositoryPathComponentCount(2),
                limit: super::RepositoryPathComponentCount(1),
            }
        );
    }

    #[test]
    fn rejects_empty_absolute_trailing_and_empty_component_paths() {
        for (input, expected) in [
            (b"".as_slice(), RepositoryPathError::Empty),
            (b"/src".as_slice(), RepositoryPathError::LeadingSlash),
            (b"src/".as_slice(), RepositoryPathError::TrailingSlash),
            (
                b"src//lib.rs".as_slice(),
                RepositoryPathError::EmptyComponent,
            ),
        ] {
            assert_eq!(
                RepositoryPath::try_from_bytes(input, GENEROUS)
                    .expect_err("invalid repository grammar must be rejected"),
                expected
            );
        }
    }

    #[test]
    fn rejects_nul_and_prohibited_components_at_any_depth() {
        for (input, expected) in [
            (b"src/\0lib.rs".as_slice(), RepositoryPathError::ContainsNul),
            (
                b"./src.rs".as_slice(),
                RepositoryPathError::CurrentDirectoryComponent,
            ),
            (
                b"src/./lib.rs".as_slice(),
                RepositoryPathError::CurrentDirectoryComponent,
            ),
            (
                b"../src.rs".as_slice(),
                RepositoryPathError::ParentDirectoryComponent,
            ),
            (
                b"src/../lib.rs".as_slice(),
                RepositoryPathError::ParentDirectoryComponent,
            ),
            (
                b".git/config".as_slice(),
                RepositoryPathError::DotGitComponent,
            ),
            (
                b"src/.git/config".as_slice(),
                RepositoryPathError::DotGitComponent,
            ),
        ] {
            assert_eq!(
                RepositoryPath::try_from_bytes(input, GENEROUS)
                    .expect_err("Git-prohibited components must be rejected"),
                expected
            );
        }
    }

    #[test]
    fn preserves_case_and_unicode_bytes_without_normalization() {
        let lower = RepositoryPath::try_from_bytes(b"src/a.rs", GENEROUS).expect("valid path");
        let upper = RepositoryPath::try_from_bytes(b"src/A.rs", GENEROUS).expect("valid path");
        let composed = RepositoryPath::try_from_bytes("caf\u{e9}.rs".as_bytes(), GENEROUS)
            .expect("valid path");
        let decomposed = RepositoryPath::try_from_bytes("cafe\u{301}.rs".as_bytes(), GENEROUS)
            .expect("valid path");

        assert_ne!(lower, upper);
        assert_ne!(composed, decomposed);
        assert!(
            RepositoryPath::try_from_bytes(b".Git/config", GENEROUS).is_ok(),
            "only the exact Git-prohibited '.git' component is rejected"
        );
    }

    #[test]
    fn equality_hashing_and_ordering_depend_only_on_exact_bytes() {
        let narrow =
            RepositoryPath::try_from_bytes(b"src/lib.rs", RepositoryPathLimits::new(10, 2))
                .expect("path fits narrow limits");
        let broad = RepositoryPath::try_from_bytes(b"src/lib.rs", GENEROUS)
            .expect("path fits broad limits");
        assert_eq!(narrow, broad);
        assert_eq!(hash(&narrow), hash(&broad));

        let mut paths = [
            RepositoryPath::try_from_bytes(b"\x80", GENEROUS).expect("valid path"),
            RepositoryPath::try_from_bytes(b"a/b", GENEROUS).expect("valid path"),
            RepositoryPath::try_from_bytes(b"a", GENEROUS).expect("valid path"),
            RepositoryPath::try_from_bytes(b"\x7f", GENEROUS).expect("valid path"),
        ];
        paths.sort();
        assert_eq!(
            paths.map(RepositoryPath::into_vec),
            [
                b"a".to_vec(),
                b"a/b".to_vec(),
                b"\x7f".to_vec(),
                b"\x80".to_vec()
            ]
        );
    }

    #[test]
    fn debug_output_does_not_expose_path_bytes() {
        let path =
            RepositoryPath::try_from_bytes(b"private/customer.rs", GENEROUS).expect("valid path");
        let debug = format!("{path:?}");

        assert!(debug.contains("byte_count"));
        assert!(debug.contains("component_count"));
        assert!(!debug.contains("private"));
        assert!(!debug.contains("customer"));
    }

    #[test]
    fn errors_have_stable_non_path_diagnostics() {
        assert_eq!(
            RepositoryPathError::ByteCountNotRepresentable.to_string(),
            "repository path byte count cannot be represented as a u64"
        );
        assert_eq!(
            RepositoryPathError::Empty.to_string(),
            "repository path cannot be empty"
        );
        assert_eq!(
            RepositoryPathError::LeadingSlash.to_string(),
            "repository path cannot start with '/'"
        );
        assert_eq!(
            RepositoryPathError::TrailingSlash.to_string(),
            "repository path cannot end with '/'"
        );
        assert_eq!(
            RepositoryPathError::EmptyComponent.to_string(),
            "repository path cannot contain an empty component"
        );
        assert_eq!(
            RepositoryPathError::ContainsNul.to_string(),
            "repository path cannot contain NUL"
        );
        assert_eq!(
            RepositoryPathError::CurrentDirectoryComponent.to_string(),
            "repository path cannot contain a '.' component"
        );
        assert_eq!(
            RepositoryPathError::ParentDirectoryComponent.to_string(),
            "repository path cannot contain a '..' component"
        );
        assert_eq!(
            RepositoryPathError::DotGitComponent.to_string(),
            "repository path cannot contain a '.git' component"
        );
        assert_eq!(
            RepositoryPathError::ComponentCountNotRepresentable.to_string(),
            "repository path component count cannot be represented as a u64"
        );
        assert_eq!(
            RepositoryPathError::ByteLimitExceeded {
                actual: super::RepositoryPathByteCount(7),
                limit: super::RepositoryPathByteCount(3),
            }
            .to_string(),
            "repository path byte count 7 exceeds limit 3"
        );
        assert_eq!(
            RepositoryPathError::ComponentLimitExceeded {
                actual: super::RepositoryPathComponentCount(4),
                limit: super::RepositoryPathComponentCount(2),
            }
            .to_string(),
            "repository path component count 4 exceeds limit 2"
        );
    }

    fn hash(path: &RepositoryPath) -> u64 {
        let mut hasher = DefaultHasher::new();
        path.hash(&mut hasher);
        hasher.finish()
    }
}
