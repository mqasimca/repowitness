use std::path::{Path, PathBuf};

use repowitness_application::{
    ConnectedWorkspaceIdTextV1, RepositoryIdentityTextV1, RepositoryPathTextByteLimit,
    RepositoryPathTextV1, SourceSlotIdTextV1,
};
use repowitness_domain::{
    ConnectedWorkspaceId, RepositoryIdentityDigest, RepositoryPath, RepositoryPathLimits,
    SourceSlotId,
};

use super::{
    ConnectedWorkspaceManifestError, ConnectedWorkspaceManifestSourceError,
    ConnectedWorkspaceManifestV1, MAX_CONNECTED_WORKSPACE_MANIFEST_BYTES,
    MAX_CONNECTED_WORKSPACE_MANIFEST_SOURCES, MAX_CONNECTED_WORKSPACE_WORKTREE_ROOT_BYTES,
    parse_connected_workspace_manifest,
};

mod boundaries;
mod golden;
mod schema;

const TEST_PARENT: &str = "/authorized/manifests";
const PATH_LIMITS: RepositoryPathLimits = RepositoryPathLimits::new(1024 * 1024, 65_535);

fn workspace_text(byte: u8) -> String {
    ConnectedWorkspaceIdTextV1::encode(ConnectedWorkspaceId::new([byte; 32])).into_string()
}

fn slot_text(index: u16) -> String {
    let mut bytes = [0_u8; 32];
    bytes[30..].copy_from_slice(&index.to_be_bytes());
    SourceSlotIdTextV1::encode(SourceSlotId::new(bytes)).into_string()
}

fn repository_text(byte: u8) -> String {
    RepositoryIdentityTextV1::encode(RepositoryIdentityDigest::new([byte; 32])).into_string()
}

fn path_text(bytes: &[u8]) -> String {
    let path = RepositoryPath::try_from_bytes(bytes, PATH_LIMITS)
        .expect("fixture repository path should be valid");
    RepositoryPathTextV1::encode(&path, RepositoryPathTextByteLimit::new(1024 * 1024))
        .expect("fixture repository path text should fit")
        .into_string()
}

fn source_table(
    slot: &str,
    repository: &str,
    worktree_root: &str,
    selector_fields: &str,
    scope_fields: &str,
) -> String {
    format!(
        "\n[[source]]\nsource_slot_id = {slot:?}\nrepository_identity = {repository:?}\nworktree_root = {worktree_root:?}\nselector = {{ {selector_fields} }}\nscope = {{ {scope_fields} }}\n"
    )
}

fn whole_source(index: u16, repository: u8, worktree_root: &str) -> String {
    source_table(
        &slot_text(index),
        &repository_text(repository),
        worktree_root,
        "kind = \"worktree-head\"",
        "kind = \"whole-repository\"",
    )
}

fn manifest(sources: &[String]) -> String {
    let mut text = format!(
        "schema_version = 1\nconnected_workspace_id = {:?}\n",
        workspace_text(0xA1)
    );
    for source in sources {
        text.push_str(source);
    }
    text
}

fn parse(text: &str) -> Result<ConnectedWorkspaceManifestV1, ConnectedWorkspaceManifestError> {
    parse_connected_workspace_manifest(text.as_bytes(), Path::new(TEST_PARENT))
}

fn assert_invalid_source(
    result: Result<ConnectedWorkspaceManifestV1, ConnectedWorkspaceManifestError>,
    expected: ConnectedWorkspaceManifestSourceError,
) {
    assert_eq!(
        result,
        Err(ConnectedWorkspaceManifestError::InvalidSource {
            ordinal: 1,
            source: expected,
        })
    );
}

fn expected_relative_root(value: &str) -> PathBuf {
    Path::new(TEST_PARENT).join(value)
}
