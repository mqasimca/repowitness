use std::{
    fs,
    path::Path,
    process::Command,
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};

use repowitness_domain::{
    AnalysisArtifactDigest, ByteOffset, ByteSpan, DeclarationDigest, MemoryActorId,
    MemoryActorKind, MemoryAssurance, MemoryBody, MemoryClaim, MemoryCommitId,
    MemoryDisplayRevision, MemoryEvidence, MemoryEvidenceIndex, MemoryFactOrdinal, MemoryKind,
    MemoryLifecycle, MemoryProducerId, MemoryProducerVersion, MemoryProvenance,
    MemoryProvenanceOrigin, MemoryQualifiedName, MemoryRecord, MemoryRecordHeader, MemoryRecordId,
    MemoryScope, MemorySymbolName, MemoryTitle, MemoryValidity, ProducerIdentity,
    RepositoryIdentityDigest, RepositoryPath, RepositoryPathLimits, RustMemorySymbolKind,
    RustSymbolMemoryEvidence, SourceContentDigest, SourceSnapshotDigest,
};
use repowitness_local::{MemoryFormatControl, MemoryRecordIdTextV1, generate_memory_yaml};
use rusqlite::Connection;

use crate::ProbeResult;

const RECORD_ID: MemoryRecordId = MemoryRecordId::new([0xB7; 16]);
const TARGET_PATH: &[u8] = b"src/cmd/set.rs";
const MAX_TARGET_FILE_BYTES: u64 = 4 * 1024 * 1024;
const MUTATION: &[u8] = b"\n        // RepoWitness Phase 0 revalidation probe.\n";

pub struct ExactMemoryInput {
    yaml: Vec<u8>,
    record_id: String,
    target: ActiveOccurrence,
}

impl ExactMemoryInput {
    pub fn yaml(&self) -> &[u8] {
        &self.yaml
    }

    pub fn record_id(&self) -> &str {
        &self.record_id
    }

    pub fn mutate_target(&self, repository: &Path) -> ProbeResult<()> {
        mutate_target(repository, &self.target)
    }
}

pub fn exact_memory_input(
    repository_root: &Path,
    database: &Path,
    repository: RepositoryIdentityDigest,
) -> ProbeResult<ExactMemoryInput> {
    let target = active_occurrence(database, repository)?;
    let introduced_by = head_commit(repository_root)?;
    let evidence = rust_evidence(&target)?;
    let record = memory_record(repository, introduced_by, evidence)?;
    let cancelled = AtomicBool::new(false);
    let deadline = Instant::now()
        .checked_add(Duration::from_secs(10))
        .ok_or("memory serialization deadline is not representable")?;
    let yaml = generate_memory_yaml(&record, MemoryFormatControl::new(&cancelled, deadline))?;
    Ok(ExactMemoryInput {
        yaml,
        record_id: MemoryRecordIdTextV1::encode(RECORD_ID).into_string(),
        target,
    })
}

fn rust_evidence(target: &ActiveOccurrence) -> ProbeResult<RustSymbolMemoryEvidence> {
    let snapshot = SourceSnapshotDigest::try_from_slice(&target.snapshot)?;
    let name_start = persisted_u64(target.name_start)?;
    let name_end = persisted_u64(target.name_end)?;
    let declaration_start = persisted_u64(target.declaration_start)?;
    let declaration_end = persisted_u64(target.declaration_end)?;
    Ok(RustSymbolMemoryEvidence::try_new(
        snapshot,
        RepositoryPath::try_from_vec(
            target.path.clone(),
            RepositoryPathLimits::new(1_048_576, 65_535),
        )?,
        SourceContentDigest::try_from_slice(&target.content)?,
        AnalysisArtifactDigest::try_from_slice(&target.artifact)?,
        MemoryFactOrdinal::try_new(persisted_u64(target.fact_ordinal)?)?,
        memory_symbol_kind(&target.kind)?,
        MemorySymbolName::try_new(target.name.clone())?,
        MemoryQualifiedName::try_new(target.qualified_name.clone())?,
        ByteSpan::try_new(ByteOffset::new(name_start), ByteOffset::new(name_end))?,
        ByteSpan::try_new(
            ByteOffset::new(declaration_start),
            ByteOffset::new(declaration_end),
        )?,
        DeclarationDigest::try_from_slice(&target.declaration)?,
        ProducerIdentity::new(
            MemoryProducerId::try_new("repowitness.rust.syntax".to_owned())?,
            MemoryProducerVersion::try_new("phase0-rust-syntax-v1".to_owned())?,
        ),
    )?)
}

