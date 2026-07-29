use std::{
    path::{Path, PathBuf},
    str,
};

use repowitness_application::{
    ConnectedWorkspaceIdTextV1, MAX_PACKAGE_SCOPE_ROOTS, PackageScope, PackageScopeError,
    RepositoryIdentityTextV1, RepositoryPathTextByteLimit, RepositoryPathTextV1,
    SourceSlotIdTextV1,
};

use super::{
    ConnectedWorkspaceManifestError, ConnectedWorkspaceManifestSourceError,
    ConnectedWorkspaceManifestSourceV1, ConnectedWorkspaceManifestV1,
    dto::{ManifestDto, ScopeDto, SelectorDto, SourceDto},
};
use crate::{
    GitPathDiscoveryLimits,
    source_selector::{SourceSelectorCategory, SourceSelectorV1},
};

/// Strict connected-workspace manifest version.
pub(crate) const CONNECTED_WORKSPACE_MANIFEST_SCHEMA_VERSION: u16 = 1;
/// Inclusive one-mebibyte manifest input bound.
pub(crate) const MAX_CONNECTED_WORKSPACE_MANIFEST_BYTES: usize = 1024 * 1024;
/// Inclusive source-tuple bound.
pub(crate) const MAX_CONNECTED_WORKSPACE_MANIFEST_SOURCES: usize = 256;
/// Inclusive UTF-8 byte bound for one worktree root.
pub(crate) const MAX_CONNECTED_WORKSPACE_WORKTREE_ROOT_BYTES: usize = 4096;

