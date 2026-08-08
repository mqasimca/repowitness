use std::cell::{Cell, RefCell};

use repowitness_local::{ConfigurationPolicyOverrides, ConfigurationPreferenceOverrides};

use super::*;

struct StubMcpConfigurationLoader {
    calls: Cell<u64>,
    invocation: RefCell<Option<ConfigurationInvocation>>,
    outcome: Result<ResolvedConfiguration, ConfigurationLoadError>,
}

impl ConfigurationLoader for StubMcpConfigurationLoader {
    fn load(
        &self,
        invocation: &ConfigurationInvocation,
    ) -> Result<ResolvedConfiguration, ConfigurationLoadError> {
        self.calls.set(self.calls.get() + 1);
        self.invocation.replace(Some(invocation.clone()));
        self.outcome.clone()
    }
}

struct RecordingMcpLauncher {
    calls: Cell<u64>,
    memory_writes_enabled: Cell<Option<bool>>,
    native_tasks_enabled: Cell<Option<bool>>,
    surface: Cell<Option<McpToolSurface>>,
    configuration: RefCell<Option<ResolvedConfiguration>>,
}

impl RecordingMcpLauncher {
    fn new() -> Self {
        Self {
            calls: Cell::new(0),
            memory_writes_enabled: Cell::new(None),
            native_tasks_enabled: Cell::new(None),
            surface: Cell::new(None),
            configuration: RefCell::new(None),
        }
    }
}

impl McpServerLauncher for RecordingMcpLauncher {
    fn launch(
        &self,
        invocation: McpServeInvocation,
        configuration: ResolvedConfiguration,
        surface: McpToolSurface,
    ) -> Result<(), McpLaunchError> {
        self.calls.set(self.calls.get() + 1);
        self.memory_writes_enabled
            .set(Some(invocation.memory_writes_enabled));
        self.native_tasks_enabled
            .set(Some(invocation.native_tasks_enabled));
        self.surface.set(Some(surface));
        self.configuration.replace(Some(configuration));
        Ok(())
    }
}

fn mcp_arguments(memory_writes: bool) -> Vec<OsString> {
    let mut arguments = vec![
        OsString::from("repowitness"),
        OsString::from("mcp-serve"),
        OsString::from("--root"),
        OsString::from("../repository"),
        OsString::from("--repository-id"),
        OsString::from(format!("rwi1:h:{}", "AB".repeat(32))),
        OsString::from("--database"),
        OsString::from("../index.db"),
    ];
    if memory_writes {
        arguments.extend([
            OsString::from("--enable-memory-writes"),
            OsString::from("--memory-actor"),
            OsString::from("trusted-local-actor"),
        ]);
    }
    arguments
}

#[test]
fn mcp_serve_arguments_are_complete_canonical_and_order_independent() {
    let identity = format!("rwi1:h:{}", "AB".repeat(32));
    let arguments = [
        OsString::from("--root"),
        OsString::from("../repository"),
        OsString::from("--repository-id"),
        OsString::from(&identity),
        OsString::from("--database"),
        OsString::from("../index.db"),
    ];
    let invocation = parse_mcp_serve_arguments(&arguments).expect("valid configuration");
    assert!(matches!(
        invocation.target,
        McpServeTarget::Single {
            root,
            database,
            repository_identity,
            ..
        } if root == Path::new("../repository")
            && database == Path::new("../index.db")
            && repository_identity == identity
    ));
    assert!(!invocation.memory_writes_enabled);
    assert!(!invocation.native_tasks_enabled);
    assert_eq!(invocation.memory_actor, None);
}

