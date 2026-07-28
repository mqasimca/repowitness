use std::{
    error::Error,
    fmt,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

use repowitness_application::MemoryRecordIdTextV1;
use repowitness_domain::{
    CanonicalMemoryDigest, MemoryPresentationDigest, MemoryRecord, MemoryRecordId, RepositoryPath,
    RepositoryPathLimits,
};
use sha2::{Digest, Sha256};

use crate::{
    ContainedSourceError, ContainedSourceRoot, MAX_MEMORY_YAML_BYTES, MemoryFormatControl,
    MemoryFormatError, ParsedMemoryRecord, SourceReadLimits, parse_memory_record,
};

const MEMORY_RECORD_PATH_LIMITS: RepositoryPathLimits = RepositoryPathLimits::new(64, 3);
const MEMORY_READ_CHUNK_BYTES: u64 = 16 * 1024;
const MEMORY_DIRECTORY: &[u8] = b".code-memory/records/";
const YAML_SUFFIX: &[u8] = b".yaml";

/// Capability-contained reader for canonical worktree memory-record files.
pub struct MemoryRecordFiles {
    source: ContainedSourceRoot,
}

impl MemoryRecordFiles {
    /// Opens one explicitly authorized repository root.
    pub fn open(repository_root: &Path) -> Result<Self, MemoryFileImportError> {
        let source = ContainedSourceRoot::open(repository_root)
            .map_err(|_| MemoryFileImportError::RepositoryUnavailable)?;
        Ok(Self { source })
    }

    /// Loads one exact canonical record path under an absolute operation deadline.
    pub fn load(
        &self,
        expected_record_id: MemoryRecordId,
        cancelled: &AtomicBool,
        deadline: Instant,
    ) -> Result<LoadedMemoryRecord, MemoryFileImportError> {
        check_control(cancelled, deadline)?;
        let path = memory_record_path(expected_record_id)?;
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(MemoryFileImportError::DeadlineExceeded);
        }
        let limits = SourceReadLimits::try_new(
            remaining,
            MAX_MEMORY_YAML_BYTES as u64,
            MEMORY_READ_CHUNK_BYTES,
        )
        .map_err(|_| MemoryFileImportError::ConfigurationInvalid)?;
        let bytes = self
            .source
            .read_unique_exact_with_cancel(&path, limits, || cancelled.load(Ordering::Acquire))
            .map_err(map_source_error)?;
        let parsed = parse_memory_record(&bytes, MemoryFormatControl::new(cancelled, deadline))
            .map_err(map_format_error)?;
        if parsed.record().header().record_id() != expected_record_id {
            return Err(MemoryFileImportError::RecordIdMismatch);
        }
        check_control(cancelled, deadline)?;
        let presentation = MemoryPresentationDigest::new(Sha256::digest(&bytes).into());
        Ok(LoadedMemoryRecord {
            parsed,
            presentation,
        })
    }
}

impl fmt::Debug for MemoryRecordFiles {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MemoryRecordFiles")
            .field("source", &self.source)
            .finish()
    }
}

/// One validated record plus semantic and exact-presentation identities.
pub struct LoadedMemoryRecord {
    parsed: ParsedMemoryRecord,
    presentation: MemoryPresentationDigest,
}

impl LoadedMemoryRecord {
    /// Returns the validated semantic record.
    #[must_use]
    pub const fn record(&self) -> &MemoryRecord {
        self.parsed.record()
    }

    /// Returns the canonical semantic revision identity.
    #[must_use]
    pub const fn revision(&self) -> CanonicalMemoryDigest {
        self.parsed.digest()
    }

    /// Returns the SHA-256 receipt for exact admitted YAML bytes.
    #[must_use]
    pub const fn presentation(&self) -> MemoryPresentationDigest {
        self.presentation
    }

    /// Consumes the load result into values needed by the application import.
    #[must_use]
    pub fn into_parts(
        self,
    ) -> (
        MemoryRecord,
        CanonicalMemoryDigest,
        MemoryPresentationDigest,
    ) {
        let revision = self.parsed.digest();
        (self.parsed.into_record(), revision, self.presentation)
    }
}

impl fmt::Debug for LoadedMemoryRecord {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LoadedMemoryRecord")
            .field("parsed", &self.parsed)
            .field("presentation", &self.presentation)
            .finish()
    }
}

