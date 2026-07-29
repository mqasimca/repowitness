use std::{
    ffi::OsString,
    fmt,
    io::Read,
    path::{Component, Path, PathBuf},
};

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::{
    ambient_authority,
    fs::{Dir, File, Metadata, OpenOptions},
};
use same_file::Handle;
use sha2::{Digest, Sha256};

/// Hard byte ceiling for repository-authored control files.
pub const MAX_BOUNDED_CONTROL_FILE_BYTES: usize = 16 * 1024 * 1024;
/// Hard component ceiling for one explicitly supplied control-file path.
pub const MAX_BOUNDED_CONTROL_FILE_COMPONENTS: usize = 256;
/// Hard encoded-byte ceiling for one explicitly supplied control-file path.
pub const MAX_BOUNDED_CONTROL_FILE_PATH_BYTES: usize = 32 * 1024;

const READ_CHUNK_BYTES: usize = 64 * 1024;

/// Immutable result of one bounded, no-follow control-file admission.
#[derive(Clone, Eq, PartialEq)]
pub struct BoundedFileContents {
    bytes: Box<[u8]>,
    sha256: [u8; 32],
}

/// Opaque exact-file and ancestor authority retained after admission.
pub struct AdmittedFileParent {
    resolved_file: ResolvedPath,
    resolved_parent: ResolvedParent,
    _directory: Dir,
    ancestor_identities: Box<[Handle]>,
    file_identity: Handle,
    file_metadata: Metadata,
    file_sha256: [u8; 32],
    maximum_bytes: usize,
}

impl fmt::Debug for AdmittedFileParent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AdmittedFileParent")
            .field("path", &"<redacted-path>")
            .field("ancestor_count", &self.ancestor_identities.len())
            .finish()
    }
}

impl AdmittedFileParent {
    pub(crate) fn lexical_path(&self) -> &Path {
        &self.resolved_parent.lexical_path
    }

    pub(crate) fn matches_contents(&self, bytes: &[u8]) -> bool {
        if bytes.len() > self.maximum_bytes {
            return false;
        }
        let digest: [u8; 32] = Sha256::digest(bytes).into();
        digest == self.file_sha256
    }

    pub(crate) fn revalidate(&self) -> Result<(), BoundedFileReadError> {
        let mut current =
            open_nofollow(&self.resolved_file).map_err(|_| BoundedFileReadError::Changed)?;
        validate_metadata(&current.metadata, self.maximum_bytes)
            .map_err(|_| BoundedFileReadError::Changed)?;
        if self.file_identity != current.identity
            || !same_identity_chain(&self.ancestor_identities, &current.parent.identities)
            || !same_metadata(&self.file_metadata, &current.metadata)
        {
            return Err(BoundedFileReadError::Changed);
        }
        let contents = read_and_digest(&mut current.file, self.maximum_bytes)
            .map_err(|_| BoundedFileReadError::Changed)?;
        let after_read = current
            .file
            .metadata()
            .map_err(|_| BoundedFileReadError::Changed)?;
        if contents.sha256 != self.file_sha256
            || !same_metadata(&current.metadata, &after_read)
            || !same_metadata(&self.file_metadata, &after_read)
        {
            return Err(BoundedFileReadError::Changed);
        }
        let final_open =
            open_nofollow(&self.resolved_file).map_err(|_| BoundedFileReadError::Changed)?;
        if self.file_identity != final_open.identity
            || !same_identity_chain(&self.ancestor_identities, &final_open.parent.identities)
            || !same_metadata(&after_read, &final_open.metadata)
        {
            return Err(BoundedFileReadError::Changed);
        }
        Ok(())
    }
}

impl BoundedFileContents {
    /// Returns the admitted bytes.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Consumes the result and returns the admitted bytes.
    #[must_use]
    pub fn into_bytes(self) -> Box<[u8]> {
        self.bytes
    }

    /// Returns the digest of the admitted bytes.
    #[must_use]
    pub const fn sha256(&self) -> &[u8; 32] {
        &self.sha256
    }
}

impl fmt::Debug for BoundedFileContents {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BoundedFileContents")
            .field("byte_count", &self.bytes.len())
            .field("sha256_bytes", &self.sha256.len())
            .finish()
    }
}