#[test]
fn mcp_registry_startup_is_exclusive_and_read_only() {
    let registry = PathBuf::from("/absolute/registry.json");
    let invocation = parse_mcp_serve_arguments(&[
        OsString::from("--registry"),
        registry.clone().into_os_string(),
    ])
    .expect("registry startup is valid");
    assert!(matches!(
        invocation.target,
        McpServeTarget::Registry { path } if path == registry
    ));
    assert!(!invocation.memory_writes_enabled);
    assert!(!invocation.native_tasks_enabled);
    assert!(invocation.personal_memory_profile.is_none());

    let identity = format!("rwi1:h:{}", "AB".repeat(32));
    for arguments in [
        vec!["--registry", "/registry.json", "--root", "/repository"],
        vec![
            "--registry",
            "/registry.json",
            "--repository-id",
            identity.as_str(),
        ],
        vec!["--registry", "/registry.json", "--enable-memory-writes"],
        vec!["--registry", "/registry.json", "--enable-native-tasks"],
        vec!["--registry", "/registry.json", "--enable-personal-memory"],
    ] {
        assert!(
            parse_mcp_serve_arguments(
                &arguments
                    .into_iter()
                    .map(OsString::from)
                    .collect::<Vec<_>>(),
            )
            .is_err()
        );
    }
}

#[test]
fn mcp_catalog_startup_is_exclusive_and_read_only() {
    let state_dir = PathBuf::from("/absolute/private-state");
    let invocation = parse_mcp_serve_arguments(&[
        OsString::from("--catalog"),
        OsString::from("--catalog-state-dir"),
        state_dir.clone().into_os_string(),
    ])
    .expect("catalog startup is valid");
    assert!(matches!(
        invocation.target,
        McpServeTarget::Catalog { state_dir: Some(path) } if path == state_dir
    ));
    assert!(!invocation.memory_writes_enabled);
    assert!(!invocation.native_tasks_enabled);
    assert!(invocation.personal_memory_profile.is_none());

    for arguments in [
        vec!["--catalog", "--registry", "/registry.json"],
        vec!["--catalog", "--root", "/repository"],
        vec!["--catalog", "--enable-memory-writes"],
        vec!["--catalog", "--enable-native-tasks"],
        vec!["--catalog", "--enable-personal-memory"],
        vec!["--catalog-state-dir", "/private-state"],
    ] {
        assert!(
            parse_mcp_serve_arguments(
                &arguments
                    .into_iter()
                    .map(OsString::from)
                    .collect::<Vec<_>>(),
            )
            .is_err()
        );
    }
}

#[test]
fn mcp_registry_reader_is_strict_bounded_and_path_free() {
    let unique = format!(
        "repowitness-mcp-registry-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time after epoch")
            .as_nanos(),
    );
    let directory = std::fs::canonicalize(std::env::temp_dir())
        .expect("canonicalize temporary directory")
        .join(unique);
    std::fs::create_dir(&directory).expect("temporary registry directory");
    let registry = directory.join("registry.json");
    let root = directory.join("repository");
    let database = directory.join("index.sqlite3");
    let first_id = format!("rwi1:h:{}", "AB".repeat(32));
    let second_id = format!("rwi1:h:{}", "CD".repeat(32));
    let document = serde_json::json!({
        "schema_version": 1,
        "repositories": [
            {"repository_id": first_id, "root": root, "database": database},
            {"repository_id": second_id, "root": directory.join("repository-two"), "database": directory.join("index-two.sqlite3")}
        ]
    });
    std::fs::write(
        &registry,
        serde_json::to_vec(&document).expect("registry JSON"),
    )
    .expect("write registry");
    let parsed = read_mcp_repository_registry(&registry).expect("registry parses");
    assert_eq!(parsed.len(), 2);
    assert!(parsed.iter().all(|entry| entry.root.is_absolute()));
    assert!(parsed.iter().all(|entry| entry.database.is_absolute()));

    for invalid in [
        br#"{"schema_version":1,"repositories":[]}"#.as_slice(),
        br#"{"schema_version":1,"repositories":[],"unknown":true}"#.as_slice(),
        br#"{"schema_version":1,"schema_version":1,"repositories":[]}"#.as_slice(),
        br#"{"schema_version":1,"repositories":[{"repository_id":"rwi1:h:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA","root":"relative","database":"/absolute/database"}]}"#.as_slice(),
    ] {
        std::fs::write(&registry, invalid).expect("rewrite invalid registry");
        assert!(read_mcp_repository_registry(&registry).is_err());
    }
    std::fs::write(&registry, vec![b' '; MAX_MCP_REPOSITORY_REGISTRY_BYTES + 1])
        .expect("write oversized registry");
    assert!(read_mcp_repository_registry(&registry).is_err());
    std::fs::remove_dir_all(&directory).expect("remove temporary registry directory");
}