fn memory_record(
    repository: RepositoryIdentityDigest,
    introduced_by: MemoryCommitId,
    evidence: RustSymbolMemoryEvidence,
) -> ProbeResult<MemoryRecord> {
    Ok(MemoryRecord::try_new(
        MemoryRecordHeader::try_new(RECORD_ID, MemoryDisplayRevision::try_new(1)?, Vec::new())?,
        MemoryClaim::new(
            MemoryKind::Decision,
            MemoryTitle::try_new("SET into_frame encoding decision".to_owned())?,
            MemoryBody::try_new(
                "SET into_frame remains the evidence anchor for expiration encoding.".to_owned(),
            )?,
        ),
        MemoryScope::new(repository, MemoryEvidenceIndex::try_new(0)?),
        MemoryProvenance::new(
            MemoryProvenanceOrigin::Human,
            MemoryActorKind::LocalAsserted,
            MemoryActorId::try_new("phase0-benchmark".to_owned())?,
        ),
        MemoryAssurance::LocallyApproved,
        MemoryLifecycle::Active,
        MemoryValidity::try_commits(vec![introduced_by], Vec::new())?,
        vec![MemoryEvidence::RustSymbol(evidence)],
        Vec::new(),
        false,
    )?)
}

fn memory_symbol_kind(kind: &str) -> ProbeResult<RustMemorySymbolKind> {
    match kind {
        "function" => Ok(RustMemorySymbolKind::Function),
        "method" => Ok(RustMemorySymbolKind::Method),
        "struct" => Ok(RustMemorySymbolKind::Struct),
        "enum" => Ok(RustMemorySymbolKind::Enum),
        "union" => Ok(RustMemorySymbolKind::Union),
        "trait" => Ok(RustMemorySymbolKind::Trait),
        "module" => Ok(RustMemorySymbolKind::Module),
        "type_alias" => Ok(RustMemorySymbolKind::TypeAlias),
        "constant" => Ok(RustMemorySymbolKind::Constant),
        "static" => Ok(RustMemorySymbolKind::Static),
        "macro" => Ok(RustMemorySymbolKind::Macro),
        _ => Err("the benchmark target used an unsupported Rust symbol kind".into()),
    }
}

fn head_commit(repository: &Path) -> ProbeResult<MemoryCommitId> {
    let output = Command::new("git")
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "rev-parse",
            "--verify",
            "HEAD",
        ])
        .current_dir(repository)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()?;
    if !output.status.success() || !output.stderr.is_empty() || output.stdout.len() > 65 {
        return Err("the benchmark Git head was unavailable".into());
    }
    let text = std::str::from_utf8(&output.stdout)?.trim();
    match text.len() {
        40 => Ok(MemoryCommitId::Sha1(decode_hex(text)?)),
        64 => Ok(MemoryCommitId::Sha256(decode_hex(text)?)),
        _ => Err("the benchmark Git object identity was malformed".into()),
    }
}

fn decode_hex<const N: usize>(text: &str) -> ProbeResult<[u8; N]> {
    if text.len() != N * 2 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("Git object identity was not fixed-width hexadecimal".into());
    }
    let mut output = [0_u8; N];
    for (index, byte) in output.iter_mut().enumerate() {
        let offset = index * 2;
        *byte = u8::from_str_radix(&text[offset..offset + 2], 16)?;
    }
    Ok(output)
}

fn persisted_u64(value: i64) -> ProbeResult<u64> {
    Ok(u64::try_from(value).map_err(|_| "persisted occurrence integer was negative")?)
}

