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
        "repository path component 1 is unavailable with exact spelling"
    );
    fs::write(fixture.path().join("empty"), b"").expect("empty fixture must be written");
    assert!(
        root.read(&path(b"empty"), limits(0, 1))
            .expect("zero limit accepts an empty file")
            .is_empty()
    );
}

#[test]
fn ordinary_reads_require_exact_component_spelling() {
    let fixture = TempDirectory::new();
    fs::create_dir(fixture.path().join("Exact")).expect("fixture directory should be created");
    fs::write(fixture.path().join("Exact/Source.rs"), b"exact")
        .expect("fixture source should be written");
    let root = ContainedSourceRoot::open(fixture.path()).expect("root should open");

    assert!(matches!(
        root.read(&path(b"exact/Source.rs"), limits(1024, 64)),
        Err(ContainedSourceError::ExactComponentUnavailable { ordinal: 1 })
    ));
    assert!(matches!(
        root.read(&path(b"Exact/source.rs"), limits(1024, 64)),
        Err(ContainedSourceError::ExactComponentUnavailable { ordinal: 2 })
    ));
    assert_eq!(
        root.read(&path(b"Exact/Source.rs"), limits(1024, 64))
            .expect("exact path should read")
            .as_ref(),
        b"exact"
    );
}

#[test]
fn exact_component_checks_classify_alternate_spelling_without_reading_the_leaf() {
    let fixture = TempDirectory::new();
    fs::create_dir(fixture.path().join("Exact")).expect("fixture directory should be created");
    fs::write(fixture.path().join("Exact/Source.rs"), b"exact")
        .expect("fixture source should be written");
    let exact = path(b"Exact/Source.rs");
    let alternate_directory = path(b"exact/Source.rs");
    let alternate_leaf = path(b"Exact/source.rs");
    let paths = [&exact, &alternate_directory, &alternate_leaf];
    let root = ContainedSourceRoot::open(fixture.path()).expect("root should open");
    let deadline_duration = Duration::from_secs(1);
    let deadline = Instant::now() + deadline_duration;
    let mut session = root
        .exact_read_session(paths, deadline, || false)
        .expect("exact-component plan should complete");

    assert!(
        !session
            .exact_components_available(
                &alternate_directory,
                deadline_duration,
                deadline,
                &mut || false,
            )
            .expect("alternate directory spelling should be classified")
    );
    assert!(
        !session
            .exact_components_available(
                &alternate_leaf,
                deadline_duration,
                deadline,
                &mut || false,
            )
            .expect("alternate leaf spelling should be classified")
    );
    assert!(
        session
            .exact_components_available(&exact, deadline_duration, deadline, &mut || false,)
            .expect("exact spelling should be classified")
    );
}

#[test]
fn exact_read_sessions_scan_flat_directories_once() {
    const FILE_COUNT: u64 = 256;

    let fixture = TempDirectory::new();
    fs::create_dir(fixture.path().join("src")).expect("source directory must be created");
    let paths = (0..FILE_COUNT)
        .map(|index| {
            let relative = format!("src/file-{index:04}.rs");
            fs::write(fixture.path().join(&relative), relative.as_bytes())
                .expect("source fixture must be written");
            path(relative.as_bytes())
        })
        .collect::<Vec<_>>();
    let root = ContainedSourceRoot::open(fixture.path()).expect("root must open");
    let mut session = root
        .exact_read_session(
            paths.iter(),
            Instant::now() + Duration::from_secs(1),
            || false,
        )
        .expect("exact-read plan must complete");

    for source in paths.iter().rev() {
        assert!(
            !session
                .read_with_cancel(source, SourceReadLimits::default(), || false)
                .expect("planned source must be read")
                .is_empty()
        );
    }

    assert!(
        session.inspected_entry_count() <= FILE_COUNT.saturating_mul(2).saturating_add(8),
        "directory-entry inspection must remain linear in the planned path count"
    );
    assert_eq!(
        session.open_directory_scan_count(),
        0,
        "completed exact-name proofs must release directory iterators"
    );
}

#[test]
fn exact_read_sessions_release_completed_leaf_directory_scans() {
    const DIRECTORY_COUNT: u64 = 96;

    let fixture = TempDirectory::new();
    let paths = (0..DIRECTORY_COUNT)
        .map(|index| {
            let directory = format!("module-{index:04}");
            fs::create_dir(fixture.path().join(&directory))
                .expect("source directory must be created");
            let relative = format!("{directory}/lib.rs");
            fs::write(fixture.path().join(&relative), b"fn indexed() {}\n")
                .expect("source fixture must be written");
            path(relative.as_bytes())
        })
        .collect::<Vec<_>>();
    let root = ContainedSourceRoot::open(fixture.path()).expect("root must open");
    let mut session = root
        .exact_read_session(
            paths.iter(),
            Instant::now() + Duration::from_secs(1),
            || false,
        )
        .expect("exact-read plan must complete");

    for source in &paths {
        session
            .read_with_cancel(source, SourceReadLimits::default(), || false)
            .expect("planned source must be read");
        assert!(
            session.open_directory_scan_count() <= 1,
            "completed leaf scans must not accumulate open directory iterators"
        );
    }
    assert_eq!(session.open_directory_scan_count(), 0);
}