#[test]
fn mcp_native_tasks_require_one_explicit_startup_opt_in() {
    let mut enabled = mcp_arguments(false);
    enabled.push(OsString::from("--enable-native-tasks"));
    let invocation = parse_mcp_serve_arguments(&enabled[2..]).expect("explicit task opt-in");
    assert!(invocation.native_tasks_enabled);

    enabled.push(OsString::from("--enable-native-tasks"));
    assert!(parse_mcp_serve_arguments(&enabled[2..]).is_err());
}

#[test]
fn mcp_personal_memory_requires_one_explicit_fixed_opaque_profile() {
    let mut enabled = mcp_arguments(false);
    enabled.extend([
        OsString::from("--enable-personal-memory"),
        OsString::from("--personal-memory-profile"),
        OsString::from("ab".repeat(16)),
    ]);
    let invocation = parse_mcp_serve_arguments(&enabled[2..]).expect("explicit profile capability");
    assert_eq!(
        invocation
            .personal_memory_profile
            .expect("profile")
            .as_bytes(),
        [0xab; 16]
    );

    let profile = "ab".repeat(16);
    let uppercase_profile = "AB".repeat(16);
    for arguments in [
        vec!["--enable-personal-memory"],
        vec!["--personal-memory-profile", &profile],
        vec![
            "--enable-personal-memory",
            "--personal-memory-profile",
            &uppercase_profile,
        ],
    ] {
        let mut incomplete = mcp_arguments(false);
        incomplete.extend(arguments.into_iter().map(OsString::from));
        assert!(parse_mcp_serve_arguments(&incomplete[2..]).is_err());
    }
}

#[test]
fn mcp_serve_admits_one_explicit_connected_graph_source_slot() {
    let expected_connected_workspace = format!("cwi1:h:{}", "CD".repeat(32));
    let expected_source_slot = format!("ssi1:h:{}", "EF".repeat(32));
    let arguments = [
        OsString::from("--root"),
        OsString::from("../repository"),
        OsString::from("--repository-id"),
        OsString::from(format!("rwi1:h:{}", "AB".repeat(32))),
        OsString::from("--database"),
        OsString::from("../index.db"),
        OsString::from("--connected-workspace-id"),
        OsString::from(&expected_connected_workspace),
        OsString::from("--source-slot-id"),
        OsString::from(&expected_source_slot),
    ];
    let invocation = parse_mcp_serve_arguments(&arguments).expect("valid graph context");
    assert!(matches!(
        invocation.target,
        McpServeTarget::Single {
            graph_workspace: GraphWorkspaceContext::ConnectedWorkspace {
            ref connected_workspace,
            ref source_slot,
            },
            ..
        } if connected_workspace == &expected_connected_workspace
            && source_slot == &expected_source_slot
    ));

    for extra in [
        vec!["--connected-workspace-id", "cwi1:h:CD"],
        vec!["--source-slot-id", "ssi1:h:EF"],
    ] {
        let mut incomplete = mcp_arguments(false);
        incomplete.extend(extra.into_iter().map(OsString::from));
        assert!(parse_mcp_serve_arguments(&incomplete).is_err());
    }
}

