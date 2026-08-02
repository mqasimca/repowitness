use std::{
    cell::{Cell, RefCell},
    ffi::OsString,
    path::{Path, PathBuf},
};

use super::*;

const REPOSITORY_ID: &str = concat!(
    "rwi1:h:",
    "A1A1A1A1A1A1A1A1",
    "A1A1A1A1A1A1A1A1",
    "A1A1A1A1A1A1A1A1",
    "A1A1A1A1A1A1A1A1"
);

struct RecordingIdentityGenerator {
    identity: Result<String, LocalIdentityGenerationError>,
    calls: Cell<u64>,
}

impl IdentityGenerator for RecordingIdentityGenerator {
    fn generate(&self, kind: LocalIdentityKind) -> Result<String, LocalIdentityGenerationError> {
        assert_eq!(kind, LocalIdentityKind::Repository);
        self.calls.set(self.calls.get() + 1);
        self.identity.clone()
    }
}

struct RecordingStateDirectory {
    database: PathBuf,
    prepare_result: Result<(), ()>,
    prepare_calls: Cell<u64>,
    root: RefCell<Option<PathBuf>>,
    state_dir: RefCell<Option<Option<PathBuf>>>,
    identity: RefCell<Option<String>>,
}

impl RecordingStateDirectory {
    fn success(database: impl Into<PathBuf>) -> Self {
        Self {
            database: database.into(),
            prepare_result: Ok(()),
            prepare_calls: Cell::new(0),
            root: RefCell::new(None),
            state_dir: RefCell::new(None),
            identity: RefCell::new(None),
        }
    }
}

impl OnboardStateDirectory for RecordingStateDirectory {
    fn prepare_database(
        &self,
        repository_root: &Path,
        state_dir: Option<&Path>,
        repository_identity: &str,
    ) -> Result<PreparedOnboardDatabase, ()> {
        self.prepare_calls.set(self.prepare_calls.get() + 1);
        self.root.replace(Some(repository_root.to_owned()));
        self.state_dir.replace(Some(state_dir.map(Path::to_owned)));
        self.identity.replace(Some(repository_identity.to_owned()));
        self.prepare_result
            .as_ref()
            .map(|()| PreparedOnboardDatabase {
                database: self.database.clone(),
            })
            .map_err(|_| ())
    }
}

fn invoke_onboard(
    arguments: &[&str],
    indexer: &impl RepositoryIndexer,
    identity: &impl IdentityGenerator,
    state: &impl OnboardStateDirectory,
) -> (u8, String, String) {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let inspector = FakeInspector::success(GitPathDiscoveryStats::new(0, 0, 0, 0, 0));
    let code = run_onboard(
        arguments.iter().map(OsString::from),
        &mut stdout,
        &mut stderr,
        &inspector,
        indexer,
        identity,
        state,
    );
    (
        code,
        String::from_utf8(stdout).expect("onboarding stdout is UTF-8"),
        String::from_utf8(stderr).expect("onboarding stderr is UTF-8"),
    )
}

#[test]
fn onboard_uses_one_explicit_root_and_generated_identity_without_leaking_paths() {
    let indexer = FakeIndexer::success(index_report());
    let identity = RecordingIdentityGenerator {
        identity: Ok(REPOSITORY_ID.to_owned()),
        calls: Cell::new(0),
    };
    let state = RecordingStateDirectory::success(
        "/private/state/repowitness/repositories/opaque/index.sqlite3",
    );

    let (code, stdout, stderr) = invoke_onboard(
        &[
            "--root",
            "/private/repository",
            "--state-dir",
            "/private/state",
        ],
        &indexer,
        &identity,
        &state,
    );

    assert_eq!(code, EXIT_SUCCESS);
    assert_eq!(
        stdout,
        format!(
            "status=ok\noperation=onboard\nrepository_id={REPOSITORY_ID}\nstate_directory_convention=repowitness/repositories/<repository-id>/index.sqlite3\ngeneration_activated=true\ngeneration=3\nsource_epoch=0\nrepository_paths=8\n"
        )
    );
    assert!(stderr.is_empty());
    assert_eq!(identity.calls.get(), 1);
    assert_eq!(state.prepare_calls.get(), 1);
    assert_eq!(indexer.calls.get(), 1);
    assert_eq!(
        state.root.borrow().as_deref(),
        Some(Path::new("/private/repository"))
    );
    assert_eq!(
        state.state_dir.borrow().as_ref().and_then(Option::as_deref),
        Some(Path::new("/private/state"))
    );
    assert_eq!(state.identity.borrow().as_deref(), Some(REPOSITORY_ID));
    assert_eq!(
        indexer.database.borrow().as_deref(),
        Some(Path::new(
            "/private/state/repowitness/repositories/opaque/index.sqlite3"
        ))
    );
    assert!(!stdout.contains("/private/repository"));
    assert!(!stdout.contains("/private/state"));
}

