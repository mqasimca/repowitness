use std::error::Error;
use std::ffi::OsString;
use std::fmt;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, File, OpenOptions};
use repowitness_domain::RepositoryPath;
use same_file::Handle;

/// Default maximum size of one source file.
pub const DEFAULT_SOURCE_FILE_BYTES: u64 = 8 * 1024 * 1024;
/// Default maximum size of one blocking read.
pub const DEFAULT_SOURCE_READ_CHUNK_BYTES: u64 = 64 * 1024;
/// Default wall-clock deadline for one source-file read.
pub const DEFAULT_SOURCE_READ_DEADLINE: Duration = Duration::from_secs(5);
/// Hard ceiling for a configured source-file limit.
pub const MAX_SOURCE_FILE_BYTES: u64 = 256 * 1024 * 1024;
/// Hard ceiling for a configured blocking-read chunk.
pub const MAX_SOURCE_READ_CHUNK_BYTES: u64 = 1024 * 1024;

/// Bounded controls for opening and reading one repository source file.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceReadLimits {
    deadline: Duration,
    file_bytes: u64,
    read_chunk_bytes: u64,
}

impl SourceReadLimits {
    /// Constructs validated source-read limits.
    pub fn try_new(
        deadline: Duration,
        file_bytes: u64,
        read_chunk_bytes: u64,
    ) -> Result<Self, SourceReadLimitError> {
        if file_bytes > MAX_SOURCE_FILE_BYTES {
            return Err(SourceReadLimitError::FileBytesTooLarge {
                maximum: MAX_SOURCE_FILE_BYTES,
            });
        }
        if read_chunk_bytes == 0 {
            return Err(SourceReadLimitError::ReadChunkIsZero);
        }
        if read_chunk_bytes > MAX_SOURCE_READ_CHUNK_BYTES {
            return Err(SourceReadLimitError::ReadChunkTooLarge {
                maximum: MAX_SOURCE_READ_CHUNK_BYTES,
            });
        }
        Ok(Self {
            deadline,
            file_bytes,
            read_chunk_bytes,
        })
    }

    /// Returns the wall-clock deadline for one source-file operation.
    #[must_use]
    pub const fn deadline(self) -> Duration {
        self.deadline
    }

    /// Returns the inclusive source-file byte limit.
    #[must_use]
    pub const fn file_bytes(self) -> u64 {
        self.file_bytes
    }

    /// Returns the maximum size of one blocking read.
    #[must_use]
    pub const fn read_chunk_bytes(self) -> u64 {
        self.read_chunk_bytes
    }
}

impl Default for SourceReadLimits {
    fn default() -> Self {
        Self {
            deadline: DEFAULT_SOURCE_READ_DEADLINE,
            file_bytes: DEFAULT_SOURCE_FILE_BYTES,
            read_chunk_bytes: DEFAULT_SOURCE_READ_CHUNK_BYTES,
        }
    }
}

/// Invalid source-read limit configuration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceReadLimitError {
    /// The file byte limit exceeded the implementation ceiling.
    FileBytesTooLarge {
        /// Largest supported value.
        maximum: u64,
    },
    /// A zero-sized read chunk would prevent progress.
    ReadChunkIsZero,
    /// The read chunk exceeded the implementation ceiling.
    ReadChunkTooLarge {
        /// Largest supported value.
        maximum: u64,
    },
}

impl fmt::Display for SourceReadLimitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FileBytesTooLarge { maximum } => {
                write!(
                    formatter,
                    "source file byte limit exceeds the supported maximum of {maximum}"
                )
            }
            Self::ReadChunkIsZero => formatter.write_str("source read chunk must be nonzero"),
            Self::ReadChunkTooLarge { maximum } => {
                write!(
                    formatter,
                    "source read chunk exceeds the supported maximum of {maximum}"
                )
            }
        }
    }
}

impl Error for SourceReadLimitError {}

/// A directory capability anchoring all subsequent source-file resolution.
pub struct ContainedSourceRoot {
    root: Dir,
}

#[derive(PartialEq, Eq)]
pub(crate) struct FileIdentity(Handle);

impl FileIdentity {
    pub(crate) fn from_path(path: &Path) -> io::Result<Self> {
        Handle::from_path(path).map(Self)
    }

    pub(crate) fn from_file(file: std::fs::File) -> io::Result<Self> {
        Handle::from_file(file).map(Self)
    }
}