#[test]
fn mcp_memory_writes_require_an_explicit_valid_fixed_actor() {
    let identity = format!("rwi1:h:{}", "AB".repeat(32));
    let arguments = [
        OsString::from("--enable-memory-writes"),
        OsString::from("--memory-actor"),
        OsString::from("trusted-local-actor"),
        OsString::from("--root"),
        OsString::from("../repository"),
        OsString::from("--repository-id"),
        OsString::from(&identity),
        OsString::from("--database"),
        OsString::from("../index.db"),
    ];
    let invocation = parse_mcp_serve_arguments(&arguments).expect("valid mutation capability");
    assert!(invocation.memory_writes_enabled);
    assert_eq!(
        invocation.memory_actor.as_deref(),
        Some("trusted-local-actor")
    );

    for extra in [
        vec!["--enable-memory-writes"],
        vec!["--memory-actor", "actor"],
        vec!["--enable-memory-writes", "--memory-actor", ""],
    ] {
        let mut arguments = vec![
            "--root",
            "repository",
            "--database",
            "index.db",
            "--repository-id",
            &identity,
        ];
        arguments.extend(extra);
        assert!(
            parse_mcp_serve_arguments(
                &arguments
                    .into_iter()
                    .map(OsString::from)
                    .collect::<Vec<_>>(),
            )
            .is_err()
        );
    }
}

#[test]
fn mcp_serve_rejects_invalid_configuration_without_starting_a_runtime() {
    let valid_identity = format!("rwi1:h:{}", "AB".repeat(32));
    for arguments in [
        vec![],
        vec!["--root", "private"],
        vec![
            "--root",
            "private",
            "--root",
            "other",
            "--database",
            "index.db",
        ],
        vec![
            "--root",
            "private",
            "--database",
            "index.db",
            "--repository-id",
            "invalid",
        ],
        vec![
            "--root",
            "private",
            "--database",
            "index.db",
            "--unknown",
            &valid_identity,
        ],
    ] {
        let arguments = arguments
            .into_iter()
            .map(OsString::from)
            .collect::<Vec<_>>();
        assert!(parse_mcp_serve_arguments(&arguments).is_err());
    }
}

#[test]
fn mcp_serve_help_uses_only_the_diagnostic_stream() {
    let mut stderr = Vec::new();
    let code = run_mcp_server(
        [
            OsString::from("repowitness"),
            OsString::from("mcp-serve"),
            OsString::from("--help"),
        ],
        &mut stderr,
    );
    assert_eq!(code, EXIT_SUCCESS);
    let help = String::from_utf8(stderr).expect("help is UTF-8");
    assert!(help.contains("Stdout is reserved exclusively"));
    assert!(help.contains("memory_manage is available only when both mutation options"));
    assert!(help.contains("--enable-native-tasks"));
}

#[test]
fn mcp_serve_rejects_excess_arguments_before_runtime_startup() {
    let mut arguments = vec![OsString::from("repowitness"), OsString::from("mcp-serve")];
    arguments.extend((0..=MAX_MCP_SERVE_ARGUMENTS).map(|_| OsString::from("--unexpected")));
    let mut stderr = Vec::new();
    assert_eq!(run_mcp_server(arguments, &mut stderr), EXIT_USAGE);
    assert_eq!(stderr, b"error: mcp-serve received too many arguments\n");
}

