use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
};

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::{
    ambient_authority,
    fs::{Dir, File, Metadata, OpenOptions},
};

use crate::contained_source::FileIdentity;

use super::super::{SqliteStoreError, canonical_database_path};

pub(super) struct BackupSourceAuthority {
    path: PathBuf,
    file: File,
    identity: FileIdentity,
}

impl BackupSourceAuthority {
    pub(super) fn open(path: &Path) -> Result<Self, SqliteStoreError> {
        let path = canonical_database_path(path)?;
        let (directory, name) = open_parent(&path)?;
        let file = open_unique_regular(&directory, &name)?;
        let identity = file_identity(&file)?;
        let authority = Self {
            path,
            file,
            identity,
        };
        authority.verify_current_path()?;
        Ok(authority)
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn verify_current_path(&self) -> Result<(), SqliteStoreError> {
        validate_unique_regular(&self.file)?;
        let (directory, name) = open_parent(&self.path)?;
        let current = open_unique_regular(&directory, &name)?;
        if file_identity(&current)? != self.identity {
            return Err(SqliteStoreError::DatabaseIdentityChanged);
        }
        Ok(())
    }
}

pub(super) struct BackupDestinationAuthority {
    directory: Dir,
    directory_path: PathBuf,
    directory_identity: FileIdentity,
    destination_name: OsString,
}

impl BackupDestinationAuthority {
    pub(super) fn open(destination: &Path) -> Result<Self, SqliteStoreError> {
        let destination = canonical_database_path(destination)?;
        let destination_name = destination
            .file_name()
            .ok_or(SqliteStoreError::BackupDestinationUnavailable)?
            .to_os_string();
        let directory_path = destination
            .parent()
            .ok_or(SqliteStoreError::BackupDestinationUnavailable)?
            .to_path_buf();
        let directory = Dir::open_ambient_dir(&directory_path, ambient_authority())
            .map_err(|_| SqliteStoreError::BackupDestinationUnavailable)?;
        let directory_identity = directory_identity(&directory)?;
        let authority = Self {
            directory,
            directory_path,
            directory_identity,
            destination_name,
        };
        authority.verify_current_directory()?;
        Ok(authority)
    }

    pub(super) fn destination_path(&self) -> PathBuf {
        self.directory_path.join(&self.destination_name)
    }

    pub(super) fn temporary_name(&self) -> OsString {
        let mut temporary = self.destination_name.clone();
        temporary.push(format!(".repowitness-partial-{}", std::process::id()));
        temporary
    }

    pub(super) fn temporary_path(&self, name: &OsStr) -> PathBuf {
        self.directory_path.join(name)
    }

    pub(super) fn create_temporary(&self, name: &OsStr) -> Result<File, SqliteStoreError> {
        let mut options = OpenOptions::new();
        options
            .read(true)
            .write(true)
            .create_new(true)
            .follow(FollowSymlinks::No);
        let file = self
            .directory
            .open_with(name, &options)
            .map_err(|_| SqliteStoreError::BackupDestinationUnavailable)?;
        validate_unique_regular(&file)
            .map_err(|_| SqliteStoreError::BackupDestinationUnavailable)?;
        Ok(file)
    }

    pub(super) fn publish(&self, temporary: &OsStr) -> Result<(), SqliteStoreError> {
        self.verify_current_directory()?;
        self.directory
            .hard_link(temporary, &self.directory, &self.destination_name)
            .map_err(|_| SqliteStoreError::BackupDestinationUnavailable)
    }

    pub(super) fn remove(&self, name: &OsStr) -> Result<(), SqliteStoreError> {
        self.directory
            .remove_file(name)
            .map_err(|_| SqliteStoreError::BackupCleanupFailed)
    }

    pub(super) fn verify_destination(
        &self,
        expected: &FileIdentity,
    ) -> Result<(), SqliteStoreError> {
        self.verify_named_file(&self.destination_name, expected)
    }

    pub(super) fn sync(&self) -> Result<(), SqliteStoreError> {
        self.verify_current_directory()?;
        self.directory
            .try_clone()
            .map(Dir::into_std_file)
            .and_then(|directory| directory.sync_all())
            .map_err(|_| SqliteStoreError::BackupCleanupFailed)
    }

