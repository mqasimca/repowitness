use std::path::Path;

use super::*;

fn identity() -> OsString {
    OsString::from(format!("rwi1:h:{}", "AB".repeat(32)))
}

fn arguments() -> Vec<OsString> {
    vec![
        OsString::from("--root"),
        OsString::from("../repository"),
        OsString::from("--repository-id"),
        identity(),
        OsString::from("--database"),
        OsString::from("../index.db"),
    ]
}

#[test]
fn mcp_serve_requires_one_explicit_repository() {
    let invocation = parse_mcp_serve_arguments(&arguments()).expect("valid MCP arguments");
    assert!(matches!(
        invocation.target,
        McpServeTarget::Single {
            root,
            database,
            repository_identity,
            ..
        } if root == Path::new("../repository")
            && database == Path::new("../index.db")
            && repository_identity == identity().to_string_lossy()
    ));
    assert!(!invocation.memory_writes_enabled);
    assert_eq!(invocation.memory_actor, None);
}

#[test]
fn mcp_serve_accepts_one_catalog_for_many_repositories() {
    let invocation = parse_mcp_serve_arguments(&[
        OsString::from("--catalog"),
        OsString::from("--catalog-state-dir"),
        OsString::from("/private/state"),
    ])
    .expect("valid catalog arguments");
    assert!(matches!(
        invocation.target,
        McpServeTarget::Catalog { state_dir: Some(path) } if path == Path::new("/private/state")
    ));
    assert!(!invocation.memory_writes_enabled);
    assert!(
        parse_mcp_serve_arguments(&[
            OsString::from("--catalog"),
            OsString::from("--root"),
            OsString::from("/private/repository"),
        ])
        .is_err()
    );
}

#[test]
fn mcp_serve_memory_writes_require_a_valid_actor() {
    let mut write_arguments = arguments();
    write_arguments.extend([
        OsString::from("--enable-memory-writes"),
        OsString::from("--memory-actor"),
        OsString::from("trusted-local-actor"),
    ]);
    let invocation = parse_mcp_serve_arguments(&write_arguments).expect("valid write capability");
    assert!(invocation.memory_writes_enabled);
    assert_eq!(
        invocation.memory_actor.as_deref(),
        Some("trusted-local-actor")
    );

    let mut missing_actor = arguments();
    missing_actor.push(OsString::from("--enable-memory-writes"));
    assert!(parse_mcp_serve_arguments(&missing_actor).is_err());
}

#[test]
fn mcp_serve_rejects_multi_repository_options() {
    let mut arguments = arguments();
    arguments.extend([
        OsString::from("--registry"),
        OsString::from("registry.json"),
    ]);
    assert!(parse_mcp_serve_arguments(&arguments).is_err());
}