#[test]
fn mcp_resolves_explicit_layers_before_launching_the_runtime() {
    let configuration = resolve_configuration(&[]).expect("default configuration");
    let loader = StubMcpConfigurationLoader {
        calls: Cell::new(0),
        invocation: RefCell::new(None),
        outcome: Ok(configuration.clone()),
    };
    let launcher = RecordingMcpLauncher::new();
    let mut arguments = mcp_arguments(false);
    arguments.extend([
        OsString::from("--repository-config"),
        OsString::from("../repository.toml"),
        OsString::from("--user-config"),
        OsString::from("../user.toml"),
        OsString::from("--workspace-config"),
        OsString::from("../workspace.toml"),
    ]);
    let mut stderr = Vec::new();

    let code = run_mcp_server_with_adapters(arguments, &mut stderr, &loader, &launcher);

    assert_eq!(code, EXIT_SUCCESS);
    assert!(stderr.is_empty());
    assert_eq!(loader.calls.get(), 1);
    assert_eq!(launcher.calls.get(), 1);
    assert_eq!(launcher.memory_writes_enabled.get(), Some(false));
    assert_eq!(launcher.native_tasks_enabled.get(), Some(false));
    assert_eq!(launcher.surface.get(), Some(McpToolSurface::NativeV1));
    assert_eq!(
        launcher
            .configuration
            .borrow()
            .as_ref()
            .map(ResolvedConfiguration::digest),
        Some(configuration.digest())
    );
    let invocation = loader.invocation.borrow();
    let invocation = invocation.as_ref().expect("configuration invocation");
    assert_eq!(invocation.user.as_deref(), Some(Path::new("../user.toml")));
    assert_eq!(
        invocation.workspace.as_deref(),
        Some(Path::new("../workspace.toml"))
    );
    assert_eq!(
        invocation.repository.as_deref(),
        Some(Path::new("../repository.toml"))
    );
}

#[test]
fn mcp_rejects_lower_trust_profile_escalation_and_write_denials_before_runtime() {
    for layer_kind in [
        ConfigurationLayerKind::Workspace,
        ConfigurationLayerKind::Repository,
    ] {
        let unavailable_profile = ConfigurationLayer::try_new(
            layer_kind,
            None,
            ConfigurationPreferenceOverrides::try_new(
                None,
                None,
                None,
                None,
                None,
                Some(McpToolProfile::IncumbentCompatible),
            )
            .expect("profile preference"),
            ConfigurationPolicyOverrides::default(),
        )
        .expect("lower-trust layer");
        let unauthorized =
            resolve_configuration(&[unavailable_profile]).expect("resolved unavailable profile");
        let loader = StubMcpConfigurationLoader {
            calls: Cell::new(0),
            invocation: RefCell::new(None),
            outcome: Ok(unauthorized),
        };
        let launcher = RecordingMcpLauncher::new();
        let mut stderr = Vec::new();
        let code =
            run_mcp_server_with_adapters(mcp_arguments(false), &mut stderr, &loader, &launcher);
        assert_eq!(code, EXIT_SOFTWARE);
        assert_eq!(launcher.calls.get(), 0);
        assert_eq!(
            stderr,
            b"error: configured MCP tool profile is unavailable\n"
        );
    }

    let user_deny = ConfigurationLayer::try_new(
        ConfigurationLayerKind::User,
        None,
        ConfigurationPreferenceOverrides::default(),
        ConfigurationPolicyOverrides::try_new(
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(true),
            None,
        )
        .expect("user policy"),
    )
    .expect("user layer");
    let repository_grant_attempt = ConfigurationLayer::try_new(
        ConfigurationLayerKind::Repository,
        None,
        ConfigurationPreferenceOverrides::default(),
        ConfigurationPolicyOverrides::try_new(
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(false),
            None,
        )
        .expect("repository policy"),
    )
    .expect("repository layer");
    let denied = resolve_configuration(&[user_deny, repository_grant_attempt])
        .expect("resolved monotonic denial");
    assert!(*denied.policy().deny_memory_writes().effective());
    let loader = StubMcpConfigurationLoader {
        calls: Cell::new(0),
        invocation: RefCell::new(None),
        outcome: Ok(denied),
    };
    let launcher = RecordingMcpLauncher::new();
    let mut stderr = Vec::new();
    let code = run_mcp_server_with_adapters(mcp_arguments(true), &mut stderr, &loader, &launcher);
    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(launcher.calls.get(), 0);
    assert_eq!(
        stderr,
        b"error: MCP memory writes are denied by configuration\n"
    );
}