impl ContainedSourceRoot {
    /// Opens the explicitly authorized repository root as a directory capability.
    pub fn open(root: &Path) -> Result<Self, ContainedSourceError> {
        let root = Dir::open_ambient_dir(root, cap_std::ambient_authority())
            .map_err(|source| ContainedSourceError::RootOpen { source })?;
        Ok(Self { root })
    }

    /// Opens and reads a regular source file without following symlinks.
    pub fn read(
        &self,
        path: &RepositoryPath,
        limits: SourceReadLimits,
    ) -> Result<Box<[u8]>, ContainedSourceError> {
        self.read_with_cancel(path, limits, || false)
    }

    /// Opens and reads a regular source file with cooperative cancellation.
    pub fn read_with_cancel(
        &self,
        path: &RepositoryPath,
        limits: SourceReadLimits,
        mut is_cancelled: impl FnMut() -> bool,
    ) -> Result<Box<[u8]>, ContainedSourceError> {
        if is_cancelled() {
            return Err(ContainedSourceError::Cancelled);
        }
        if limits.deadline().is_zero() {
            return Err(ContainedSourceError::DeadlineExceeded {
                deadline: limits.deadline(),
            });
        }
        let deadline = Instant::now()
            .checked_add(limits.deadline())
            .ok_or(ContainedSourceError::DeadlineNotRepresentable)?;
        let mut file = self.open_regular_file(path, limits, deadline, &mut is_cancelled)?;
        read_regular_file(&mut file, limits, deadline, &mut is_cancelled)
    }

    pub(crate) fn aliases_identity(
        &self,
        path: &RepositoryPath,
        identity: &FileIdentity,
        deadline_duration: Duration,
        deadline: Instant,
        is_cancelled: &mut impl FnMut() -> bool,
    ) -> Result<bool, ContainedSourceError> {
        let file =
            self.open_regular_file_unbounded(path, deadline_duration, deadline, is_cancelled)?;
        let candidate = Handle::from_file(file.into_std())
            .map_err(|source| ContainedSourceError::MetadataRead { source })?;
        Ok(candidate == identity.0)
    }

    fn open_regular_file(
        &self,
        path: &RepositoryPath,
        limits: SourceReadLimits,
        deadline: Instant,
        is_cancelled: &mut impl FnMut() -> bool,
    ) -> Result<File, ContainedSourceError> {
        let file =
            self.open_regular_file_unbounded(path, limits.deadline(), deadline, is_cancelled)?;
        let metadata = file
            .metadata()
            .map_err(|source| ContainedSourceError::MetadataRead { source })?;
        if metadata.len() > limits.file_bytes() {
            return Err(ContainedSourceError::FileByteLimitExceeded {
                limit: limits.file_bytes(),
            });
        }
        check_control(limits, deadline, is_cancelled)?;
        Ok(file)
    }

    fn open_regular_file_unbounded(
        &self,
        path: &RepositoryPath,
        deadline_duration: Duration,
        deadline: Instant,
        is_cancelled: &mut impl FnMut() -> bool,
    ) -> Result<File, ContainedSourceError> {
        check_control_duration(deadline_duration, deadline, is_cancelled)?;
        let mut directory = self
            .root
            .try_clone()
            .map_err(|source| ContainedSourceError::RootClone { source })?;
        let mut components = path.components().peekable();
        let mut ordinal = 0_u32;

        while let Some(component) = components.next() {
            ordinal = ordinal
                .checked_add(1)
                .ok_or(ContainedSourceError::ComponentCountOverflowed)?;
            check_control_duration(deadline_duration, deadline, is_cancelled)?;
            let component = repository_component(component)?;
            if components.peek().is_some() {
                directory = directory
                    .open_dir_nofollow(&component)
                    .map_err(|source| ContainedSourceError::DirectoryOpen { ordinal, source })?;
                continue;
            }

            let mut options = OpenOptions::new();
            options.read(true).follow(FollowSymlinks::No);
            set_nonblocking_if_supported(&mut options);
            let file = directory
                .open_with(&component, &options)
                .map_err(|source| ContainedSourceError::FileOpen { source })?;
            let metadata = file
                .metadata()
                .map_err(|source| ContainedSourceError::MetadataRead { source })?;
            if !metadata.is_file() {
                return Err(ContainedSourceError::NotRegularFile);
            }
            check_control_duration(deadline_duration, deadline, is_cancelled)?;
            return Ok(file);
        }

        Err(ContainedSourceError::RepositoryPathHadNoComponents)
    }
}