/// Parses one bounded manifest from already-admitted bytes.
///
/// Relative roots are joined lexically to `manifest_parent`; this function
/// performs no filesystem access, canonicalization, normalization, or symlink
/// traversal.
pub(crate) fn parse_connected_workspace_manifest(
    bytes: &[u8],
    manifest_parent: &Path,
) -> Result<ConnectedWorkspaceManifestV1, ConnectedWorkspaceManifestError> {
    if bytes.len() > MAX_CONNECTED_WORKSPACE_MANIFEST_BYTES {
        return Err(ConnectedWorkspaceManifestError::InputTooLarge {
            limit: u64::try_from(MAX_CONNECTED_WORKSPACE_MANIFEST_BYTES)
                .expect("manifest byte limit fits u64"),
        });
    }
    let text = str::from_utf8(bytes).map_err(|_| ConnectedWorkspaceManifestError::InvalidUtf8)?;
    let dto = toml::from_str::<ManifestDto>(text)
        .map_err(|_| ConnectedWorkspaceManifestError::InvalidToml)?;
    if dto.schema_version != u64::from(CONNECTED_WORKSPACE_MANIFEST_SCHEMA_VERSION) {
        return Err(ConnectedWorkspaceManifestError::UnsupportedSchemaVersion);
    }
    validate_source_count(dto.sources.len())?;
    let connected_workspace = ConnectedWorkspaceIdTextV1::decode(&dto.connected_workspace_id)
        .map_err(
            |source| ConnectedWorkspaceManifestError::InvalidConnectedWorkspaceId { source },
        )?;
    let mut sources = dto
        .sources
        .into_iter()
        .enumerate()
        .map(|(index, source)| {
            let ordinal = u64::try_from(index + 1).expect("bounded source ordinal fits u64");
            parse_source(source, manifest_parent).map_err(|source| {
                ConnectedWorkspaceManifestError::InvalidSource { ordinal, source }
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    sources.sort_unstable_by_key(ConnectedWorkspaceManifestSourceV1::source_slot);
    if sources
        .windows(2)
        .any(|pair| pair[0].source_slot() == pair[1].source_slot())
    {
        return Err(ConnectedWorkspaceManifestError::DuplicateSourceSlot);
    }
    Ok(ConnectedWorkspaceManifestV1::new(
        connected_workspace,
        sources.into_boxed_slice(),
    ))
}

fn validate_source_count(count: usize) -> Result<(), ConnectedWorkspaceManifestError> {
    if !(1..=MAX_CONNECTED_WORKSPACE_MANIFEST_SOURCES).contains(&count) {
        return Err(ConnectedWorkspaceManifestError::SourceCountOutOfRange {
            minimum: 1,
            maximum: u64::try_from(MAX_CONNECTED_WORKSPACE_MANIFEST_SOURCES)
                .expect("source limit fits u64"),
        });
    }
    Ok(())
}

fn parse_source(
    dto: SourceDto,
    manifest_parent: &Path,
) -> Result<ConnectedWorkspaceManifestSourceV1, ConnectedWorkspaceManifestSourceError> {
    let source_slot = SourceSlotIdTextV1::decode(&dto.source_slot_id)
        .map_err(|source| ConnectedWorkspaceManifestSourceError::SourceSlotId { source })?;
    let repository = RepositoryIdentityTextV1::decode(&dto.repository_identity)
        .map_err(|source| ConnectedWorkspaceManifestSourceError::RepositoryIdentity { source })?;
    let worktree_root = parse_worktree_root(dto.worktree_root, manifest_parent)?;
    let selector = parse_selector(dto.selector)?;
    let package_scope = parse_scope(dto.scope)?;
    Ok(ConnectedWorkspaceManifestSourceV1::new(
        source_slot,
        repository,
        worktree_root,
        selector,
        package_scope,
    ))
}

fn parse_worktree_root(
    text: String,
    manifest_parent: &Path,
) -> Result<PathBuf, ConnectedWorkspaceManifestSourceError> {
    if text.is_empty()
        || text.len() > MAX_CONNECTED_WORKSPACE_WORKTREE_ROOT_BYTES
        || text.as_bytes().contains(&0)
    {
        return Err(ConnectedWorkspaceManifestSourceError::WorktreeRoot);
    }
    let root = PathBuf::from(text);
    if root.is_absolute() {
        Ok(root)
    } else {
        Ok(manifest_parent.join(root))
    }
}

fn parse_selector(
    dto: SelectorDto,
) -> Result<SourceSelectorV1, ConnectedWorkspaceManifestSourceError> {
    let (text, expected) = match (dto.kind.as_str(), dto.value.as_deref()) {
        ("worktree-head", None) => ("worktree-head", SourceSelectorCategory::WorktreeHead),
        ("exact-revision", Some(value)) => (value, SourceSelectorCategory::ExactRevision),
        ("full-ref", Some(value)) => (value, SourceSelectorCategory::FullRef),
        _ => return Err(ConnectedWorkspaceManifestSourceError::SelectorShape),
    };
    let selector = SourceSelectorV1::parse(text)
        .map_err(|source| ConnectedWorkspaceManifestSourceError::Selector { source })?;
    if selector.category() != expected {
        return Err(ConnectedWorkspaceManifestSourceError::SelectorShape);
    }
    Ok(selector)
}

fn parse_scope(dto: ScopeDto) -> Result<PackageScope, ConnectedWorkspaceManifestSourceError> {
    match (dto.kind.as_str(), dto.roots) {
        ("whole-repository", None) => Ok(PackageScope::whole_repository()),
        ("explicit-roots", Some(roots)) => parse_explicit_roots(roots),
        _ => Err(ConnectedWorkspaceManifestSourceError::ScopeShape),
    }
}

fn parse_explicit_roots(
    encoded_roots: Vec<String>,
) -> Result<PackageScope, ConnectedWorkspaceManifestSourceError> {
    if encoded_roots.len()
        > usize::try_from(MAX_PACKAGE_SCOPE_ROOTS.get()).expect("package root limit fits usize")
    {
        return Err(ConnectedWorkspaceManifestSourceError::PackageScope {
            source: PackageScopeError::RootLimitExceeded {
                limit: MAX_PACKAGE_SCOPE_ROOTS,
            },
        });
    }
    let encoded_limit = RepositoryPathTextByteLimit::new(
        u64::try_from(MAX_CONNECTED_WORKSPACE_MANIFEST_BYTES)
            .expect("manifest byte limit fits u64"),
    );
    let path_limits = GitPathDiscoveryLimits::default().repository_path();
    let roots = encoded_roots
        .into_iter()
        .enumerate()
        .map(|(index, encoded)| {
            let ordinal = u64::try_from(index + 1).expect("bounded package root ordinal fits u64");
            RepositoryPathTextV1::decode(&encoded, encoded_limit, path_limits).map_err(|source| {
                ConnectedWorkspaceManifestSourceError::PackageRoot { ordinal, source }
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    PackageScope::try_explicit_roots(roots)
        .map_err(|source| ConnectedWorkspaceManifestSourceError::PackageScope { source })
}
