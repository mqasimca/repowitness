use super::*;

const REPOSITORY_ID: &str = concat!(
    "rwi1:h:",
    "ABABABABABABABABABABABABABABABABABABABABABABABABABABABABABABABAB"
);
const CONNECTED_WORKSPACE_ID: &str = concat!(
    "cwi1:h:",
    "1111111111111111111111111111111111111111111111111111111111111111"
);
const SOURCE_SLOT_ID: &str = concat!(
    "ssi1:h:",
    "2222222222222222222222222222222222222222222222222222222222222222"
);

#[test]
fn scip_evidence_parses_exact_connected_scope_and_roots_before_io() {
    let arguments = [
        "--connected-workspace-id",
        CONNECTED_WORKSPACE_ID,
        "--source-slot-id",
        SOURCE_SLOT_ID,
        "--database",
        "../index.sqlite3",
        "--symbol",
        "scip-rust pkg 1 Symbol.",
        "--package-root",
        "rwp1:h:737263",
        "--workspace-view",
        "7",
        "--timeout-ms",
        "8",
    ]
    .into_iter()
    .map(OsString::from)
    .collect::<Vec<_>>();
    let invocation = parse_scip_evidence_arguments(&arguments).expect("valid CLI request");
    assert!(matches!(
        invocation.workspace,
        GraphWorkspaceContext::ConnectedWorkspace { ref connected_workspace, ref source_slot }
            if connected_workspace == CONNECTED_WORKSPACE_ID && source_slot == SOURCE_SLOT_ID
    ));
    assert_eq!(invocation.request.workspace_view(), Some(7));
    assert_eq!(invocation.request.timeout(), Duration::from_millis(8));
    assert_eq!(
        invocation.request.symbol().as_str(),
        "scip-rust pkg 1 Symbol."
    );
    assert_eq!(invocation.request.package_scope().root_count().get(), 1);
}

#[test]
fn scip_evidence_rejects_invalid_input_and_help_has_no_io() {
    for arguments in [
        vec![
            "--repository-id",
            REPOSITORY_ID,
            "--database",
            "db",
            "--symbol",
            "",
        ],
        vec![
            "--repository-id",
            REPOSITORY_ID,
            "--database",
            "db",
            "--symbol",
            "x",
            "--workspace-view",
            "0",
        ],
        vec![
            "--repository-id",
            REPOSITORY_ID,
            "--database",
            "db",
            "--symbol",
            "x",
            "--package-root",
            "not-a-path",
        ],
        vec![
            "--repository-id",
            REPOSITORY_ID,
            "--database",
            "db",
            "--symbol",
            "x",
            "--symbol",
            "y",
        ],
    ] {
        let arguments = arguments
            .into_iter()
            .map(OsString::from)
            .collect::<Vec<_>>();
        assert!(parse_scip_evidence_arguments(&arguments).is_err());
    }
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    assert_eq!(
        run_scip_evidence(
            [OsString::from("--help")].into_iter(),
            &mut stdout,
            &mut stderr,
        ),
        EXIT_SUCCESS
    );
    assert!(stderr.is_empty());
    assert!(
        String::from_utf8(stdout)
            .expect("UTF-8")
            .contains("scip-evidence")
    );
}
