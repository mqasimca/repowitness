use std::ffi::OsString;

use super::*;

struct CodexTempDirectory(PathBuf);

impl CodexTempDirectory {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let ordinal = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root =
            std::fs::canonicalize(std::env::temp_dir()).expect("canonicalize temporary directory");
        let path = root.join(format!(
            "repowitness-cli-codex-{}-{ordinal}",
            std::process::id()
        ));
        std::fs::create_dir(&path).expect("create temporary Codex home");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for CodexTempDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(unix)]
#[test]
fn codex_install_and_remove_own_only_the_marked_catalog_records() {
    let directory = CodexTempDirectory::new();
    let configuration = directory.path().join(CODEX_CONFIG_FILE);
    std::fs::write(&configuration, "model = \"gpt-5\"\n").expect("seed configuration");

    let install = [
        OsString::from("install"),
        OsString::from("--codex-home"),
        directory.path().as_os_str().to_owned(),
    ];
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    assert_eq!(
        run_codex(install.clone().into_iter(), &mut stdout, &mut stderr),
        EXIT_SUCCESS
    );
    assert_eq!(stderr, Vec::<u8>::new());
    assert_eq!(
        String::from_utf8(stdout).expect("UTF-8"),
        "status=ok\noperation=codex-install\nintegration=global-catalog\nrestart=required\n"
    );
    let installed = std::fs::read_to_string(&configuration).expect("read installed configuration");
    assert!(installed.starts_with("model = \"gpt-5\"\n"));
    assert_eq!(installed.matches(CODEX_INTEGRATION_BEGIN).count(), 1);
    assert!(installed.contains("[mcp_servers.repowitness]"));
    assert!(installed.contains("--catalog-state-dir"));
    assert!(installed.contains(CODEX_CATALOG_STATE_DIRECTORY));
    assert!(installed.contains("repowitness codex session-start"));

    let mut stdout = Vec::new();
    assert_eq!(
        run_codex(install.into_iter(), &mut stdout, &mut stderr),
        EXIT_SUCCESS
    );
    assert_eq!(
        String::from_utf8(stdout).expect("UTF-8"),
        "status=ok\noperation=codex-install\nintegration=already-installed\nrestart=not-required\n"
    );
    assert_eq!(
        std::fs::read_to_string(&configuration).expect("read repeat configuration"),
        installed
    );

    let remove = [
        OsString::from("remove"),
        OsString::from("--codex-home"),
        directory.path().as_os_str().to_owned(),
    ];
    let mut stdout = Vec::new();
    assert_eq!(
        run_codex(remove.clone().into_iter(), &mut stdout, &mut stderr),
        EXIT_SUCCESS
    );
    assert_eq!(
        String::from_utf8(stdout).expect("UTF-8"),
        "status=ok\noperation=codex-remove\nintegration=removed\nrestart=required\n"
    );
    assert_eq!(
        std::fs::read_to_string(&configuration).expect("read removed configuration"),
        "model = \"gpt-5\"\n"
    );

    let mut stdout = Vec::new();
    assert_eq!(
        run_codex(remove.into_iter(), &mut stdout, &mut stderr),
        EXIT_SUCCESS
    );
    assert_eq!(
        String::from_utf8(stdout).expect("UTF-8"),
        "status=ok\noperation=codex-remove\nintegration=absent\nrestart=not-required\n"
    );
}

#[cfg(not(unix))]
#[test]
fn codex_install_fails_closed_without_private_catalog_state() {
    let directory = CodexTempDirectory::new();
    let configuration = directory.path().join(CODEX_CONFIG_FILE);
    std::fs::write(&configuration, "model = \"gpt-5\"\n").expect("seed configuration");

    let install = [
        OsString::from("install"),
        OsString::from("--codex-home"),
        directory.path().as_os_str().to_owned(),
    ];
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    assert_eq!(
        run_codex(install.into_iter(), &mut stdout, &mut stderr),
        EXIT_SOFTWARE
    );
    assert_eq!(stdout, Vec::<u8>::new());
    assert_eq!(
        String::from_utf8(stderr).expect("UTF-8"),
        "error: Codex catalog is unavailable on this platform\n"
    );
    assert_eq!(
        std::fs::read_to_string(&configuration).expect("read unchanged configuration"),
        "model = \"gpt-5\"\n"
    );
}

#[test]
fn codex_install_preserves_unmanaged_representation_and_reports_no_paths() {
    let directory = CodexTempDirectory::new();
    let configuration = directory.path().join(CODEX_CONFIG_FILE);
    let original = "[mcp_servers]\nrepowitness = { command = \"different\" }\n";
    std::fs::write(&configuration, original).expect("seed configuration");
    let arguments = [
        OsString::from("install"),
        OsString::from("--codex-home"),
        directory.path().as_os_str().to_owned(),
    ];
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    assert_eq!(
        run_codex(arguments.into_iter(), &mut stdout, &mut stderr),
        EXIT_SOFTWARE
    );
    assert!(stdout.is_empty());
    let stderr = String::from_utf8(stderr).expect("UTF-8");
    assert_eq!(
        stderr,
        "error: Codex global configuration could not be updated\n"
    );
    assert!(!stderr.contains(&directory.path().display().to_string()));
    assert_eq!(
        std::fs::read_to_string(&configuration).expect("read preserved configuration"),
        original
    );
}

#[test]
fn codex_session_start_is_non_mutating_and_path_free() {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    assert_eq!(
        run_codex(
            [OsString::from("session-start")].into_iter(),
            &mut stdout,
            &mut stderr,
        ),
        EXIT_SUCCESS
    );
    assert!(stderr.is_empty());
    let message = String::from_utf8(stdout).expect("UTF-8");
    assert!(message.contains("RepoWitness catalog"));
    assert!(!message.contains("/"));
}