#[test]
fn caller_supplied_canonical_identity_is_reused_without_entropy_or_repository_discovery() {
    let indexer = FakeIndexer::success(index_report());
    let identity = RecordingIdentityGenerator {
        identity: Err(LocalIdentityGenerationError::EntropyUnavailable),
        calls: Cell::new(0),
    };
    let state = RecordingStateDirectory::success("/private/state/index.sqlite3");

    let (code, stdout, stderr) = invoke_onboard(
        &[
            "--repository-id",
            REPOSITORY_ID,
            "--root",
            "/private/only-this-root",
        ],
        &indexer,
        &identity,
        &state,
    );

    assert_eq!(code, EXIT_SUCCESS);
    assert!(stderr.is_empty());
    assert!(stdout.contains(REPOSITORY_ID));
    assert_eq!(identity.calls.get(), 0);
    assert_eq!(state.prepare_calls.get(), 1);
    assert_eq!(state.state_dir.borrow().as_ref(), Some(&None));
    assert_eq!(indexer.calls.get(), 1);
    assert_eq!(
        indexer.repository_root.borrow().as_deref(),
        Some(Path::new("/private/only-this-root"))
    );
}

#[test]
fn invalid_input_and_noncanonical_identity_fail_before_state_or_index_writes() {
    let indexer = FakeIndexer::failure("must not be called");
    let identity = RecordingIdentityGenerator {
        identity: Ok(REPOSITORY_ID.to_owned()),
        calls: Cell::new(0),
    };
    let state = RecordingStateDirectory::success("/private/state/index.sqlite3");

    for arguments in [
        vec!["--root"],
        vec!["--root", ""],
        vec!["--root", "/private/root", "--root", "/private/other"],
        vec!["--root", "/private/root", "--unknown", "private"],
        vec![
            "--repository-id",
            "not-canonical",
            "--root",
            "/private/root",
        ],
        vec!["--help", "unexpected"],
    ] {
        let (code, stdout, stderr) = invoke_onboard(&arguments, &indexer, &identity, &state);
        assert_eq!(code, EXIT_USAGE);
        assert!(stdout.is_empty());
        assert!(stderr.starts_with("error:"));
        assert!(!stderr.contains("private"));
    }
    assert_eq!(identity.calls.get(), 0);
    assert_eq!(state.prepare_calls.get(), 0);
    assert_eq!(indexer.calls.get(), 0);
}

#[test]
fn unavailable_private_state_is_redacted_and_does_not_index() {
    let indexer = FakeIndexer::failure("must not be called");
    let identity = RecordingIdentityGenerator {
        identity: Ok(REPOSITORY_ID.to_owned()),
        calls: Cell::new(0),
    };
    let state = RecordingStateDirectory {
        database: PathBuf::from("/private/state/index.sqlite3"),
        prepare_result: Err(()),
        prepare_calls: Cell::new(0),
        root: RefCell::new(None),
        state_dir: RefCell::new(None),
        identity: RefCell::new(None),
    };

    let (code, stdout, stderr) =
        invoke_onboard(&["--root", "/private/root"], &indexer, &identity, &state);

    assert_eq!(code, EXIT_SOFTWARE);
    assert!(stdout.is_empty());
    assert_eq!(stderr, "error: private onboarding state is unavailable\n");
    assert_eq!(indexer.calls.get(), 0);
    assert!(!stderr.contains("/private"));
}
