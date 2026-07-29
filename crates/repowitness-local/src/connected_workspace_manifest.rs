//! Strict version-1 connected-workspace manifest admission.

mod dto;
mod error;
mod model;
mod parser;

pub(crate) use error::{ConnectedWorkspaceManifestError, ConnectedWorkspaceManifestSourceError};
pub(crate) use model::{ConnectedWorkspaceManifestSourceV1, ConnectedWorkspaceManifestV1};
pub(crate) use parser::{
    CONNECTED_WORKSPACE_MANIFEST_SCHEMA_VERSION, MAX_CONNECTED_WORKSPACE_MANIFEST_BYTES,
    parse_connected_workspace_manifest,
};
#[cfg(test)]
pub(crate) use parser::{
    MAX_CONNECTED_WORKSPACE_MANIFEST_SOURCES, MAX_CONNECTED_WORKSPACE_WORKTREE_ROOT_BYTES,
};

#[cfg(test)]
mod tests;
