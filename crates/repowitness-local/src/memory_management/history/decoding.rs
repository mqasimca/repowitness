use repowitness_application::MemoryRecordIdTextV1;
use repowitness_domain::{MemoryCommitId, MemoryObjectFormat, MemoryRecordId};

use super::LocalMemoryManageError;

const MEMORY_PATH_PREFIX: &[u8] = b".code-memory/records/";
const YAML_SUFFIX: &[u8] = b".yaml";

pub(super) struct TreeEntry {
    pub(super) object_hex: String,
    pub(super) record_id: MemoryRecordId,
}

pub(super) fn parse_commit_lines(
    format: MemoryObjectFormat,
    output: &[u8],
) -> Result<Vec<MemoryCommitId>, LocalMemoryManageError> {
    if output.is_empty() {
        return Ok(Vec::new());
    }
    if output.last() != Some(&b'\n') {
        return Err(LocalMemoryManageError::HistoryUnavailable);
    }
    let mut commits = Vec::new();
    for line in output[..output.len() - 1].split(|byte| *byte == b'\n') {
        let commit =
            parse_object_id(format, line).ok_or(LocalMemoryManageError::HistoryUnavailable)?;
        commits.push(commit);
    }
    if commits.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(LocalMemoryManageError::HistoryUnavailable);
    }
    Ok(commits)
}

pub(super) fn parse_tree_entries(
    format: MemoryObjectFormat,
    output: &[u8],
) -> Result<Vec<TreeEntry>, LocalMemoryManageError> {
    if output.is_empty() {
        return Ok(Vec::new());
    }
    let records = output
        .strip_suffix(&[0])
        .ok_or(LocalMemoryManageError::HistoryUnavailable)?;
    let mut entries = Vec::new();
    for raw in records.split(|byte| *byte == 0) {
        let (metadata, path) = split_once(raw, b'\t')?;
        let mut fields = metadata.split(|byte| *byte == b' ');
        if fields.next() != Some(b"100644".as_slice()) || fields.next() != Some(b"blob".as_slice())
        {
            return Err(LocalMemoryManageError::HistoryUnavailable);
        }
        let object = fields
            .next()
            .ok_or(LocalMemoryManageError::HistoryUnavailable)?;
        if fields.next().is_some() || parse_object_id(format, object).is_none() {
            return Err(LocalMemoryManageError::HistoryUnavailable);
        }
        let record_id = record_id_from_path(path)?;
        let object_hex = std::str::from_utf8(object)
            .map_err(|_| LocalMemoryManageError::HistoryUnavailable)?
            .to_owned();
        entries.push(TreeEntry {
            object_hex,
            record_id,
        });
    }
    entries.sort_by_key(|entry| entry.record_id);
    if entries
        .windows(2)
        .any(|pair| pair[0].record_id == pair[1].record_id)
    {
        return Err(LocalMemoryManageError::HistoryUnavailable);
    }
    Ok(entries)
}

fn split_once(bytes: &[u8], delimiter: u8) -> Result<(&[u8], &[u8]), LocalMemoryManageError> {
    let index = bytes
        .iter()
        .position(|byte| *byte == delimiter)
        .ok_or(LocalMemoryManageError::HistoryUnavailable)?;
    Ok((&bytes[..index], &bytes[index + 1..]))
}

fn record_id_from_path(path: &[u8]) -> Result<MemoryRecordId, LocalMemoryManageError> {
    let name = path
        .strip_prefix(MEMORY_PATH_PREFIX)
        .and_then(|value| value.strip_suffix(YAML_SUFFIX))
        .filter(|value| !value.is_empty() && !value.contains(&b'/'))
        .ok_or(LocalMemoryManageError::HistoryUnavailable)?;
    let name = std::str::from_utf8(name).map_err(|_| LocalMemoryManageError::HistoryUnavailable)?;
    MemoryRecordIdTextV1::decode(name).map_err(|_| LocalMemoryManageError::HistoryUnavailable)
}

fn parse_object_id(format: MemoryObjectFormat, bytes: &[u8]) -> Option<MemoryCommitId> {
    let expected = match format {
        MemoryObjectFormat::Sha1 => 40,
        MemoryObjectFormat::Sha256 => 64,
    };
    if bytes.len() != expected {
        return None;
    }
    let mut decoded = [0_u8; 32];
    for (index, pair) in bytes.chunks_exact(2).enumerate() {
        decoded[index] = decode_hex(pair)?;
    }
    Some(match format {
        MemoryObjectFormat::Sha1 => {
            let mut value = [0_u8; 20];
            value.copy_from_slice(&decoded[..20]);
            MemoryCommitId::Sha1(value)
        }
        MemoryObjectFormat::Sha256 => MemoryCommitId::Sha256(decoded),
    })
}

fn decode_hex(pair: &[u8]) -> Option<u8> {
    Some(hex_nibble(*pair.first()?)? << 4 | hex_nibble(*pair.get(1)?)?)
}

const fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

pub(super) fn commit_hex(commit: MemoryCommitId) -> String {
    let mut output = String::with_capacity(commit.as_bytes().len() * 2);
    for byte in commit.as_bytes() {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    output
}
