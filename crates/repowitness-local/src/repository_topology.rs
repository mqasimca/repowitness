//! Path-only repository topology preparation for immutable source generations.

use std::{
    error::Error,
    fmt,
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

use repowitness_application::{REPOSITORY_TOPOLOGY_PROFILE_VERSION, RepositoryTopologyCategory};
use repowitness_domain::RepositoryPath;
use sha2::{Digest, Sha256};

const TOPOLOGY_DIGEST_DOMAIN: &[u8] = b"RepoWitness\0repository-topology\0";

/// Path-only topology ready for staging beside one source generation.
#[derive(Clone, Eq, PartialEq)]
pub struct PreparedRepositoryTopology {
    entries: Box<[(RepositoryPath, RepositoryTopologyCategory)]>,
    digest: [u8; 32],
}

impl fmt::Debug for PreparedRepositoryTopology {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedRepositoryTopology")
            .field(
                "entries",
                &format_args!("<{} redacted entries>", self.entries.len()),
            )
            .field("digest", &"<redacted-digest>")
            .finish()
    }
}

impl PreparedRepositoryTopology {
    /// Returns entries in canonical repository-byte-path order.
    #[must_use]
    pub const fn entries(&self) -> &[(RepositoryPath, RepositoryTopologyCategory)] {
        &self.entries
    }

    /// Returns the separate topology receipt digest.
    #[must_use]
    pub const fn digest(&self) -> &[u8; 32] {
        &self.digest
    }
}

/// Stable failure before complete topology output exists.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RepositoryTopologyPreparationError {
    /// Paths were duplicated or not in canonical order.
    InvalidPaths,
    /// Cancellation was observed before complete output existed.
    Cancelled,
    /// The monotonic deadline elapsed before complete output existed.
    DeadlineExceeded,
}

impl fmt::Display for RepositoryTopologyPreparationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidPaths => "repository topology paths are invalid",
            Self::Cancelled => "repository topology preparation cancelled",
            Self::DeadlineExceeded => "repository topology preparation deadline exceeded",
        })
    }
}

impl Error for RepositoryTopologyPreparationError {}