impl fmt::Debug for ContainedSourceRoot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ContainedSourceRoot")
            .field("root", &"<directory-capability>")
            .finish()
    }
}

/// Failure to resolve or read a repository source through its directory capability.
#[derive(Debug)]
pub enum ContainedSourceError {
    /// The explicitly authorized repository root could not be opened.
    RootOpen {
        /// Safe operating-system diagnostic.
        source: io::Error,
    },
    /// The root capability could not be cloned for an independent read.
    RootClone {
        /// Safe operating-system diagnostic.
        source: io::Error,
    },
    /// A repository path component cannot be represented on this host.
    UnsupportedPathEncoding,
    /// A path component count overflowed its stable diagnostic type.
    ComponentCountOverflowed,
    /// An intermediate directory could not be opened without following links.
    DirectoryOpen {
        /// One-based component ordinal.
        ordinal: u32,
        /// Safe operating-system diagnostic.
        source: io::Error,
    },
    /// The final component could not be opened without following links.
    FileOpen {
        /// Safe operating-system diagnostic.
        source: io::Error,
    },
    /// Handle-based metadata inspection failed.
    MetadataRead {
        /// Safe operating-system diagnostic.
        source: io::Error,
    },
    /// The opened handle was not a regular file.
    NotRegularFile,
    /// The file exceeded the inclusive byte limit.
    FileByteLimitExceeded {
        /// Configured inclusive limit.
        limit: u64,
    },
    /// A file read failed.
    FileRead {
        /// Safe operating-system diagnostic.
        source: io::Error,
    },
    /// The configured deadline cannot be represented by the monotonic clock.
    DeadlineNotRepresentable,
    /// The operation was cancelled before producing output.
    Cancelled,
    /// The operation exceeded its wall-clock deadline.
    DeadlineExceeded {
        /// Configured duration, without an absolute timestamp.
        deadline: Duration,
    },
    /// A validated repository path unexpectedly had no components.
    RepositoryPathHadNoComponents,
}

impl fmt::Display for ContainedSourceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RootOpen { .. } => {
                formatter.write_str("repository root capability could not be opened")
            }
            Self::RootClone { .. } => {
                formatter.write_str("repository root capability could not be cloned")
            }
            Self::UnsupportedPathEncoding => {
                formatter.write_str("repository path cannot be represented on this host")
            }
            Self::ComponentCountOverflowed => {
                formatter.write_str("repository path component count overflowed")
            }
            Self::DirectoryOpen { ordinal, .. } => {
                write!(
                    formatter,
                    "repository directory component {ordinal} could not be opened safely"
                )
            }
            Self::FileOpen { .. } => {
                formatter.write_str("repository source file could not be opened safely")
            }
            Self::MetadataRead { .. } => {
                formatter.write_str("repository source metadata could not be read")
            }
            Self::NotRegularFile => formatter.write_str("repository source is not a regular file"),
            Self::FileByteLimitExceeded { limit } => {
                write!(
                    formatter,
                    "repository source exceeds the byte limit of {limit}"
                )
            }
            Self::FileRead { .. } => formatter.write_str("repository source could not be read"),
            Self::DeadlineNotRepresentable => {
                formatter.write_str("source read deadline is not representable")
            }
            Self::Cancelled => formatter.write_str("source read was cancelled"),
            Self::DeadlineExceeded { deadline } => {
                write!(
                    formatter,
                    "source read exceeded its deadline of {deadline:?}"
                )
            }
            Self::RepositoryPathHadNoComponents => {
                formatter.write_str("validated repository path had no components")
            }
        }
    }
}

impl Error for ContainedSourceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::RootOpen { source }
            | Self::RootClone { source }
            | Self::DirectoryOpen { source, .. }
            | Self::FileOpen { source }
            | Self::MetadataRead { source }
            | Self::FileRead { source } => Some(source),
            Self::UnsupportedPathEncoding
            | Self::ComponentCountOverflowed
            | Self::NotRegularFile
            | Self::FileByteLimitExceeded { .. }
            | Self::DeadlineNotRepresentable
            | Self::Cancelled
            | Self::DeadlineExceeded { .. }
            | Self::RepositoryPathHadNoComponents => None,
        }
    }
}

