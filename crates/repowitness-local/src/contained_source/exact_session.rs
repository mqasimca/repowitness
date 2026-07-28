use std::collections::BTreeMap;
use std::ffi::OsStr;

use cap_std::fs::{Dir, OpenOptions, ReadDir};

use super::*;

/// An operation-scoped exact-path cache for a bounded set of source reads.
///
/// Each visited directory is enumerated at most once. Callers create a new
/// session for every independent validation pass so cached observations never
/// replace final path and content revalidation.
pub(crate) struct ExactReadSession<'root> {
    root: &'root ContainedSourceRoot,
    nodes: Vec<ExactPathNode>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExactReadSessionError {
    Cancelled,
    DeadlineExceeded,
}

#[derive(Default)]
struct ExactPathNode {
    children: BTreeMap<Box<[u8]>, usize>,
    found: bool,
    unresolved_children: usize,
    scan: ExactDirectoryScan,
}

#[derive(Default)]
struct ExactDirectoryScan {
    entries: Option<ReadDir>,
    exhausted: bool,
    inspected: u64,
}

impl<'root> ExactReadSession<'root> {
    pub(super) fn new<'path>(
        root: &'root ContainedSourceRoot,
        paths: impl IntoIterator<Item = &'path RepositoryPath>,
        deadline: Instant,
        is_cancelled: &mut impl FnMut() -> bool,
    ) -> Result<Self, ExactReadSessionError> {
        check_plan_control(deadline, is_cancelled)?;
        let mut session = Self {
            root,
            nodes: vec![ExactPathNode::default()],
        };
        for path in paths {
            session.plan(path, deadline, is_cancelled)?;
        }
        for node in &mut session.nodes {
            check_plan_control(deadline, is_cancelled)?;
            node.unresolved_children = node.children.len();
        }
        Ok(session)
    }

    pub(crate) fn read_with_cancel(
        &mut self,
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
        let mut file = self.open_exact_regular_file(path, limits, deadline, &mut is_cancelled)?;
        read_regular_file(&mut file, limits, deadline, &mut is_cancelled)
    }

    fn plan(
        &mut self,
        path: &RepositoryPath,
        deadline: Instant,
        is_cancelled: &mut impl FnMut() -> bool,
    ) -> Result<(), ExactReadSessionError> {
        let mut parent = 0_usize;
        for component in path.components() {
            check_plan_control(deadline, is_cancelled)?;
            let child = self.nodes[parent].children.get(component).copied();
            parent = match child {
                Some(child) => child,
                None => {
                    let child = self.nodes.len();
                    self.nodes.push(ExactPathNode::default());
                    self.nodes[parent].children.insert(component.into(), child);
                    child
                }
            };
        }
        Ok(())
    }

    fn open_exact_regular_file(
        &mut self,
        path: &RepositoryPath,
        limits: SourceReadLimits,
        deadline: Instant,
        is_cancelled: &mut impl FnMut() -> bool,
    ) -> Result<File, ContainedSourceError> {
        check_control(limits, deadline, is_cancelled)?;
        let mut directory = self
            .root
            .root
            .try_clone()
            .map_err(|source| ContainedSourceError::RootClone { source })?;
        let mut components = path.components().peekable();
        let mut ordinal = 0_u32;
        let mut parent = 0_usize;

        while let Some(component) = components.next() {
            ordinal = ordinal
                .checked_add(1)
                .ok_or(ContainedSourceError::ComponentCountOverflowed)?;
            check_control(limits, deadline, is_cancelled)?;
            let child = self.nodes[parent]
                .children
                .get(component)
                .copied()
                .ok_or(ContainedSourceError::ExactComponentUnavailable { ordinal })?;
            self.ensure_exact_component(
                &directory,
                parent,
                child,
                ordinal,
                limits,
                deadline,
                is_cancelled,
            )?;
            let component = repository_component(component)?;
            if components.peek().is_some() {
                directory = directory
                    .open_dir_nofollow(&component)
                    .map_err(|source| ContainedSourceError::DirectoryOpen { ordinal, source })?;
                parent = child;
                continue;
            }

            return open_regular_file(&directory, &component, limits, deadline, is_cancelled);
        }

        Err(ContainedSourceError::RepositoryPathHadNoComponents)
    }

    #[allow(clippy::too_many_arguments)]
    fn ensure_exact_component(
        &mut self,
        directory: &Dir,
        parent: usize,
        child: usize,
        ordinal: u32,
        limits: SourceReadLimits,
        deadline: Instant,
        is_cancelled: &mut impl FnMut() -> bool,
    ) -> Result<(), ContainedSourceError> {
        loop {
            if self.nodes[child].found {
                return Ok(());
            }
            if self.nodes[parent].scan.exhausted {
                return Err(ContainedSourceError::ExactComponentUnavailable { ordinal });
            }
            check_control(limits, deadline, is_cancelled)?;
            if self.nodes[parent].scan.entries.is_none() {
                self.nodes[parent].scan.entries = Some(directory.entries().map_err(|source| {
                    ContainedSourceError::DirectoryEntryRead { ordinal, source }
                })?);
            }

            let entry = {
                let scan = &mut self.nodes[parent].scan;
                scan.entries
                    .as_mut()
                    .expect("an initialized exact directory scan must have entries")
                    .next()
            };
            let Some(entry) = entry else {
                let scan = &mut self.nodes[parent].scan;
                scan.entries = None;
                scan.exhausted = true;
                continue;
            };
            self.record_entry(parent, ordinal, limits, deadline, is_cancelled, entry)?;
        }
    }

    fn record_entry(
        &mut self,
        parent: usize,
        ordinal: u32,
        limits: SourceReadLimits,
        deadline: Instant,
        is_cancelled: &mut impl FnMut() -> bool,
        entry: io::Result<DirEntry>,
    ) -> Result<(), ContainedSourceError> {
        let inspected = self.nodes[parent].scan.inspected.checked_add(1).ok_or(
            ContainedSourceError::DirectoryEntryLimitExceeded {
                limit: MAX_EXACT_DIRECTORY_ENTRIES,
            },
        )?;
        self.nodes[parent].scan.inspected = inspected;
        if inspected > MAX_EXACT_DIRECTORY_ENTRIES {
            return Err(ContainedSourceError::DirectoryEntryLimitExceeded {
                limit: MAX_EXACT_DIRECTORY_ENTRIES,
            });
        }
        check_control(limits, deadline, is_cancelled)?;
        let entry =
            entry.map_err(|source| ContainedSourceError::DirectoryEntryRead { ordinal, source })?;
        let name = entry.file_name();
        let matched = host_component_bytes(&name)
            .and_then(|name| self.nodes[parent].children.get(name))
            .copied();
        if let Some(matched) = matched
            && !self.nodes[matched].found
        {
            self.nodes[matched].found = true;
            let unresolved = self.nodes[parent]
                .unresolved_children
                .checked_sub(1)
                .ok_or(ContainedSourceError::ComponentCountOverflowed)?;
            self.nodes[parent].unresolved_children = unresolved;
            if unresolved == 0 {
                self.nodes[parent].scan.entries = None;
                self.nodes[parent].scan.exhausted = true;
            }
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn inspected_entry_count(&self) -> u64 {
        self.nodes.iter().map(|node| node.scan.inspected).sum()
    }

    #[cfg(test)]
    pub(crate) fn open_directory_scan_count(&self) -> usize {
        self.nodes
            .iter()
            .filter(|node| node.scan.entries.is_some())
            .count()
    }
}

fn open_regular_file(
    directory: &Dir,
    component: &Path,
    limits: SourceReadLimits,
    deadline: Instant,
    is_cancelled: &mut impl FnMut() -> bool,
) -> Result<File, ContainedSourceError> {
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    set_nonblocking_if_supported(&mut options);
    let file = directory
        .open_with(component, &options)
        .map_err(|source| ContainedSourceError::FileOpen { source })?;
    let metadata = file
        .metadata()
        .map_err(|source| ContainedSourceError::MetadataRead { source })?;
    if !metadata.is_file() {
        return Err(ContainedSourceError::NotRegularFile);
    }
    if metadata.len() > limits.file_bytes() {
        return Err(ContainedSourceError::FileByteLimitExceeded {
            limit: limits.file_bytes(),
        });
    }
    check_control(limits, deadline, is_cancelled)?;
    Ok(file)
}

#[cfg(unix)]
fn host_component_bytes(component: &OsStr) -> Option<&[u8]> {
    use std::os::unix::ffi::OsStrExt as _;

    Some(component.as_bytes())
}

#[cfg(not(unix))]
fn host_component_bytes(component: &OsStr) -> Option<&[u8]> {
    component.to_str().map(str::as_bytes)
}

fn check_plan_control(
    deadline: Instant,
    is_cancelled: &mut impl FnMut() -> bool,
) -> Result<(), ExactReadSessionError> {
    if is_cancelled() {
        return Err(ExactReadSessionError::Cancelled);
    }
    if Instant::now() >= deadline {
        return Err(ExactReadSessionError::DeadlineExceeded);
    }
    Ok(())
}
