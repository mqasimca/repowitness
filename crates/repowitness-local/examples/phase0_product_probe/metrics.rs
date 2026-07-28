use std::{
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
};

use repowitness_domain::RepositoryIdentityDigest;
use rusqlite::Connection;

use crate::ProbeResult;

pub fn active_configuration_digest(
    database: &Path,
    repository: RepositoryIdentityDigest,
) -> ProbeResult<[u8; 32]> {
    let connection = Connection::open(database)?;
    let bytes: Vec<u8> = connection.query_row(
        "SELECT snapshot.configuration_digest
         FROM workspaces AS workspace
         JOIN index_generations AS generation
           ON generation.generation_id = workspace.active_generation_id
         JOIN source_snapshots AS snapshot
           ON snapshot.snapshot_digest = generation.snapshot_digest
         WHERE workspace.repository_identity = ?1
           AND generation.lifecycle_state = 'active'",
        [repository.as_bytes().as_slice()],
        |row| row.get(0),
    )?;
    bytes
        .try_into()
        .map_err(|_| "active configuration digest was malformed".into())
}

pub fn hex_digest(digest: &[u8; 32]) -> String {
    let mut output = String::with_capacity(64);
    for byte in digest {
        write!(&mut output, "{byte:02x}").expect("writing to a String cannot fail");
    }
    output
}

pub fn file_size(path: &Path) -> u64 {
    fs::metadata(path).map_or(0, |metadata| metadata.len())
}

pub fn wal_file_size(database: &Path) -> u64 {
    let mut path = PathBuf::from(database);
    let mut name = path
        .file_name()
        .map_or_else(Default::default, std::ffi::OsString::from);
    name.push("-wal");
    path.set_file_name(name);
    file_size(&path)
}

#[cfg(test)]
mod tests {
    use super::hex_digest;

    #[test]
    fn digest_hex_is_lowercase_and_fixed_width() {
        assert_eq!(hex_digest(&[0xAB; 32]), "ab".repeat(32));
    }
}