/// Path-free failure from bounded control-file admission.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BoundedFileReadError {
    /// The supplied path or byte limit is outside the bounded contract.
    InvalidRequest,
    /// A path component or the final target was unavailable or unsafe.
    Unavailable,
    /// The final regular file exceeded the inclusive byte limit.
    TooLarge,
    /// The file or one of its ancestors changed during admission.
    Changed,
}

impl fmt::Display for BoundedFileReadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidRequest => "bounded file request is invalid",
            Self::Unavailable => "bounded file is unavailable",
            Self::TooLarge => "bounded file exceeds its byte limit",
            Self::Changed => "bounded file changed during admission",
        })
    }
}

impl std::error::Error for BoundedFileReadError {}

/// Reads one uniquely linked regular file through a no-follow component walk.
///
/// Relative paths are anchored to the current working directory. `.` and `..`
/// are normalized lexically before capabilities are opened. Every ancestor and
/// the final component is opened without following symlinks or reparse points.
/// A fresh second walk, stable file identity, metadata, and content digest
/// fence the result against replacement and in-place mutation.
pub fn read_bounded_regular_file(
    path: &Path,
    maximum_bytes: usize,
) -> Result<BoundedFileContents, BoundedFileReadError> {
    read_bounded_regular_file_with_parent(path, maximum_bytes).map(|(contents, _parent)| contents)
}

/// Reads one bounded regular file and retains its exact file/ancestor authority.
pub fn read_bounded_regular_file_with_parent(
    path: &Path,
    maximum_bytes: usize,
) -> Result<(BoundedFileContents, AdmittedFileParent), BoundedFileReadError> {
    read_bounded_regular_file_with_hook(path, maximum_bytes, || {})
}

fn read_bounded_regular_file_with_hook(
    path: &Path,
    maximum_bytes: usize,
    after_open: impl FnOnce(),
) -> Result<(BoundedFileContents, AdmittedFileParent), BoundedFileReadError> {
    let resolved = ResolvedPath::try_new(path)?;
    validate_maximum(maximum_bytes)?;

    let mut opened = open_nofollow(&resolved)?;
    validate_metadata(&opened.metadata, maximum_bytes)?;
    after_open();
    let first = read_and_digest(&mut opened.file, maximum_bytes)?;
    let first_final_metadata = opened
        .file
        .metadata()
        .map_err(|_| BoundedFileReadError::Unavailable)?;
    if !same_metadata(&opened.metadata, &first_final_metadata) {
        return Err(BoundedFileReadError::Changed);
    }

    let mut current = open_nofollow(&resolved).map_err(|error| match error {
        BoundedFileReadError::TooLarge => BoundedFileReadError::TooLarge,
        BoundedFileReadError::InvalidRequest => BoundedFileReadError::InvalidRequest,
        BoundedFileReadError::Unavailable | BoundedFileReadError::Changed => {
            BoundedFileReadError::Changed
        }
    })?;
    validate_metadata(&current.metadata, maximum_bytes)?;
    if opened.identity != current.identity
        || !same_identity_chain(&opened.parent.identities, &current.parent.identities)
        || !same_metadata(&first_final_metadata, &current.metadata)
    {
        return Err(BoundedFileReadError::Changed);
    }
    let verification = read_and_digest(&mut current.file, maximum_bytes)?;
    let verification_metadata = current
        .file
        .metadata()
        .map_err(|_| BoundedFileReadError::Changed)?;
    if first.sha256 != verification.sha256
        || first.bytes.len() != verification.bytes.len()
        || !same_metadata(&current.metadata, &verification_metadata)
    {
        return Err(BoundedFileReadError::Changed);
    }
    let final_open = open_nofollow(&resolved).map_err(|_| BoundedFileReadError::Changed)?;
    if current.identity != final_open.identity
        || !same_identity_chain(&current.parent.identities, &final_open.parent.identities)
        || !same_metadata(&verification_metadata, &final_open.metadata)
    {
        return Err(BoundedFileReadError::Changed);
    }
    Ok((
        first,
        AdmittedFileParent {
            resolved_file: resolved,
            resolved_parent: final_open.parent.resolved,
            _directory: final_open.parent.directory,
            ancestor_identities: final_open.parent.identities,
            file_identity: final_open.identity,
            file_metadata: final_open.metadata,
            file_sha256: verification.sha256,
            maximum_bytes,
        },
    ))
}

fn validate_maximum(maximum_bytes: usize) -> Result<(), BoundedFileReadError> {
    if maximum_bytes > MAX_BOUNDED_CONTROL_FILE_BYTES {
        return Err(BoundedFileReadError::InvalidRequest);
    }
    Ok(())
}