fn read_regular_file(
    file: &mut File,
    limits: SourceReadLimits,
    deadline: Instant,
    is_cancelled: &mut impl FnMut() -> bool,
) -> Result<Box<[u8]>, ContainedSourceError> {
    let chunk_bytes = usize::try_from(limits.read_chunk_bytes()).map_err(|_| {
        ContainedSourceError::FileByteLimitExceeded {
            limit: limits.file_bytes(),
        }
    })?;
    let capacity =
        usize::try_from(limits.file_bytes().min(limits.read_chunk_bytes())).map_err(|_| {
            ContainedSourceError::FileByteLimitExceeded {
                limit: limits.file_bytes(),
            }
        })?;
    let mut output = Vec::with_capacity(capacity);
    let mut buffer = vec![0_u8; chunk_bytes];

    loop {
        check_control(limits, deadline, is_cancelled)?;
        let output_bytes = u64::try_from(output.len()).map_err(|_| {
            ContainedSourceError::FileByteLimitExceeded {
                limit: limits.file_bytes(),
            }
        })?;
        let remaining = limits.file_bytes().checked_sub(output_bytes).ok_or(
            ContainedSourceError::FileByteLimitExceeded {
                limit: limits.file_bytes(),
            },
        )?;
        let requested = remaining
            .checked_add(1)
            .unwrap_or(remaining)
            .min(limits.read_chunk_bytes());
        let requested = usize::try_from(requested).map_err(|_| {
            ContainedSourceError::FileByteLimitExceeded {
                limit: limits.file_bytes(),
            }
        })?;
        let read = file
            .read(&mut buffer[..requested])
            .map_err(|source| ContainedSourceError::FileRead { source })?;
        if read == 0 {
            return Ok(output.into_boxed_slice());
        }
        let read =
            u64::try_from(read).map_err(|_| ContainedSourceError::FileByteLimitExceeded {
                limit: limits.file_bytes(),
            })?;
        if read > remaining {
            return Err(ContainedSourceError::FileByteLimitExceeded {
                limit: limits.file_bytes(),
            });
        }
        let read =
            usize::try_from(read).map_err(|_| ContainedSourceError::FileByteLimitExceeded {
                limit: limits.file_bytes(),
            })?;
        output.extend_from_slice(&buffer[..read]);
    }
}

fn check_control(
    limits: SourceReadLimits,
    deadline: Instant,
    is_cancelled: &mut impl FnMut() -> bool,
) -> Result<(), ContainedSourceError> {
    check_control_duration(limits.deadline(), deadline, is_cancelled)
}

fn check_control_duration(
    deadline_duration: Duration,
    deadline: Instant,
    is_cancelled: &mut impl FnMut() -> bool,
) -> Result<(), ContainedSourceError> {
    if is_cancelled() {
        return Err(ContainedSourceError::Cancelled);
    }
    if Instant::now() >= deadline {
        return Err(ContainedSourceError::DeadlineExceeded {
            deadline: deadline_duration,
        });
    }
    Ok(())
}

#[cfg(unix)]
fn repository_component(component: &[u8]) -> Result<PathBuf, ContainedSourceError> {
    use std::os::unix::ffi::OsStringExt;

    Ok(PathBuf::from(OsString::from_vec(component.to_vec())))
}

#[cfg(not(unix))]
fn repository_component(component: &[u8]) -> Result<PathBuf, ContainedSourceError> {
    let component = std::str::from_utf8(component)
        .map_err(|_| ContainedSourceError::UnsupportedPathEncoding)?;
    Ok(PathBuf::from(component))
}

#[cfg(unix)]
fn set_nonblocking_if_supported(options: &mut OpenOptions) {
    use cap_std::fs::OpenOptionsExt;

    let nonblocking = i32::try_from(rustix::fs::OFlags::NONBLOCK.bits())
        .expect("the platform O_NONBLOCK flag must fit cap-std's Unix flag type");
    options.custom_flags(nonblocking);
}