    pub(super) fn verify_open_file(
        &self,
        file: &File,
        expected: &FileIdentity,
    ) -> Result<(), SqliteStoreError> {
        validate_unique_regular(file)?;
        if &file_identity(file)? != expected {
            return Err(SqliteStoreError::DatabaseIdentityChanged);
        }
        Ok(())
    }

    pub(super) fn verify_named_file(
        &self,
        name: &OsStr,
        expected: &FileIdentity,
    ) -> Result<(), SqliteStoreError> {
        self.verify_current_directory()?;
        let file = open_unique_regular(&self.directory, name)?;
        self.verify_open_file(&file, expected)
    }

    pub(super) fn verify_named_identity(
        &self,
        name: &OsStr,
        expected: &FileIdentity,
    ) -> Result<(), SqliteStoreError> {
        let file = open_regular(&self.directory, name)?;
        if &file_identity(&file)? != expected {
            return Err(SqliteStoreError::DatabaseIdentityChanged);
        }
        Ok(())
    }

    fn verify_current_directory(&self) -> Result<(), SqliteStoreError> {
        let current = open_directory_nofollow(&self.directory_path)?;
        if directory_identity(&current)? != self.directory_identity {
            return Err(SqliteStoreError::DatabaseIdentityChanged);
        }
        Ok(())
    }
}

pub(super) fn file_identity(file: &File) -> Result<FileIdentity, SqliteStoreError> {
    FileIdentity::from_file(
        file.try_clone()
            .map_err(|_| SqliteStoreError::DatabaseIdentityChanged)?
            .into_std(),
    )
    .map_err(|_| SqliteStoreError::DatabaseIdentityChanged)
}

fn open_parent(path: &Path) -> Result<(Dir, OsString), SqliteStoreError> {
    let parent = path
        .parent()
        .ok_or(SqliteStoreError::DatabaseIdentityChanged)?;
    let name = path
        .file_name()
        .ok_or(SqliteStoreError::DatabaseIdentityChanged)?
        .to_os_string();
    let directory = Dir::open_ambient_dir(parent, ambient_authority())
        .map_err(|_| SqliteStoreError::DatabaseIdentityChanged)?;
    Ok((directory, name))
}

fn open_directory_nofollow(path: &Path) -> Result<Dir, SqliteStoreError> {
    let Some(name) = path.file_name() else {
        return Dir::open_ambient_dir(path, ambient_authority())
            .map_err(|_| SqliteStoreError::DatabaseIdentityChanged);
    };
    let parent = path
        .parent()
        .ok_or(SqliteStoreError::DatabaseIdentityChanged)?;
    let parent = Dir::open_ambient_dir(parent, ambient_authority())
        .map_err(|_| SqliteStoreError::DatabaseIdentityChanged)?;
    parent
        .open_dir_nofollow(name)
        .map_err(|_| SqliteStoreError::DatabaseIdentityChanged)
}

fn directory_identity(directory: &Dir) -> Result<FileIdentity, SqliteStoreError> {
    FileIdentity::from_file(
        directory
            .try_clone()
            .map_err(|_| SqliteStoreError::DatabaseIdentityChanged)?
            .into_std_file(),
    )
    .map_err(|_| SqliteStoreError::DatabaseIdentityChanged)
}

fn open_unique_regular(directory: &Dir, name: &OsStr) -> Result<File, SqliteStoreError> {
    let file = open_regular(directory, name)?;
    validate_unique_regular(&file)?;
    Ok(file)
}

fn open_regular(directory: &Dir, name: &OsStr) -> Result<File, SqliteStoreError> {
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let file = directory
        .open_with(name, &options)
        .map_err(|_| SqliteStoreError::DatabaseIdentityChanged)?;
    let metadata = file
        .metadata()
        .map_err(|_| SqliteStoreError::DatabaseIdentityChanged)?;
    if !metadata.is_file() {
        return Err(SqliteStoreError::DatabaseIdentityChanged);
    }
    Ok(file)
}

fn validate_unique_regular(file: &File) -> Result<(), SqliteStoreError> {
    let metadata = file
        .metadata()
        .map_err(|_| SqliteStoreError::DatabaseIdentityChanged)?;
    if !metadata.is_file() || !has_one_link(&metadata) {
        return Err(SqliteStoreError::DatabaseIdentityChanged);
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
