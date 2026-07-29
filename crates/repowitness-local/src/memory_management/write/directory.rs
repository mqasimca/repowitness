use std::path::{Path, PathBuf};

use cap_fs_ext::DirExt;
use cap_std::{ambient_authority, fs::Dir};

use crate::contained_source::FileIdentity;

use super::LocalMemoryManageError;

pub(super) struct RecordsDirectoryAuthority {
    directory: Dir,
    worktree: PathBuf,
    identity: FileIdentity,
}

impl RecordsDirectoryAuthority {
    pub(super) fn new(directory: Dir, worktree: &Path) -> Result<Self, LocalMemoryManageError> {
        let identity = directory_identity(&directory)?;
        let authority = Self {
            directory,
            worktree: worktree.to_path_buf(),
            identity,
        };
        authority.verify_current_path()?;
        Ok(authority)
    }

    pub(super) const fn directory(&self) -> &Dir {
        &self.directory
    }

    pub(super) fn verify_current_path(&self) -> Result<(), LocalMemoryManageError> {
        let root = Dir::open_ambient_dir(&self.worktree, ambient_authority())
            .map_err(|_| LocalMemoryManageError::FilePublicationFailed)?;
        let memory = root
            .open_dir_nofollow(".code-memory")
            .map_err(|_| LocalMemoryManageError::FilePublicationFailed)?;
        let records = memory
            .open_dir_nofollow("records")
            .map_err(|_| LocalMemoryManageError::FilePublicationFailed)?;
        if directory_identity(&records)? != self.identity {
            return Err(LocalMemoryManageError::FilePublicationFailed);
        }
        Ok(())
    }
}

fn directory_identity(directory: &Dir) -> Result<FileIdentity, LocalMemoryManageError> {
    FileIdentity::from_file(
        directory
            .try_clone()
            .map_err(|_| LocalMemoryManageError::FilePublicationFailed)?
            .into_std_file(),
    )
    .map_err(|_| LocalMemoryManageError::FilePublicationFailed)
}