#[cfg(not(unix))]
fn set_nonblocking_if_supported(_options: &mut OpenOptions) {}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    use repowitness_domain::RepositoryPathLimits;

    use super::*;

    static NEXT_FIXTURE_ID: AtomicU64 = AtomicU64::new(0);
    const PATH_LIMITS: RepositoryPathLimits = RepositoryPathLimits::new(4096, 256);

    struct TempDirectory {
        root: PathBuf,
    }

    impl TempDirectory {
        fn new() -> Self {
            let fixture_id = NEXT_FIXTURE_ID.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "repowitness-contained-source-{}-{fixture_id}",
                std::process::id()
            ));
            fs::create_dir(&root).expect("fixture directory must be created");
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

    fn path(bytes: &[u8]) -> RepositoryPath {
        RepositoryPath::try_from_bytes(bytes, PATH_LIMITS)
            .expect("fixture repository path must be valid")
    }

    fn limits(file_bytes: u64, chunk_bytes: u64) -> SourceReadLimits {
        SourceReadLimits::try_new(Duration::from_secs(1), file_bytes, chunk_bytes)
            .expect("fixture limits must be valid")
    }

    #[test]
    fn regular_files_are_read_exactly_with_inclusive_limits() {
        let fixture = TempDirectory::new();
        fs::create_dir(fixture.path().join("src")).expect("source directory must be created");
        fs::write(fixture.path().join("src/lib.rs"), b"fn exact() {}\n")
            .expect("source fixture must be written");
        let root = ContainedSourceRoot::open(fixture.path()).expect("root must open");
        let source = path(b"src/lib.rs");

        assert_eq!(
            root.read(&source, limits(14, 3))
                .expect("exact limit must be inclusive")
                .as_ref(),
            b"fn exact() {}\n"
        );
        assert!(matches!(
            root.read(&source, limits(13, 3)),
            Err(ContainedSourceError::FileByteLimitExceeded { limit: 13 })
        ));
        assert_eq!(
            root.read(&path(b"empty"), limits(0, 1))
                .expect_err("missing empty fixture must fail")
                .to_string(),
            "repository source file could not be opened safely"
        );
        fs::write(fixture.path().join("empty"), b"").expect("empty fixture must be written");
        assert!(
            root.read(&path(b"empty"), limits(0, 1))
                .expect("zero limit accepts an empty file")
                .is_empty()
        );
    }

    #[test]
    fn cancellation_deadline_and_limit_configuration_are_explicit() {
        let fixture = TempDirectory::new();
        fs::write(fixture.path().join("source.rs"), b"fn source() {}\n")
            .expect("source fixture must be written");
        let root = ContainedSourceRoot::open(fixture.path()).expect("root must open");
        let source = path(b"source.rs");

        assert!(matches!(
            root.read_with_cancel(&source, SourceReadLimits::default(), || true),
            Err(ContainedSourceError::Cancelled)
        ));
        let zero_deadline = SourceReadLimits::try_new(Duration::ZERO, 1024, 64)
            .expect("zero deadline remains an operation outcome");
        assert!(matches!(
            root.read(&source, zero_deadline),
            Err(ContainedSourceError::DeadlineExceeded { deadline })
                if deadline == Duration::ZERO
        ));
        assert_eq!(
            SourceReadLimits::try_new(Duration::from_secs(1), MAX_SOURCE_FILE_BYTES + 1, 1),
            Err(SourceReadLimitError::FileBytesTooLarge {
                maximum: MAX_SOURCE_FILE_BYTES
            })
        );
        assert_eq!(
            SourceReadLimits::try_new(Duration::from_secs(1), 1, 0),
            Err(SourceReadLimitError::ReadChunkIsZero)
        );
        assert_eq!(
            SourceReadLimits::try_new(Duration::from_secs(1), 1, MAX_SOURCE_READ_CHUNK_BYTES + 1),
            Err(SourceReadLimitError::ReadChunkTooLarge {
                maximum: MAX_SOURCE_READ_CHUNK_BYTES
            })
        );
    }

    #[cfg(unix)]
    #[test]
    fn final_and_intermediate_symlinks_cannot_escape_the_root() {
        use std::os::unix::fs::symlink;

        let fixture = TempDirectory::new();
        let outside = TempDirectory::new();
        fs::write(outside.path().join("private.rs"), b"private")
            .expect("outside fixture must be written");
        symlink(
            outside.path().join("private.rs"),
            fixture.path().join("final.rs"),
        )
        .expect("final symlink must be created");
        symlink(outside.path(), fixture.path().join("linked"))
            .expect("directory symlink must be created");
        let root = ContainedSourceRoot::open(fixture.path()).expect("root must open");

        let final_error = root
            .read(&path(b"final.rs"), SourceReadLimits::default())
            .expect_err("final symlink must not be followed");
        let intermediate_error = root
            .read(&path(b"linked/private.rs"), SourceReadLimits::default())
            .expect_err("intermediate symlink must not be followed");
        assert!(matches!(final_error, ContainedSourceError::FileOpen { .. }));
        assert!(matches!(
            intermediate_error,
            ContainedSourceError::DirectoryOpen { ordinal: 1, .. }
        ));
        assert!(!final_error.to_string().contains("private"));
        assert!(!intermediate_error.to_string().contains("private"));
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_paths_are_opened_without_lossy_conversion() {
        use std::os::unix::ffi::OsStringExt;

        let fixture = TempDirectory::new();
        let name = b"non-utf8-\xFF.rs".to_vec();
        fs::write(
            fixture.path().join(OsString::from_vec(name.clone())),
            b"bytes",
        )
        .expect("non-UTF-8 fixture must be written");
        let root = ContainedSourceRoot::open(fixture.path()).expect("root must open");

        assert_eq!(
            root.read(&path(&name), SourceReadLimits::default())
                .expect("non-UTF-8 path must be lossless")
                .as_ref(),
            b"bytes"
        );
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn hard_link_aliases_are_identified_through_contained_handles() {
        let fixture = TempDirectory::new();
        let outside = TempDirectory::new();
        let database = outside.path().join("index.sqlite3");
        fs::write(&database, b"database").expect("database fixture must be written");
        fs::hard_link(&database, fixture.path().join("alias"))
            .expect("hard-link fixture must be created");
        fs::write(fixture.path().join("other"), b"other")
            .expect("independent fixture must be written");
        let identity =
            FileIdentity::from_path(&database).expect("database identity must be readable");
        let root = ContainedSourceRoot::open(fixture.path()).expect("root must open");
        let deadline_duration = Duration::from_secs(1);
        let deadline = Instant::now() + deadline_duration;

        assert!(
            root.aliases_identity(
                &path(b"alias"),
                &identity,
                deadline_duration,
                deadline,
                &mut || false,
            )
            .expect("hard-link identity check must complete")
        );
        assert!(
            !root
                .aliases_identity(
                    &path(b"other"),
                    &identity,
                    deadline_duration,
                    deadline,
                    &mut || false,
                )
                .expect("independent identity check must complete")
        );
    }

    #[cfg(unix)]
    #[test]
    fn special_files_are_rejected_without_blocking() {
        use std::os::unix::net::UnixListener;

        let fixture = TempDirectory::new();
        let _listener = UnixListener::bind(fixture.path().join("socket"))
            .expect("socket fixture must be created");
        let root = ContainedSourceRoot::open(fixture.path()).expect("root must open");
        let started = Instant::now();

        let error = root
            .read(&path(b"socket"), SourceReadLimits::default())
            .expect_err("socket must not be read as source");
        assert!(
            matches!(
                error,
                ContainedSourceError::FileOpen { .. } | ContainedSourceError::NotRegularFile
            ),
            "unexpected special-file error: {error}"
        );
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "special-file rejection must not block"
        );
    }

    #[test]
    fn an_open_handle_pins_content_across_path_replacement() {
        let fixture = TempDirectory::new();
        let source_path = fixture.path().join("source.rs");
        let replaced_path = fixture.path().join("replaced.rs");
        fs::write(&source_path, b"old").expect("old source must be written");
        let root = ContainedSourceRoot::open(fixture.path()).expect("root must open");
        let read_limits = limits(3, 1);
        let deadline = Instant::now() + read_limits.deadline();
        let mut file = root
            .open_regular_file(&path(b"source.rs"), read_limits, deadline, &mut || false)
            .expect("old source handle must open");

        fs::rename(&source_path, &replaced_path).expect("old source must be renamed");
        fs::write(&source_path, b"new").expect("new source must replace the path");

        assert_eq!(
            read_regular_file(&mut file, read_limits, deadline, &mut || false)
                .expect("opened handle must remain readable")
                .as_ref(),
            b"old"
        );
        assert_eq!(
            root.read(&path(b"source.rs"), read_limits)
                .expect("subsequent read must see replacement")
                .as_ref(),
            b"new"
        );
    }

    #[test]
    fn diagnostics_and_debug_output_do_not_expose_paths() {
        let fixture = TempDirectory::new();
        let root = ContainedSourceRoot::open(fixture.path()).expect("root must open");
        let error = root
            .read(&path(b"private-name.rs"), SourceReadLimits::default())
            .expect_err("missing source must fail");

        assert!(!error.to_string().contains("private-name"));
        assert!(!format!("{error:?}").contains("private-name"));
        assert!(!format!("{root:?}").contains(&fixture.path().to_string_lossy().to_string()));
        assert!(error.source().is_some());
    }
}
