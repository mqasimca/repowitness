use super::*;

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
    assert_eq!(invocation.root, Path::new("../repository"));
    assert_eq!(invocation.database, Path::new("../index.db"));
    assert_eq!(invocation.repository_identity, identity);
    assert!(!invocation.memory_writes_enabled);
    assert_eq!(invocation.memory_actor, None);
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
}

#[test]
fn mcp_serve_rejects_excess_arguments_before_runtime_startup() {
    let mut arguments = vec![OsString::from("repowitness"), OsString::from("mcp-serve")];
    arguments.extend((0..=MAX_MCP_SERVE_ARGUMENTS).map(|_| OsString::from("--unexpected")));
    let mut stderr = Vec::new();
    assert_eq!(run_mcp_server(arguments, &mut stderr), EXIT_USAGE);
    assert_eq!(stderr, b"error: mcp-serve received too many arguments\n");
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
    assert_eq!(
        invocation.root.as_os_str().as_bytes(),
        root.as_os_str().as_bytes()
    );
    assert_eq!(
        invocation.database.as_os_str().as_bytes(),
        database.as_os_str().as_bytes()
    );

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
