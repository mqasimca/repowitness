use super::*;

const CONNECTED_WORKSPACE_ID: &str = concat!(
    "cwi1:h:",
    "1111111111111111111111111111111111111111111111111111111111111111"
);
const SOURCE_SLOT_ID: &str = concat!(
    "ssi1:h:",
    "2222222222222222222222222222222222222222222222222222222222222222"
);

#[test]
fn scip_import_parses_explicit_contained_scope_before_io() {
    let arguments = [
        "--database",
        "../index.sqlite3",
        "--root",
        "../repository",
        "--scip-file",
        "../producer/output.scip",
        "--connected-workspace-id",
        CONNECTED_WORKSPACE_ID,
        "--source-slot-id",
        SOURCE_SLOT_ID,
        "--workspace-view",
        "7",
        "--timeout-ms",
        "8",
    ]
    .into_iter()
    .map(OsString::from)
    .collect::<Vec<_>>();
    let invocation = parse_scip_import_arguments(&arguments).expect("valid CLI request");
    assert_eq!(invocation.connected_workspace, CONNECTED_WORKSPACE_ID);
    assert_eq!(invocation.source_slot, SOURCE_SLOT_ID);
    assert_eq!(invocation.workspace_view, Some(7));
    assert_eq!(invocation.timeout, std::time::Duration::from_millis(8));

    let maximum_timeout_arguments = [
        "--database",
        "../index.sqlite3",
        "--root",
        "../repository",
        "--scip-file",
        "../producer/output.scip",
        "--connected-workspace-id",
        CONNECTED_WORKSPACE_ID,
        "--source-slot-id",
        SOURCE_SLOT_ID,
        "--timeout-ms",
        "300000",
    ]
    .into_iter()
    .map(OsString::from)
    .collect::<Vec<_>>();
    let maximum_timeout =
        parse_scip_import_arguments(&maximum_timeout_arguments).expect("maximum timeout");
    assert_eq!(
        maximum_timeout.timeout,
        std::time::Duration::from_millis(300000)
    );
}

#[test]
fn scip_import_rejects_ambiguous_or_invalid_input_and_help_has_no_io() {
    for arguments in [
        vec!["--database", "db", "--root", "root", "--scip-file", "file"],
        vec![
            "--database",
            "db",
            "--root",
            "root",
            "--scip-file",
            "file",
            "--connected-workspace-id",
            CONNECTED_WORKSPACE_ID,
            "--source-slot-id",
            SOURCE_SLOT_ID,
            "--timeout-ms",
            "0",
        ],
        vec![
            "--database",
            "db",
            "--root",
            "root",
            "--scip-file",
            "file",
            "--connected-workspace-id",
            CONNECTED_WORKSPACE_ID,
            "--source-slot-id",
            SOURCE_SLOT_ID,
            "--workspace-view",
            "0",
        ],
    ] {
        assert!(
            parse_scip_import_arguments(
                &arguments
                    .into_iter()
                    .map(OsString::from)
                    .collect::<Vec<_>>()
            )
            .is_err()
        );
    }
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    assert_eq!(
        run_scip_import(
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
            .contains("scip-import")
    );
}