/// Stable, path- and content-redacted worktree memory admission failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryFileImportError {
    /// The explicitly authorized repository root could not be opened.
    RepositoryUnavailable,
    /// The exact canonical file path could not be admitted or read.
    FileUnavailable,
    /// The exact path resolved to a special file.
    NotRegularFile,
    /// The file had multiple links or its link count could not be proved.
    MultipleLinks,
    /// The file exceeded the inclusive 64 KiB YAML limit.
    InputTooLarge,
    /// File bytes did not form one accepted version-1 memory record.
    InvalidRecord,
    /// The decoded record identity did not match its canonical filename.
    RecordIdMismatch,
    /// A fixed internal admission limit could not be constructed.
    ConfigurationInvalid,
    /// Cancellation was visible before complete admission.
    Cancelled,
    /// The absolute admission deadline elapsed.
    DeadlineExceeded,
}

impl fmt::Display for MemoryFileImportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::RepositoryUnavailable => "memory repository root is unavailable",
            Self::FileUnavailable => "memory record file is unavailable",
            Self::NotRegularFile => "memory record path is not a regular file",
            Self::MultipleLinks => "memory record file does not have one unique link",
            Self::InputTooLarge => "memory record file exceeds its byte limit",
            Self::InvalidRecord => "memory record file is invalid",
            Self::RecordIdMismatch => "memory record identity does not match its filename",
            Self::ConfigurationInvalid => "memory file admission configuration is invalid",
            Self::Cancelled => "memory file admission was cancelled",
            Self::DeadlineExceeded => "memory file admission deadline elapsed",
        })
    }
}

impl Error for MemoryFileImportError {}

fn memory_record_path(record_id: MemoryRecordId) -> Result<RepositoryPath, MemoryFileImportError> {
    let record_id = MemoryRecordIdTextV1::encode(record_id);
    let mut bytes = Vec::with_capacity(
        MEMORY_DIRECTORY
            .len()
            .saturating_add(record_id.as_str().len())
            .saturating_add(YAML_SUFFIX.len()),
    );
    bytes.extend_from_slice(MEMORY_DIRECTORY);
    bytes.extend_from_slice(record_id.as_str().as_bytes());
    bytes.extend_from_slice(YAML_SUFFIX);
    RepositoryPath::try_from_vec(bytes, MEMORY_RECORD_PATH_LIMITS)
        .map_err(|_| MemoryFileImportError::ConfigurationInvalid)
}

fn check_control(cancelled: &AtomicBool, deadline: Instant) -> Result<(), MemoryFileImportError> {
    if cancelled.load(Ordering::Acquire) {
        return Err(MemoryFileImportError::Cancelled);
    }
    if Instant::now() >= deadline {
        return Err(MemoryFileImportError::DeadlineExceeded);
    }
    Ok(())
}

fn map_source_error(error: ContainedSourceError) -> MemoryFileImportError {
    match error {
        ContainedSourceError::RootOpen { .. } | ContainedSourceError::RootClone { .. } => {
            MemoryFileImportError::RepositoryUnavailable
        }
        ContainedSourceError::NotRegularFile => MemoryFileImportError::NotRegularFile,
        ContainedSourceError::LinkCountNotUnique => MemoryFileImportError::MultipleLinks,
        ContainedSourceError::FileByteLimitExceeded { .. } => MemoryFileImportError::InputTooLarge,
        ContainedSourceError::Cancelled => MemoryFileImportError::Cancelled,
        ContainedSourceError::DeadlineNotRepresentable
        | ContainedSourceError::DeadlineExceeded { .. } => MemoryFileImportError::DeadlineExceeded,
        ContainedSourceError::UnsupportedPathEncoding
        | ContainedSourceError::ComponentCountOverflowed
        | ContainedSourceError::DirectoryOpen { .. }
        | ContainedSourceError::FileOpen { .. }
        | ContainedSourceError::MetadataRead { .. }
        | ContainedSourceError::ExactComponentUnavailable { .. }
        | ContainedSourceError::DirectoryEntryLimitExceeded { .. }
        | ContainedSourceError::DirectoryEntryRead { .. }
        | ContainedSourceError::FileRead { .. }
        | ContainedSourceError::RepositoryPathHadNoComponents => {
            MemoryFileImportError::FileUnavailable
        }
    }
}