/// Classifies exact Git-discovered paths without opening any file content.
pub fn prepare_repository_topology(
    paths: Box<[RepositoryPath]>,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<PreparedRepositoryTopology, RepositoryTopologyPreparationError> {
    check_control(cancelled, deadline)?;
    if paths.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(RepositoryTopologyPreparationError::InvalidPaths);
    }
    let mut hash = repository_topology_hasher(REPOSITORY_TOPOLOGY_PROFILE_VERSION);
    for path in &paths {
        check_control(cancelled, deadline)?;
        let category = classify_path(path);
        update_repository_topology_hasher(&mut hash, path, category)?;
    }
    check_control(cancelled, deadline)?;
    let entries = paths
        .into_vec()
        .into_iter()
        .map(|path| {
            let category = classify_path(&path);
            (path, category)
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    Ok(PreparedRepositoryTopology {
        entries,
        digest: hash.finalize().into(),
    })
}

pub(crate) fn repository_topology_hasher(profile_version: u16) -> Sha256 {
    let mut hash = Sha256::new();
    hash.update(TOPOLOGY_DIGEST_DOMAIN);
    hash.update(profile_version.to_be_bytes());
    hash
}

pub(crate) fn update_repository_topology_hasher(
    hasher: &mut Sha256,
    path: &RepositoryPath,
    category: RepositoryTopologyCategory,
) -> Result<(), RepositoryTopologyPreparationError> {
    hasher.update(
        u64::try_from(path.as_bytes().len())
            .map_err(|_| RepositoryTopologyPreparationError::InvalidPaths)?
            .to_be_bytes(),
    );
    hasher.update(path.as_bytes());
    hasher.update(category.as_str().as_bytes());
    Ok(())
}

fn classify_path(path: &RepositoryPath) -> RepositoryTopologyCategory {
    let bytes = path.as_bytes();
    if is_agent_instruction(bytes) {
        RepositoryTopologyCategory::AgentInstruction
    } else if is_workflow_descriptor(bytes) {
        RepositoryTopologyCategory::WorkflowDescriptor
    } else if is_documentation(bytes) {
        RepositoryTopologyCategory::Documentation
    } else if is_build_descriptor(bytes) {
        RepositoryTopologyCategory::BuildDescriptor
    } else if is_package_descriptor(bytes) {
        RepositoryTopologyCategory::PackageDescriptor
    } else if is_configuration_descriptor(bytes) {
        RepositoryTopologyCategory::ConfigurationDescriptor
    } else {
        RepositoryTopologyCategory::OtherTrackedFile
    }
}

fn is_agent_instruction(path: &[u8]) -> bool {
    matches!(
        path.rsplit(|byte| *byte == b'/').next(),
        Some(b"AGENTS.md" | b"CLAUDE.md" | b"GEMINI.md" | b".cursorrules")
    ) || path == b".github/copilot-instructions.md"
}

fn is_workflow_descriptor(path: &[u8]) -> bool {
    path.strip_prefix(b".github/workflows/")
        .is_some_and(|name| name.ends_with(b".yml") || name.ends_with(b".yaml"))
}

fn is_documentation(path: &[u8]) -> bool {
    path.ends_with(b".md") || path.starts_with(b"docs/")
}

fn is_build_descriptor(path: &[u8]) -> bool {
    matches!(
        path.rsplit(|byte| *byte == b'/').next(),
        Some(
            b"Makefile"
                | b"GNUmakefile"
                | b"build.rs"
                | b"justfile"
                | b"Taskfile.yml"
                | b"Taskfile.yaml"
        )
    )
}

fn is_package_descriptor(path: &[u8]) -> bool {
    matches!(
        path.rsplit(|byte| *byte == b'/').next(),
        Some(
            b"Cargo.toml"
                | b"Cargo.lock"
                | b"go.mod"
                | b"go.sum"
                | b"package.json"
                | b"package-lock.json"
                | b"pnpm-lock.yaml"
                | b"yarn.lock"
                | b"pyproject.toml"
                | b"poetry.lock"
                | b"requirements.txt"
        )
    ) || path.starts_with(b"requirements/")
}

fn is_configuration_descriptor(path: &[u8]) -> bool {
    matches!(
        path,
        b".gitignore"
            | b".gitattributes"
            | b".editorconfig"
            | b"rust-toolchain.toml"
            | b"rustfmt.toml"
            | b"clippy.toml"
            | b"deny.toml"
            | b".cargo/config.toml"
    ) || path.starts_with(b"config/")
}

fn check_control(
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), RepositoryTopologyPreparationError> {
    if cancelled.load(Ordering::Acquire) {
        Err(RepositoryTopologyPreparationError::Cancelled)
    } else if Instant::now() >= deadline {
        Err(RepositoryTopologyPreparationError::DeadlineExceeded)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::atomic::AtomicBool,
        time::{Duration, Instant},
    };

    use repowitness_application::RepositoryTopologyCategory;
    use repowitness_domain::{RepositoryPath, RepositoryPathLimits};

    use super::prepare_repository_topology;

    #[test]
    fn classifies_the_fixed_path_only_allow_list_without_opening_files() {
        let limits = RepositoryPathLimits::new(1024, 32);
        let mut paths = [
            b".github/workflows/ci.yml".as_slice(),
            b"AGENTS.md",
            b"Cargo.toml",
            b"README.md",
            b"config/local.toml",
            b"src/lib.rs",
            b"Makefile",
        ]
        .into_iter()
        .map(|path| RepositoryPath::try_from_bytes(path, limits).expect("path"))
        .collect::<Vec<_>>();
        paths.sort();
        let topology = prepare_repository_topology(
            paths.into_boxed_slice(),
            &AtomicBool::new(false),
            Instant::now() + Duration::from_secs(1),
        )
        .expect("topology");
        let categories = topology
            .entries()
            .iter()
            .map(|(_, category)| *category)
            .collect::<Vec<_>>();
        assert!(categories.contains(&RepositoryTopologyCategory::AgentInstruction));
        assert!(categories.contains(&RepositoryTopologyCategory::WorkflowDescriptor));
        assert!(categories.contains(&RepositoryTopologyCategory::Documentation));
        assert!(categories.contains(&RepositoryTopologyCategory::BuildDescriptor));
        assert!(categories.contains(&RepositoryTopologyCategory::PackageDescriptor));
        assert!(categories.contains(&RepositoryTopologyCategory::ConfigurationDescriptor));
        assert!(categories.contains(&RepositoryTopologyCategory::OtherTrackedFile));
    }
}
