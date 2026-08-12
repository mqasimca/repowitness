use std::{
    cell::Cell,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use repowitness_local::{ConfigurationPolicyOverrides, ConfigurationPreferenceOverrides};

use super::*;

struct FakeConfigurationLoader {
    calls: Cell<u64>,
    outcome: Result<ResolvedConfiguration, ConfigurationLoadError>,
}

impl ConfigurationLoader for FakeConfigurationLoader {
    fn load(
        &self,
        _invocation: &ConfigurationInvocation,
    ) -> Result<ResolvedConfiguration, ConfigurationLoadError> {
        self.calls.set(self.calls.get() + 1);
        self.outcome.clone()
    }
}

fn default_configuration() -> ResolvedConfiguration {
    resolve_configuration(&[]).expect("built-in configuration must resolve")
}

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let physical_temporary_directory = std::fs::canonicalize(std::env::temp_dir())
            .expect("canonicalize temporary directory for no-follow fixture");
        let path = physical_temporary_directory.join(format!(
            "repowitness-cli-config-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).expect("create temporary directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn explain_emits_complete_path_free_default_configuration() {
    let loader = FakeConfigurationLoader {
        calls: Cell::new(0),
        outcome: Ok(default_configuration()),
    };
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run_config(
        [OsString::from("explain")].into_iter(),
        &mut stdout,
        &mut stderr,
        &loader,
    );

    assert_eq!(code, EXIT_SUCCESS);
    assert_eq!(loader.calls.get(), 1);
    assert!(stderr.is_empty());
    let output = String::from_utf8(stdout).expect("UTF-8 report");
    assert!(output.contains("operation=config_explain\n"));
    assert!(output.contains("schema_version=1\n"));
    assert!(output.contains("resolver_version=1\n"));
    assert!(output.contains("profile=local\n"));
    assert!(output.contains("configuration_digest_sha256="));
    assert!(output.contains("preference_query_results_effective="));
    assert!(output.contains("policy_allowed_language_0=rust\n"));
    assert!(output.contains("preference_mcp_tool_profile_authorized=canonical\n"));
    assert!(output.contains("policy_retained_generations_per_source_slot=2\n"));
    assert!(output.contains(
        "policy_retained_generations_per_source_slot_constrained_by_0=built_in_defaults\n"
    ));
    assert!(output.contains("policy_max_retention_generation_candidates=64\n"));
    assert!(output.contains("policy_max_retention_rows=1000000\n"));
    assert!(output.contains("policy_max_retention_bytes=536870912\n"));
    assert!(!output.contains('/'));
    assert!(!output.contains('\\'));
}

#[test]
fn doctor_warns_for_empty_language_set_and_rejects_unavailable_tool_profile() {
    let empty_languages = std::collections::BTreeSet::new();
    let mut allowed_profiles = std::collections::BTreeSet::new();
    allowed_profiles.insert(McpToolProfile::Canonical);
    let layer = ConfigurationLayer::try_new(
        ConfigurationLayerKind::Repository,
        None,
        ConfigurationPreferenceOverrides::try_new(
            None,
            None,
            None,
            None,
            None,
            Some(McpToolProfile::Minimal),
        )
        .expect("preference"),
        ConfigurationPolicyOverrides::try_new(
            Some(empty_languages),
            Some(allowed_profiles),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .expect("policy"),
    )
    .expect("layer");
    let loader = FakeConfigurationLoader {
        calls: Cell::new(0),
        outcome: Ok(resolve_configuration(&[layer]).expect("resolved")),
    };
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run_doctor(std::iter::empty(), &mut stdout, &mut stderr, &loader);

    assert_eq!(code, EXIT_SOFTWARE);
    assert!(stderr.is_empty());
    let output = String::from_utf8(stdout).expect("UTF-8 report");
    assert!(output.starts_with("operation=doctor\nstatus=error\nschema_version=1\n"));
    assert!(output.contains("check_language_adapters=warning\n"));
    assert!(output.contains("check_mcp_tool_profile=error\n"));
    assert!(output.contains("error_count=1\n"));
    assert!(output.contains("warning_count=2\n"));
    assert!(output.contains("warning_0=no_language_adapters_enabled\n"));
    assert!(output.contains("warning_1=target_checks_not_requested\n"));
    let digest = output
        .lines()
        .find_map(|line| line.strip_prefix("configuration_digest_sha256="))
        .expect("configuration digest");
    assert_eq!(digest.len(), 64);
    assert!(digest.bytes().all(|byte| byte.is_ascii_hexdigit()));
}

#[test]
fn doctor_failure_is_structured_and_does_not_echo_loader_details() {
    let loader = FakeConfigurationLoader {
        calls: Cell::new(0),
        outcome: Err(ConfigurationLoadError::Unavailable),
    };
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run_doctor(
        [
            OsString::from("--repository-config"),
            OsString::from("../private/repowitness.toml"),
        ]
        .into_iter(),
        &mut stdout,
        &mut stderr,
        &loader,
    );

    assert_eq!(code, EXIT_SOFTWARE);
    assert!(stderr.is_empty());
    assert_eq!(loader.calls.get(), 1);
    let output = String::from_utf8(stdout).expect("UTF-8 report");
    assert!(output.contains("check_configuration=error\n"));
    assert!(!output.contains("private"));
    assert!(!output.contains("repowitness.toml"));
}

#[test]
fn parsers_reject_missing_unknown_duplicate_odd_empty_and_excess_arguments() {
    for arguments in [
        vec![],
        vec!["unknown"],
        vec!["explain", "--unknown", "value"],
        vec!["explain", "--user-config"],
        vec!["explain", "--user-config", ""],
        vec!["explain", "--user-config", "one", "--user-config", "two"],
        vec![
            "explain",
            "--user-config",
            "one",
            "--workspace-config",
            "two",
            "--repository-config",
            "three",
            "--unknown",
        ],
    ] {
        let loader = FakeConfigurationLoader {
            calls: Cell::new(0),
            outcome: Ok(default_configuration()),
        };
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let code = run_config(
            arguments.into_iter().map(OsString::from),
            &mut stdout,
            &mut stderr,
            &loader,
        );
        assert_eq!(code, EXIT_USAGE);
        assert_eq!(loader.calls.get(), 0);
        assert!(stdout.is_empty());
    }
}

#[test]
fn local_loader_bounds_reads_and_resolves_explicit_layers() {
    let directory = TempDirectory::new();
    let user = directory.path().join("user.toml");
    std::fs::write(
        &user,
        b"schema_version = 1\n[preferences]\nquery_results = 7\n",
    )
    .expect("write configuration");
    let invocation = ConfigurationInvocation {
        user: Some(user),
        workspace: None,
        repository: None,
    };

    let configuration = LocalConfigurationLoader
        .load(&invocation)
        .expect("configuration");
    assert_eq!(*configuration.preferences().query_results().effective(), 7);

    let oversized = directory.path().join("oversized.toml");
    std::fs::write(&oversized, vec![b'x'; MAX_CONFIGURATION_FILE_BYTES + 1])
        .expect("write oversized configuration");
    let invocation = ConfigurationInvocation {
        user: Some(oversized.clone()),
        workspace: None,
        repository: None,
    };
    assert_eq!(
        LocalConfigurationLoader.load(&invocation),
        Err(ConfigurationLoadError::Invalid)
    );

    let indexer = FakeIndexer::failure("must not be called");
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run_index(
        [
            OsString::from("--repository-id"),
            OsString::from("repository-id"),
            OsString::from("--database"),
            OsString::from("../index.db"),
            OsString::from("--user-config"),
            oversized.into_os_string(),
            OsString::from("../repository"),
        ]
        .into_iter(),
        &mut stdout,
        &mut stderr,
        &indexer,
        &LocalConfigurationLoader,
    );
    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(indexer.calls.get(), 0);
    assert!(stdout.is_empty());
    assert_eq!(stderr, b"error: configuration resolution failed\n");
}

#[test]
fn mcp_loads_shared_user_config_and_explicit_user_config_wins() {
    let directory = TempDirectory::new();
    let shared = directory.path().join("config.toml");
    std::fs::write(
        &shared,
        b"schema_version = 1\n[preferences]\nquery_results = 7\n",
    )
    .expect("write shared configuration");

    let configuration = LocalConfigurationLoader
        .load_mcp_with_default_path(&ConfigurationInvocation::default(), Some(&shared))
        .expect("shared configuration");
    assert_eq!(*configuration.preferences().query_results().effective(), 7);

    let explicit = directory.path().join("explicit.toml");
    std::fs::write(
        &explicit,
        b"schema_version = 1\n[preferences]\nquery_results = 9\n",
    )
    .expect("write explicit configuration");
    let configuration = LocalConfigurationLoader
        .load_mcp_with_default_path(
            &ConfigurationInvocation {
                user: Some(explicit),
                workspace: None,
                repository: None,
            },
            Some(&shared),
        )
        .expect("explicit configuration");
    assert_eq!(*configuration.preferences().query_results().effective(), 9);
}

#[test]
fn default_mcp_user_configuration_path_uses_shared_state_root() {
    assert_eq!(
        default_user_configuration_path(Path::new("/state")),
        Path::new("/state/repowitness/config.toml")
    );
}

#[cfg(unix)]
#[test]
fn local_loader_rejects_a_fifo_without_waiting_for_a_writer() {
    use std::{os::unix::fs::FileTypeExt, process::Command, sync::mpsc, time::Duration};

    let directory = TempDirectory::new();
    let fifo = directory.path().join("configuration.fifo");
    let status = Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .expect("mkfifo should start");
    assert!(status.success());
    assert!(
        std::fs::symlink_metadata(&fifo)
            .expect("FIFO metadata")
            .file_type()
            .is_fifo()
    );

    let (sender, receiver) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        let result = LocalConfigurationLoader.load(&ConfigurationInvocation {
            user: Some(fifo),
            workspace: None,
            repository: None,
        });
        sender.send(result).expect("test receiver should remain");
    });
    assert_eq!(
        receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("configuration FIFO must not block"),
        Err(ConfigurationLoadError::Unavailable)
    );
    handle.join().expect("configuration loader thread");
}

#[cfg(unix)]
#[test]
fn local_loader_rejects_configuration_symlinks() {
    use std::os::unix::fs::symlink;

    let directory = TempDirectory::new();
    let target = directory.path().join("target.toml");
    let alias = directory.path().join("alias.toml");
    std::fs::write(&target, b"schema_version = 1\n").expect("write target");
    symlink(&target, &alias).expect("create symlink");

    assert_eq!(
        LocalConfigurationLoader.load(&ConfigurationInvocation {
            user: Some(alias),
            workspace: None,
            repository: None,
        }),
        Err(ConfigurationLoadError::Unavailable)
    );
}

#[test]
fn local_loader_uses_fixed_layer_order_and_repository_policy_cannot_grant_authority() {
    let directory = TempDirectory::new();
    let user = directory.path().join("user.toml");
    let workspace = directory.path().join("workspace.toml");
    let repository = directory.path().join("repository.toml");
    std::fs::write(
        &user,
        b"schema_version = 1\n[preferences]\nquery_results = 9\n[policy]\ndeny_memory_writes = true\nallowed_mcp_tool_profiles = []\nretained_generations_per_source_slot = 4\nmax_retention_generation_candidates = 48\n",
    )
    .expect("write user configuration");
    std::fs::write(
        &workspace,
        b"schema_version = 1\n[preferences]\nquery_results = 7\n",
    )
    .expect("write workspace configuration");
    std::fs::write(
        &repository,
        b"schema_version = 1\n[preferences]\nquery_results = 5\n[policy]\ndeny_memory_writes = false\nallowed_mcp_tool_profiles = [\"canonical\"]\nretained_generations_per_source_slot = 3\nmax_retention_generation_candidates = 56\n",
    )
    .expect("write repository configuration");

    let configuration = LocalConfigurationLoader
        .load(&ConfigurationInvocation {
            user: Some(user),
            workspace: Some(workspace),
            repository: Some(repository),
        })
        .expect("configuration");

    assert_eq!(*configuration.preferences().query_results().effective(), 5);
    assert!(*configuration.policy().deny_memory_writes().effective());
    assert!(
        configuration
            .policy()
            .allowed_mcp_tool_profiles()
            .effective()
            .is_empty()
    );
    assert_eq!(
        configuration.preferences().mcp_tool_profile().authorized(),
        None
    );
    let retention = configuration.policy().retention();
    assert_eq!(
        *retention.retained_generations_per_source_slot().effective(),
        4
    );
    assert_eq!(*retention.max_generation_candidates().effective(), 48);
    assert_eq!(
        retention
            .retained_generations_per_source_slot()
            .constraining_layers(),
        &[
            ConfigurationLayerKind::BuiltInDefaults,
            ConfigurationLayerKind::User
        ]
    );
}

#[test]
fn invocation_debug_redacts_every_configuration_path() {
    let invocation = ConfigurationInvocation {
        user: Some(PathBuf::from("../sensitive-user.toml")),
        workspace: Some(PathBuf::from("../sensitive-workspace.toml")),
        repository: Some(PathBuf::from("../sensitive-repository.toml")),
    };

    let debug = format!("{invocation:?}");
    assert_eq!(debug.matches("<redacted-path>").count(), 3);
    assert!(!debug.contains("sensitive"));
    assert!(!debug.contains("toml"));
}