fn map_format_error(error: MemoryFormatError) -> MemoryFileImportError {
    match error {
        MemoryFormatError::InputTooLarge => MemoryFileImportError::InputTooLarge,
        MemoryFormatError::Cancelled => MemoryFileImportError::Cancelled,
        MemoryFormatError::DeadlineExceeded => MemoryFileImportError::DeadlineExceeded,
        MemoryFormatError::InvalidYaml
        | MemoryFormatError::InvalidRecord(_)
        | MemoryFormatError::InvalidCanonicalRecord
        | MemoryFormatError::CanonicalizationFailed
        | MemoryFormatError::GenerationFailed => MemoryFileImportError::InvalidRecord,
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicBool, AtomicU64, Ordering},
        time::{Duration, Instant},
    };

    use repowitness_application::MemoryRecordIdTextV1;
    use repowitness_domain::{MemoryPresentationDigest, MemoryRecordId};
    use sha2::{Digest, Sha256};

    use super::{MAX_MEMORY_YAML_BYTES, MemoryFileImportError, MemoryRecordFiles};

    const COMMIT_YAML: &[u8] = include_bytes!("../tests/fixtures/memory-v1/commit.yaml");
    const WORKTREE_YAML: &[u8] =
        include_bytes!("../tests/fixtures/memory-v1/worktree-relationship.yaml");
    static NEXT_FIXTURE_ID: AtomicU64 = AtomicU64::new(0);

    struct TempDirectory {
        root: PathBuf,
    }

    impl TempDirectory {
        fn new() -> Self {
            let fixture_id = NEXT_FIXTURE_ID.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "repowitness-memory-import-{}-{fixture_id}",
                std::process::id()
            ));
            fs::create_dir(&root).expect("fixture directory must be created");
            Self { root }
        }

        #[cfg(unix)]
        fn new_short() -> Self {
            let fixture_id = NEXT_FIXTURE_ID.fetch_add(1, Ordering::Relaxed);
            let root =
                PathBuf::from("/tmp").join(format!("rwmi-{}-{fixture_id}", std::process::id()));
            fs::create_dir(&root).expect("short fixture directory must be created");
            Self { root }
        }

        fn path(&self) -> &Path {
            &self.root
        }
    }

    impl Drop for TempDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn records_directory(root: &Path) -> PathBuf {
        root.join(".code-memory/records")
    }

    fn record_path(root: &Path, id: MemoryRecordId) -> PathBuf {
        records_directory(root).join(format!(
            "{}.yaml",
            MemoryRecordIdTextV1::encode(id).as_str()
        ))
    }

    fn write_record(root: &Path, id: MemoryRecordId, bytes: &[u8]) {
        fs::create_dir_all(records_directory(root))
            .expect("memory records directory must be created");
        fs::write(record_path(root, id), bytes).expect("memory fixture must be written");
    }

    fn load(
        root: &Path,
        id: MemoryRecordId,
        cancelled: &AtomicBool,
    ) -> Result<super::LoadedMemoryRecord, MemoryFileImportError> {
        MemoryRecordFiles::open(root)?.load(id, cancelled, Instant::now() + Duration::from_secs(5))
    }

    #[test]
    fn canonical_file_loads_with_semantic_and_presentation_receipts() {
        let fixture = TempDirectory::new();
        let id = MemoryRecordId::new([0; 16]);
        write_record(fixture.path(), id, COMMIT_YAML);
        let loaded =
            load(fixture.path(), id, &AtomicBool::new(false)).expect("canonical file must load");

        assert_eq!(loaded.record().header().record_id(), id);
        assert_eq!(
            loaded.presentation(),
            MemoryPresentationDigest::new(Sha256::digest(COMMIT_YAML).into())
        );
        let (record, revision, presentation) = loaded.into_parts();
        assert_eq!(record.header().record_id(), id);
        assert_eq!(revision.as_bytes().len(), 32);
        assert_eq!(
            presentation,
            MemoryPresentationDigest::new(Sha256::digest(COMMIT_YAML).into())
        );
    }

    #[test]
    fn filename_identity_mismatch_and_alternate_case_fail_closed() {
        let mismatch = TempDirectory::new();
        let ff_id = MemoryRecordId::new([0xff; 16]);
        write_record(mismatch.path(), ff_id, COMMIT_YAML);
        assert_eq!(
            load(mismatch.path(), ff_id, &AtomicBool::new(false))
                .expect_err("record ID mismatch must fail"),
            MemoryFileImportError::RecordIdMismatch
        );

        let alternate = TempDirectory::new();
        fs::create_dir_all(records_directory(alternate.path()))
            .expect("records directory must be created");
        let lowercase_name = format!(
            "{}.yaml",
            MemoryRecordIdTextV1::encode(ff_id)
                .as_str()
                .to_ascii_lowercase()
        );
        fs::write(
            records_directory(alternate.path()).join(lowercase_name),
            WORKTREE_YAML,
        )
        .expect("alternate-case fixture must be written");
        assert_eq!(
            load(alternate.path(), ff_id, &AtomicBool::new(false))
                .expect_err("alternate-case filename must fail"),
            MemoryFileImportError::FileUnavailable
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlink_and_special_file_targets_are_rejected() {
        use std::{os::unix::fs::symlink, os::unix::net::UnixListener};

        let symlink_fixture = TempDirectory::new();
        let outside = TempDirectory::new();
        let id = MemoryRecordId::new([0; 16]);
        fs::write(outside.path().join("record.yaml"), COMMIT_YAML)
            .expect("outside record must be written");
        fs::create_dir_all(records_directory(symlink_fixture.path()))
            .expect("records directory must be created");
        symlink(
            outside.path().join("record.yaml"),
            record_path(symlink_fixture.path(), id),
        )
        .expect("symlink fixture must be created");
        assert_eq!(
            load(symlink_fixture.path(), id, &AtomicBool::new(false))
                .expect_err("symlink must fail"),
            MemoryFileImportError::FileUnavailable
        );

        let special_fixture = TempDirectory::new_short();
        fs::create_dir_all(records_directory(special_fixture.path()))
            .expect("records directory must be created");
        let _listener = UnixListener::bind(record_path(special_fixture.path(), id))
            .expect("socket fixture must be created");
        assert_eq!(
            load(special_fixture.path(), id, &AtomicBool::new(false))
                .expect_err("special file must fail"),
            MemoryFileImportError::FileUnavailable
        );
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn hard_link_target_is_rejected() {
        let fixture = TempDirectory::new();
        let outside = TempDirectory::new();
        let id = MemoryRecordId::new([0; 16]);
        let source = outside.path().join("record.yaml");
        fs::write(&source, COMMIT_YAML).expect("outside record must be written");
        fs::create_dir_all(records_directory(fixture.path()))
            .expect("records directory must be created");
        fs::hard_link(source, record_path(fixture.path(), id))
            .expect("hard-link fixture must be created");

        assert_eq!(
            load(fixture.path(), id, &AtomicBool::new(false)).expect_err("hard link must fail"),
            MemoryFileImportError::MultipleLinks
        );
    }

    #[test]
    fn byte_parse_control_and_debug_failures_are_redacted() {
        let oversized = TempDirectory::new();
        let id = MemoryRecordId::new([0; 16]);
        write_record(oversized.path(), id, &vec![b'x'; MAX_MEMORY_YAML_BYTES + 1]);
        assert_eq!(
            load(oversized.path(), id, &AtomicBool::new(false))
                .expect_err("oversized file must fail"),
            MemoryFileImportError::InputTooLarge
        );

        let malformed = TempDirectory::new();
        write_record(malformed.path(), id, b"private: [unterminated");
        let invalid = load(malformed.path(), id, &AtomicBool::new(false))
            .expect_err("malformed YAML must fail");
        assert_eq!(invalid, MemoryFileImportError::InvalidRecord);
        assert!(!format!("{invalid:?}").contains("private"));
        assert!(!invalid.to_string().contains("private"));

        let cancelled = AtomicBool::new(true);
        assert_eq!(
            load(malformed.path(), id, &cancelled).expect_err("cancellation must fail"),
            MemoryFileImportError::Cancelled
        );
        let files = MemoryRecordFiles::open(malformed.path()).expect("root must open");
        assert_eq!(
            files
                .load(id, &AtomicBool::new(false), Instant::now())
                .expect_err("elapsed deadline must fail"),
            MemoryFileImportError::DeadlineExceeded
        );
    }
}