#[derive(Clone)]
struct ResolvedPath {
    root: PathBuf,
    components: Box<[OsString]>,
}

impl ResolvedPath {
    fn try_new(path: &Path) -> Result<Self, BoundedFileReadError> {
        if path.as_os_str().is_empty()
            || path.as_os_str().as_encoded_bytes().len() > MAX_BOUNDED_CONTROL_FILE_PATH_BYTES
        {
            return Err(BoundedFileReadError::InvalidRequest);
        }
        let absolute = std::path::absolute(path).map_err(|_| BoundedFileReadError::Unavailable)?;
        if absolute.as_os_str().as_encoded_bytes().len() > MAX_BOUNDED_CONTROL_FILE_PATH_BYTES {
            return Err(BoundedFileReadError::InvalidRequest);
        }
        let mut root = PathBuf::new();
        let mut components = Vec::<OsString>::new();
        let mut rooted = false;
        for component in absolute.components() {
            match component {
                Component::Prefix(prefix) if !rooted && components.is_empty() => {
                    root.push(prefix.as_os_str());
                }
                Component::RootDir if components.is_empty() => {
                    root.push(Path::new(std::path::MAIN_SEPARATOR_STR));
                    rooted = true;
                }
                Component::CurDir => {}
                Component::ParentDir => {
                    if components.pop().is_none() {
                        return Err(BoundedFileReadError::InvalidRequest);
                    }
                }
                Component::Normal(component) => {
                    if component.is_empty()
                        || components.len() >= MAX_BOUNDED_CONTROL_FILE_COMPONENTS
                    {
                        return Err(BoundedFileReadError::InvalidRequest);
                    }
                    components.push(component.to_owned());
                }
                Component::Prefix(_) | Component::RootDir => {
                    return Err(BoundedFileReadError::InvalidRequest);
                }
            }
        }
        if !rooted || components.is_empty() {
            return Err(BoundedFileReadError::InvalidRequest);
        }
        Ok(Self {
            root,
            components: components.into_boxed_slice(),
        })
    }
}

#[derive(Clone)]
struct ResolvedParent {
    root: PathBuf,
    components: Box<[OsString]>,
    lexical_path: PathBuf,
}

impl ResolvedPath {
    fn parent(&self) -> Result<ResolvedParent, BoundedFileReadError> {
        let (_, parents) = self
            .components
            .split_last()
            .ok_or(BoundedFileReadError::InvalidRequest)?;
        let mut lexical_path = self.root.clone();
        for component in parents {
            lexical_path.push(component);
        }
        Ok(ResolvedParent {
            root: self.root.clone(),
            components: parents.to_vec().into_boxed_slice(),
            lexical_path,
        })
    }
}

struct OpenedFile {
    file: File,
    identity: Handle,
    metadata: Metadata,
    parent: OpenedParent,
}

struct OpenedParent {
    resolved: ResolvedParent,
    directory: Dir,
    identities: Box<[Handle]>,
}

fn open_nofollow(resolved: &ResolvedPath) -> Result<OpenedFile, BoundedFileReadError> {
    let (final_component, _) = resolved
        .components
        .split_last()
        .ok_or(BoundedFileReadError::InvalidRequest)?;
    let parent = open_parent(&resolved.parent()?)?;
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    set_nonblocking(&mut options);
    let file = parent
        .directory
        .open_with(final_component, &options)
        .map_err(|_| BoundedFileReadError::Unavailable)?;
    let metadata = file
        .metadata()
        .map_err(|_| BoundedFileReadError::Unavailable)?;
    let identity = Handle::from_file(
        file.try_clone()
            .map_err(|_| BoundedFileReadError::Unavailable)?
            .into_std(),
    )
    .map_err(|_| BoundedFileReadError::Unavailable)?;
    Ok(OpenedFile {
        file,
        identity,
        metadata,
        parent,
    })
}

fn open_parent(resolved: &ResolvedParent) -> Result<OpenedParent, BoundedFileReadError> {
    let mut directory = Dir::open_ambient_dir(&resolved.root, ambient_authority())
        .map_err(|_| BoundedFileReadError::Unavailable)?;
    let mut identities = Vec::with_capacity(resolved.components.len().saturating_add(1));
    identities.push(directory_identity(&directory)?);
    for component in &resolved.components {
        directory = directory
            .open_dir_nofollow(component)
            .map_err(|_| BoundedFileReadError::Unavailable)?;
        identities.push(directory_identity(&directory)?);
    }
    Ok(OpenedParent {
        resolved: resolved.clone(),
        directory,
        identities: identities.into_boxed_slice(),
    })
}

