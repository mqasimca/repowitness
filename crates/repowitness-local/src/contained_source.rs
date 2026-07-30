use std::error::Error;
#[cfg(unix)]
use std::ffi::OsString;
use std::fmt;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, DirEntry, File, Metadata, OpenOptions};
use repowitness_domain::RepositoryPath;
use same_file::Handle;

mod exact_session;
pub(crate) use exact_session::{ExactReadSession, ExactReadSessionError};

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
/// Maximum entries inspected while proving one path component's exact spelling.
pub const MAX_EXACT_DIRECTORY_ENTRIES: u64 = 65_536;

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

pub(crate) fn file_has_single_link(file: &std::fs::File) -> io::Result<bool> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;

        file.metadata().map(|metadata| metadata.nlink() == 1)
    }
    #[cfg(windows)]
    {
        use cap_fs_ext::MetadataExt as _;

        let file = File::from_std(file.try_clone()?);
        file.metadata().map(|metadata| metadata.nlink() == 1)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = file;
        Ok(false)
    }
}

impl ContainedSourceRoot {
    /// Opens the explicitly authorized repository root as a directory capability.
    pub fn open(root: &Path) -> Result<Self, ContainedSourceError> {
        let root = Dir::open_ambient_dir(root, cap_std::ambient_authority())
            .map_err(|source| ContainedSourceError::RootOpen { source })?;
        Ok(Self { root })
    }

    /// Opens and reads an exact-spelling regular source file without following symlinks.
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
        let mut file =
            self.open_exact_regular_file(path, limits, deadline, false, &mut is_cancelled)?;
        read_regular_file(&mut file, limits, deadline, &mut is_cancelled)
    }

    pub(crate) fn exact_read_session<'root, 'path>(
        &'root self,
        paths: impl IntoIterator<Item = &'path RepositoryPath>,
        deadline: Instant,
        mut is_cancelled: impl FnMut() -> bool,
    ) -> Result<ExactReadSession<'root>, ExactReadSessionError> {
        ExactReadSession::new(self, paths, deadline, &mut is_cancelled)
    }

    /// Reads one exact-spelling regular file that has exactly one filesystem link.
    ///
    /// This stricter boundary is intended for repository-authored control data.
    /// It fails closed on case aliases, directory-enumeration overflow, hard
    /// links, symlinks, and platforms without a reliable link count.
    pub fn read_unique_exact_with_cancel(
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
        let mut file =
            self.open_exact_regular_file(path, limits, deadline, true, &mut is_cancelled)?;
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

    fn open_exact_regular_file(
        &self,
        path: &RepositoryPath,
        limits: SourceReadLimits,
        deadline: Instant,
        require_unique_link: bool,
        is_cancelled: &mut impl FnMut() -> bool,
    ) -> Result<File, ContainedSourceError> {
        check_control(limits, deadline, is_cancelled)?;
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
            check_control(limits, deadline, is_cancelled)?;
            let component = repository_component(component)?;
            let entry = exact_entry(
                &directory,
                &component,
                ordinal,
                limits,
                deadline,
                is_cancelled,
            )?;
            if components.peek().is_some() {
                directory = directory
                    .open_dir_nofollow(&component)
                    .map_err(|source| ContainedSourceError::DirectoryOpen { ordinal, source })?;
                continue;
            }

            let mut options = OpenOptions::new();
            options.read(true).follow(FollowSymlinks::No);
            set_nonblocking_if_supported(&mut options);
            let file = entry
                .open_with(&options)
                .map_err(|source| ContainedSourceError::FileOpen { source })?;
            let metadata = file
                .metadata()
                .map_err(|source| ContainedSourceError::MetadataRead { source })?;
            if !metadata.is_file() {
                return Err(ContainedSourceError::NotRegularFile);
            }
            if require_unique_link && !has_one_link(&metadata) {
                return Err(ContainedSourceError::LinkCountNotUnique);
            }
            if metadata.len() > limits.file_bytes() {
                return Err(ContainedSourceError::FileByteLimitExceeded {
                    limit: limits.file_bytes(),
                });
            }
            check_control(limits, deadline, is_cancelled)?;
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
    /// An exact path component was absent or had alternate spelling.
    ExactComponentUnavailable {
        /// One-based component ordinal.
        ordinal: u32,
    },
    /// Exact-name proof exceeded its bounded directory enumeration.
    DirectoryEntryLimitExceeded {
        /// Inclusive number of entries inspected.
        limit: u64,
    },
    /// A directory entry could not be inspected.
    DirectoryEntryRead {
        /// One-based component ordinal.
        ordinal: u32,
        /// Safe operating-system diagnostic.
        source: io::Error,
    },
    /// The regular file had multiple links or no reliable link count.
    LinkCountNotUnique,
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
            Self::ExactComponentUnavailable { ordinal } => {
                write!(
                    formatter,
                    "repository path component {ordinal} is unavailable with exact spelling"
                )
            }
            Self::DirectoryEntryLimitExceeded { limit } => {
                write!(
                    formatter,
                    "repository directory exceeds the exact-name scan limit of {limit}"
                )
            }
            Self::DirectoryEntryRead { ordinal, .. } => {
                write!(
                    formatter,
                    "repository directory entry for component {ordinal} could not be inspected"
                )
            }
            Self::LinkCountNotUnique => {
                formatter.write_str("repository source does not have one unique filesystem link")
            }
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
            | Self::DirectoryEntryRead { source, .. }
            | Self::FileRead { source } => Some(source),
            Self::UnsupportedPathEncoding
            | Self::ComponentCountOverflowed
            | Self::NotRegularFile
            | Self::ExactComponentUnavailable { .. }
            | Self::DirectoryEntryLimitExceeded { .. }
            | Self::LinkCountNotUnique
            | Self::FileByteLimitExceeded { .. }
            | Self::DeadlineNotRepresentable
            | Self::Cancelled
            | Self::DeadlineExceeded { .. }
            | Self::RepositoryPathHadNoComponents => None,
        }
    }
}

include!("contained_source/io.rs");

#[cfg(test)]
mod tests;