#[test]
fn exact_read_session_planning_obeys_cancellation_and_deadlines() {
    let fixture = TempDirectory::new();
    fs::write(fixture.path().join("source.rs"), b"fn source() {}\n")
        .expect("source fixture must be written");
    let root = ContainedSourceRoot::open(fixture.path()).expect("root must open");
    let source = path(b"source.rs");

    assert!(matches!(
        root.exact_read_session(
            std::iter::once(&source),
            Instant::now() + Duration::from_secs(1),
            || true,
        ),
        Err(ExactReadSessionError::Cancelled)
    ));
    assert!(matches!(
        root.exact_read_session(std::iter::once(&source), Instant::now(), || false),
        Err(ExactReadSessionError::DeadlineExceeded)
    ));
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
    let linked_source = path(b"linked/private.rs");

    let final_error = root
        .read(&path(b"final.rs"), SourceReadLimits::default())
        .expect_err("final symlink must not be followed");
    let intermediate_error = root
        .read(&linked_source, SourceReadLimits::default())
        .expect_err("intermediate symlink must not be followed");
    let deadline_duration = Duration::from_secs(1);
    let deadline = Instant::now() + deadline_duration;
    let mut session = root
        .exact_read_session(std::iter::once(&linked_source), deadline, || false)
        .expect("symlink path plan should complete");
    let classification_error = session
        .exact_components_available(&linked_source, deadline_duration, deadline, &mut || false)
        .expect_err("exact-component checks must not follow intermediate symlinks");
    assert!(matches!(final_error, ContainedSourceError::FileOpen { .. }));
    assert!(matches!(
        intermediate_error,
        ContainedSourceError::DirectoryOpen { ordinal: 1, .. }
    ));
    assert!(matches!(
        classification_error,
        ContainedSourceError::DirectoryOpen { ordinal: 1, .. }
    ));
    assert!(!final_error.to_string().contains("private"));
    assert!(!intermediate_error.to_string().contains("private"));
    assert!(!classification_error.to_string().contains("private"));
}

#[cfg(unix)]
#[cfg_attr(
    target_vendor = "apple",
    ignore = "Apple filesystems reject the byte-invalid fixture name"
)]
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
fn file_link_count_is_checked_from_the_open_handle() {
    let fixture = TempDirectory::new();
    let original = fixture.path().join("original");
    let alias = fixture.path().join("alias");
    fs::write(&original, b"linked").expect("hard-link source must be written");
    let file = std::fs::File::open(&original).expect("single-link file must open");
    assert!(file_has_single_link(&file).expect("single-link metadata must be readable"));

    fs::hard_link(&original, &alias).expect("hard-link fixture must be created");
    assert!(!file_has_single_link(&file).expect("hard-link metadata must be readable"));
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
    fs::write(fixture.path().join("other"), b"other").expect("independent fixture must be written");
    let identity = FileIdentity::from_path(&database).expect("database identity must be readable");
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

#[cfg(any(unix, windows))]
#[test]
fn unique_exact_reads_reject_hard_links_and_accept_single_links() {
    let fixture = TempDirectory::new();
    fs::write(fixture.path().join("single"), b"single")
        .expect("single-link fixture must be written");
    fs::write(fixture.path().join("original"), b"linked")
        .expect("hard-link source must be written");
    fs::hard_link(
        fixture.path().join("original"),
        fixture.path().join("alias"),
    )
    .expect("hard-link fixture must be created");
    let root = ContainedSourceRoot::open(fixture.path()).expect("root must open");

    assert_eq!(
            root.read_unique_exact_with_cancel(
                &path(b"single"),
                SourceReadLimits::default(),
                || false,
            )
            .expect("single-link file must be admitted")
            .as_ref(),
            b"single"
        );
    assert!(matches!(
        root.read_unique_exact_with_cancel(&path(b"alias"), SourceReadLimits::default(), || false,),
        Err(ContainedSourceError::LinkCountNotUnique)
    ));
}

#[test]
fn unique_exact_reads_reject_alternate_component_case() {
    let fixture = TempDirectory::new();
    fs::create_dir(fixture.path().join("Records"))
        .expect("alternate-case directory must be created");
    fs::write(fixture.path().join("Records/Memory.yaml"), b"memory")
        .expect("alternate-case file must be written");
    let root = ContainedSourceRoot::open(fixture.path()).expect("root must open");

    assert!(matches!(
        root.read_unique_exact_with_cancel(
            &path(b"records/Memory.yaml"),
            SourceReadLimits::default(),
            || false,
        ),
        Err(ContainedSourceError::ExactComponentUnavailable { ordinal: 1 })
    ));
    assert!(matches!(
        root.read_unique_exact_with_cancel(
            &path(b"Records/memory.yaml"),
            SourceReadLimits::default(),
            || false,
        ),
        Err(ContainedSourceError::ExactComponentUnavailable { ordinal: 2 })
    ));
}

#[cfg(unix)]
#[test]
fn special_files_are_rejected_without_blocking() {
    use std::os::unix::net::UnixListener;

    let fixture = TempDirectory::new();
    let _listener =
        UnixListener::bind(fixture.path().join("socket")).expect("socket fixture must be created");
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
        .open_exact_regular_file(
            &path(b"source.rs"),
            read_limits,
            deadline,
            false,
            &mut || false,
        )
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
    assert!(matches!(
        error,
        ContainedSourceError::ExactComponentUnavailable { ordinal: 1 }
    ));
    assert!(error.source().is_none());
}