fn mutate_target(repository: &Path, target: &ActiveOccurrence) -> ProbeResult<()> {
    if target.path.as_slice() != TARGET_PATH {
        return Err("the exact memory target path was not the pinned SET implementation".into());
    }
    let path = repository.join("src/cmd/set.rs");
    let metadata = fs::symlink_metadata(&path)?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() > MAX_TARGET_FILE_BYTES
    {
        return Err("the benchmark mutation target was not an admitted regular file".into());
    }
    let source = fs::read(&path)?;
    let declaration_start = usize::try_from(target.declaration_start)?;
    let declaration_end = usize::try_from(target.declaration_end)?;
    let declaration = source
        .get(declaration_start..declaration_end)
        .ok_or("the persisted declaration span was outside the source")?;
    let relative_brace = declaration
        .iter()
        .position(|byte| *byte == b'{')
        .ok_or("the target method had no body delimiter")?;
    let insertion = declaration_start
        .checked_add(relative_brace)
        .and_then(|value| value.checked_add(1))
        .ok_or("benchmark mutation offset overflowed")?;
    let capacity = source
        .len()
        .checked_add(MUTATION.len())
        .ok_or("benchmark mutation capacity overflowed")?;
    let mut changed = Vec::with_capacity(capacity);
    changed.extend_from_slice(&source[..insertion]);
    changed.extend_from_slice(MUTATION);
    changed.extend_from_slice(&source[insertion..]);
    fs::write(path, changed)?;
    Ok(())
}

struct ActiveOccurrence {
    snapshot: Vec<u8>,
    path: Vec<u8>,
    content: Vec<u8>,
    artifact: Vec<u8>,
    fact_ordinal: i64,
    kind: String,
    name: String,
    qualified_name: String,
    name_start: i64,
    name_end: i64,
    declaration_start: i64,
    declaration_end: i64,
    declaration: Vec<u8>,
}

fn active_occurrence(
    database: &Path,
    repository: RepositoryIdentityDigest,
) -> ProbeResult<ActiveOccurrence> {
    let connection = Connection::open(database)?;
    Ok(connection.query_row(
        "SELECT generation.snapshot_digest, file.repository_path,
                file.content_digest, file.artifact_digest, fact.ordinal,
                fact.kind, fact.name, fact.qualified_name,
                fact.name_start, fact.name_end,
                fact.declaration_start, fact.declaration_end,
                correspondence.declaration_digest
         FROM workspaces AS workspace
         JOIN index_generations AS generation
           ON generation.generation_id = workspace.active_generation_id
         JOIN generation_files AS file
           ON file.generation_id = generation.generation_id
         JOIN analysis_artifacts AS artifact
           ON artifact.artifact_digest = file.artifact_digest
         JOIN artifact_facts AS fact
           ON fact.artifact_digest = file.artifact_digest
         JOIN artifact_fact_correspondence AS correspondence
           ON correspondence.artifact_digest = fact.artifact_digest
          AND correspondence.fact_ordinal = fact.ordinal
          AND correspondence.profile_id = 'rust-name-elided'
          AND correspondence.profile_version = 1
         WHERE workspace.repository_identity = ?1
           AND generation.lifecycle_state = 'active'
           AND artifact.language = 'rust'
           AND file.repository_path = ?2
           AND fact.name = 'into_frame'
         ORDER BY fact.ordinal
         LIMIT 1",
        (repository.as_bytes().as_slice(), TARGET_PATH),
        |row| {
            Ok(ActiveOccurrence {
                snapshot: row.get(0)?,
                path: row.get(1)?,
                content: row.get(2)?,
                artifact: row.get(3)?,
                fact_ordinal: row.get(4)?,
                kind: row.get(5)?,
                name: row.get(6)?,
                qualified_name: row.get(7)?,
                name_start: row.get(8)?,
                name_end: row.get(9)?,
                declaration_start: row.get(10)?,
                declaration_end: row.get(11)?,
                declaration: row.get(12)?,
            })
        },
    )?)
}

#[cfg(test)]
mod tests {
    use super::decode_hex;

    #[test]
    fn fixed_width_git_hex_is_strict() {
        assert_eq!(decode_hex::<2>("00ff").expect("valid hex"), [0, 255]);
        assert!(decode_hex::<2>("00fg").is_err());
        assert!(decode_hex::<2>("00").is_err());
    }
}
