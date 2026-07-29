fn exact_entry(
    directory: &Dir,
    expected: &Path,
    ordinal: u32,
    limits: SourceReadLimits,
    deadline: Instant,
    is_cancelled: &mut impl FnMut() -> bool,
) -> Result<DirEntry, ContainedSourceError> {
    let entries = directory
        .entries()
        .map_err(|source| ContainedSourceError::DirectoryEntryRead { ordinal, source })?;
    let mut inspected = 0_u64;
    for entry in entries {
        inspected =
            inspected
                .checked_add(1)
                .ok_or(ContainedSourceError::DirectoryEntryLimitExceeded {
                    limit: MAX_EXACT_DIRECTORY_ENTRIES,
                })?;
        if inspected > MAX_EXACT_DIRECTORY_ENTRIES {
            return Err(ContainedSourceError::DirectoryEntryLimitExceeded {
                limit: MAX_EXACT_DIRECTORY_ENTRIES,
            });
        }
        check_control(limits, deadline, is_cancelled)?;
        let entry =
            entry.map_err(|source| ContainedSourceError::DirectoryEntryRead { ordinal, source })?;
        if entry.file_name() == expected.as_os_str() {
            return Ok(entry);
        }
    }
    Err(ContainedSourceError::ExactComponentUnavailable { ordinal })
}

#[cfg(unix)]
fn has_one_link(metadata: &Metadata) -> bool {
    use cap_std::fs::MetadataExt as _;

    metadata.nlink() == 1
}

#[cfg(windows)]
fn has_one_link(metadata: &Metadata) -> bool {
    use cap_fs_ext::MetadataExt as _;

    metadata.nlink() == 1
}

#[cfg(not(any(unix, windows)))]
fn has_one_link(_metadata: &Metadata) -> bool {
    false
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
