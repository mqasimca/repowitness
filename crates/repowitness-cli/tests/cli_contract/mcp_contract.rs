#[test]
fn mcp_help_describes_single_and_catalog_modes() {
    let output = repowitness(&["mcp-serve", "--help"]);
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    let help = String::from_utf8(output.stderr).expect("MCP help is UTF-8");
    assert!(help.contains("one repository or a local catalog"));
    assert!(help.contains("--repository-id"));
    assert!(help.contains("--catalog"));
    assert!(!help.contains("--registry"));
}

#[test]
fn mcp_rejects_missing_repository_arguments_before_transport_startup() {
    let output = repowitness(&["mcp-serve"]);
    assert_eq!(output.status.code(), Some(64));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8(output.stderr)
        .expect("MCP diagnostic is UTF-8")
        .starts_with("error:"));
}