#[test]
fn mcp_maps_user_authorized_compatibility_profile_to_the_fixed_alias_surface() {
    let user_profile = ConfigurationLayer::try_new(
        ConfigurationLayerKind::User,
        None,
        ConfigurationPreferenceOverrides::try_new(
            None,
            None,
            None,
            None,
            None,
            Some(McpToolProfile::IncumbentCompatible),
        )
        .expect("profile preference"),
        ConfigurationPolicyOverrides::default(),
    )
    .expect("user layer");
    let configuration =
        resolve_configuration(&[user_profile]).expect("authorized compatibility profile");
    let loader = StubMcpConfigurationLoader {
        calls: Cell::new(0),
        invocation: RefCell::new(None),
        outcome: Ok(configuration),
    };
    let launcher = RecordingMcpLauncher::new();
    let mut stderr = Vec::new();

    let code = run_mcp_server_with_adapters(mcp_arguments(false), &mut stderr, &loader, &launcher);

    assert_eq!(code, EXIT_SUCCESS);
    assert!(stderr.is_empty());
    assert_eq!(launcher.calls.get(), 1);
    assert_eq!(
        launcher.surface.get(),
        Some(McpToolSurface::NativeV1PlusIncumbentSubsetV1)
    );
}

#[test]
fn mcp_configuration_failure_is_redacted_and_precedes_runtime_initialization() {
    let loader = StubMcpConfigurationLoader {
        calls: Cell::new(0),
        invocation: RefCell::new(None),
        outcome: Err(ConfigurationLoadError::Invalid),
    };
    let launcher = RecordingMcpLauncher::new();
    let mut arguments = mcp_arguments(false);
    arguments.extend([
        OsString::from("--user-config"),
        OsString::from("../private-secret-name.toml"),
    ]);
    let mut stderr = Vec::new();
    let code = run_mcp_server_with_adapters(arguments, &mut stderr, &loader, &launcher);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(loader.calls.get(), 1);
    assert_eq!(launcher.calls.get(), 0);
    assert_eq!(stderr, b"error: configuration resolution failed\n");
}

#[cfg(unix)]
#[test]
fn mcp_serve_preserves_non_utf8_paths_but_rejects_non_utf8_trust_text() {
    use std::os::unix::ffi::{OsStrExt, OsStringExt};

    let identity = format!("rwi1:h:{}", "AB".repeat(32));
    let root = OsString::from_vec(vec![b'r', 0xFF]);
    let database = OsString::from_vec(vec![b'd', 0xFE]);
    let arguments = [
        OsString::from("--root"),
        root.clone(),
        OsString::from("--database"),
        database.clone(),
        OsString::from("--repository-id"),
        OsString::from(&identity),
    ];
    let invocation = parse_mcp_serve_arguments(&arguments).expect("byte paths are supported");
    assert!(matches!(
        invocation.target,
        McpServeTarget::Single {
            root: parsed_root,
            database: parsed_database,
            ..
        } if parsed_root.as_os_str().as_bytes() == root.as_os_str().as_bytes()
            && parsed_database.as_os_str().as_bytes() == database.as_os_str().as_bytes()
    ));

    for option in ["--repository-id", "--memory-actor"] {
        let mut arguments = vec![
            OsString::from("--root"),
            OsString::from("repository"),
            OsString::from("--database"),
            OsString::from("index.db"),
            OsString::from("--repository-id"),
            OsString::from(&identity),
            OsString::from("--enable-memory-writes"),
            OsString::from("--memory-actor"),
            OsString::from("actor"),
        ];
        let position = arguments
            .iter()
            .position(|argument| argument == OsStr::new(option))
            .expect("option exists");
        arguments[position + 1] = OsString::from_vec(vec![0xFF]);
        assert!(parse_mcp_serve_arguments(&arguments).is_err());
    }
}