fn directory_identity(directory: &Dir) -> Result<Handle, BoundedFileReadError> {
    Handle::from_file(
        directory
            .try_clone()
            .map_err(|_| BoundedFileReadError::Unavailable)?
            .into_std_file(),
    )
    .map_err(|_| BoundedFileReadError::Unavailable)
}

fn same_identity_chain(expected: &[Handle], current: &[Handle]) -> bool {
    expected.len() == current.len()
        && expected
            .iter()
            .zip(current)
            .all(|(expected, current)| expected == current)
}

fn read_and_digest(
    file: &mut File,
    maximum_bytes: usize,
) -> Result<BoundedFileContents, BoundedFileReadError> {
    let mut bytes = Vec::with_capacity(maximum_bytes.min(READ_CHUNK_BYTES));
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; READ_CHUNK_BYTES];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|_| BoundedFileReadError::Unavailable)?;
        if read == 0 {
            break;
        }
        if bytes
            .len()
            .checked_add(read)
            .is_none_or(|length| length > maximum_bytes)
        {
            return Err(BoundedFileReadError::TooLarge);
        }
        hasher.update(&buffer[..read]);
        bytes.extend_from_slice(&buffer[..read]);
    }
    Ok(BoundedFileContents {
        bytes: bytes.into_boxed_slice(),
        sha256: hasher.finalize().into(),
    })
}

fn validate_metadata(
    metadata: &Metadata,
    maximum_bytes: usize,
) -> Result<(), BoundedFileReadError> {
    if !metadata.is_file() || !has_one_link(metadata) || is_reparse_point(metadata) {
        return Err(BoundedFileReadError::Unavailable);
    }
    let maximum = u64::try_from(maximum_bytes).map_err(|_| BoundedFileReadError::InvalidRequest)?;
    if metadata.len() > maximum {
        return Err(BoundedFileReadError::TooLarge);
    }
    Ok(())
}

#[cfg(unix)]
fn has_one_link(metadata: &Metadata) -> bool {
    use cap_std::fs::MetadataExt as _;

    metadata.nlink() == 1
}

#[cfg(windows)]
fn has_one_link(metadata: &Metadata) -> bool {
    use cap_std::fs::MetadataExt as _;

    metadata.number_of_links() == Some(1)
}

#[cfg(not(any(unix, windows)))]
fn has_one_link(_metadata: &Metadata) -> bool {
    false
}

#[cfg(windows)]
fn is_reparse_point(metadata: &Metadata) -> bool {
    use cap_std::fs::MetadataExt as _;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
    metadata
        .file_attributes()
        .is_some_and(|attributes| attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0)
}

#[cfg(not(windows))]
const fn is_reparse_point(_metadata: &Metadata) -> bool {
    false
}

#[cfg(unix)]
fn same_metadata(before: &Metadata, after: &Metadata) -> bool {
    use cap_std::fs::MetadataExt as _;

    before.dev() == after.dev()
        && before.ino() == after.ino()
        && before.mode() == after.mode()
        && before.nlink() == after.nlink()
        && before.len() == after.len()
        && before.mtime() == after.mtime()
        && before.mtime_nsec() == after.mtime_nsec()
        && before.ctime() == after.ctime()
        && before.ctime_nsec() == after.ctime_nsec()
}

#[cfg(not(unix))]
fn same_metadata(before: &Metadata, after: &Metadata) -> bool {
    before.file_type() == after.file_type()
        && before.len() == after.len()
        && before.modified().ok() == after.modified().ok()
        && has_one_link(before) == has_one_link(after)
        && is_reparse_point(before) == is_reparse_point(after)
}

#[cfg(unix)]
fn set_nonblocking(options: &mut OpenOptions) {
    use cap_std::fs::OpenOptionsExt as _;

    options.custom_flags(
        i32::try_from(rustix::fs::OFlags::NONBLOCK.bits())
            .expect("O_NONBLOCK flag bits fit the platform open flag type"),
    );
}

#[cfg(not(unix))]
fn set_nonblocking(_options: &mut OpenOptions) {}

#[cfg(test)]
mod tests;
