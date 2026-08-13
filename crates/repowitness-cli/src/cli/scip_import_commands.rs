#[derive(Clone)]
struct ScipImportInvocation {
    database: PathBuf,
    root: PathBuf,
    scip_file: PathBuf,
    connected_workspace: String,
    source_slot: String,
    workspace_view: Option<i64>,
    timeout: std::time::Duration,
}
enum ScipImportOverlayError {
    InvalidWorkspaceView,
    Import(repowitness_local::LocalScipOverlayImportError),
}

fn run_scip_import(
    args: impl Iterator<Item = OsString>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
) -> u8 {
    let arguments: Vec<OsString> = args.take(MAX_SCIP_IMPORT_ARGUMENTS + 1).collect();
    if arguments.len() > MAX_SCIP_IMPORT_ARGUMENTS {
        return emit_error(
            stderr,
            EXIT_USAGE,
            "error: scip-import received too many arguments; use scip-import --help\n",
        );
    }
    if matches!(arguments.as_slice(), [help] if help == OsStr::new("--help") || help == OsStr::new("-h")) {
        return emit_output(stdout, SCIP_IMPORT_HELP);
    }
    let invocation = match parse_scip_import_arguments(&arguments) {
        Ok(invocation) => invocation,
        Err(message) => return emit_error(stderr, EXIT_USAGE, message),
    };
    match import_scip_overlay(&invocation) {
        Ok(result) => emit_scip_import_output(stdout, result),
        Err(ScipImportOverlayError::InvalidWorkspaceView) => emit_error(
            stderr,
            EXIT_USAGE,
            "error: scip-import workspace view is invalid\n",
        ),
        Err(ScipImportOverlayError::Import(error)) => emit_scip_import_failure(stderr, &error),
    }
}

fn import_scip_overlay(
    invocation: &ScipImportInvocation,
) -> Result<repowitness_local::LocalScipOverlayImportResult, ScipImportOverlayError> {
    let mut request = repowitness_local::LocalScipOverlayImportRequest::new(
        &invocation.database,
        &invocation.root,
        &invocation.scip_file,
        &invocation.connected_workspace,
        &invocation.source_slot,
    )
    .with_deadline(invocation.timeout);
    if let Some(workspace_view) = invocation.workspace_view {
        request = match request.with_exact_view(workspace_view) {
            Ok(request) => request,
            Err(_) => {
                return Err(ScipImportOverlayError::InvalidWorkspaceView);
            }
        };
    }
    repowitness_local::import_local_scip_overlay(request, Arc::new(AtomicBool::new(false)))
        .map_err(ScipImportOverlayError::Import)
}

fn emit_scip_import_failure(
    stderr: &mut impl Write,
    error: &repowitness_local::LocalScipOverlayImportError,
) -> u8 {
    emit_error(
        stderr,
        EXIT_SOFTWARE,
        &format!(
            "error: SCIP import failed (category={})\n",
            error.category().as_str()
        ),
    )
}

fn parse_scip_import_arguments(
    arguments: &[OsString],
) -> Result<ScipImportInvocation, &'static str> {
    let mut database = None;
    let mut root = None;
    let mut scip_file = None;
    let mut connected_workspace = None;
    let mut source_slot = None;
    let mut workspace_view = None;
    let mut timeout_ms = None;
    let mut index = 0_usize;
    while index < arguments.len() {
        let option = &arguments[index];
        let value = arguments
            .get(index + 1)
            .ok_or("error: scip-import option requires a value; use scip-import --help\n")?;
        index += 2;
        if option == OsStr::new("--database") {
            if value.is_empty() || database.replace(PathBuf::from(value)).is_some() {
                return Err("error: scip-import accepts one non-empty --database\n");
            }
            continue;
        }
        if option == OsStr::new("--root") {
            if value.is_empty() || root.replace(PathBuf::from(value)).is_some() {
                return Err("error: scip-import accepts one non-empty --root\n");
            }
            continue;
        }
        if option == OsStr::new("--scip-file") {
            if value.is_empty() || scip_file.replace(PathBuf::from(value)).is_some() {
                return Err("error: scip-import accepts one non-empty --scip-file\n");
            }
            continue;
        }
        if option == OsStr::new("--connected-workspace-id") {
            let value = value
                .to_str()
                .ok_or("error: scip-import workspace identity must be Unicode\n")?;
            ConnectedWorkspaceIdTextV1::decode(value).map_err(|_| {
                "error: scip-import workspace identity must be canonical cwi1:h: text\n"
            })?;
            if connected_workspace.replace(value.to_owned()).is_some() {
                return Err("error: scip-import accepts --connected-workspace-id only once\n");
            }
            continue;
        }
        if option == OsStr::new("--source-slot-id") {
            let value = value
                .to_str()
                .ok_or("error: scip-import source slot identity must be Unicode\n")?;
            SourceSlotIdTextV1::decode(value).map_err(|_| {
                "error: scip-import source slot identity must be canonical ssi1:h: text\n"
            })?;
            if source_slot.replace(value.to_owned()).is_some() {
                return Err("error: scip-import accepts --source-slot-id only once\n");
            }
            continue;
        }
        if option == OsStr::new("--workspace-view") {
            let value = i64::try_from(parse_graph_u64(value)?)
                .map_err(|_| "error: scip-import workspace view is too large\n")?;
            if value <= 0 || workspace_view.replace(value).is_some() {
                return Err("error: scip-import accepts one positive --workspace-view\n");
            }
            continue;
        }
        if option == OsStr::new("--timeout-ms") {
            let value = parse_graph_u64(value)?;
            let maximum = u64::try_from(
                repowitness_local::MAX_LOCAL_SCIP_IMPORT_DEADLINE.as_millis(),
            )
            .map_err(|_| "error: scip-import timeout bound is unavailable\n")?;
            if !(1..=maximum).contains(&value) || timeout_ms.replace(value).is_some() {
                return Err("error: scip-import timeout must be 1 through 300000 milliseconds\n");
            }
            continue;
        }
        return Err("error: unsupported scip-import option; use scip-import --help\n");
    }
    let timeout_ms = timeout_ms.unwrap_or_else(|| {
        u64::try_from(repowitness_local::DEFAULT_LOCAL_SCIP_IMPORT_DEADLINE.as_millis())
            .expect("the local SCIP import default deadline fits in u64 milliseconds")
    });
    Ok(ScipImportInvocation {
        database: database.ok_or("error: scip-import requires --database\n")?,
        root: root.ok_or("error: scip-import requires --root\n")?,
        scip_file: scip_file.ok_or("error: scip-import requires --scip-file\n")?,
        connected_workspace: connected_workspace
            .ok_or("error: scip-import requires --connected-workspace-id\n")?,
        source_slot: source_slot.ok_or("error: scip-import requires --source-slot-id\n")?,
        workspace_view,
        timeout: std::time::Duration::from_millis(timeout_ms),
    })
}

fn emit_scip_import_output(
    stdout: &mut impl Write,
    result: repowitness_local::LocalScipOverlayImportResult,
) -> u8 {
    let overlay = result.overlay();
    let output = serde_json::json!({
        "schema_version": 1_u8,
        "connected_workspace": ConnectedWorkspaceIdTextV1::encode(result.connected_workspace()).into_string(),
        "workspace_view": result.workspace_view(),
        "source_slot": SourceSlotIdTextV1::encode(result.source_slot()).into_string(),
        "overlay_sha256": hex(overlay.digest().as_bytes()),
        "documents": overlay.documents(),
        "ignored_external_documents": result.ignored_external_documents(),
        "occurrences": overlay.occurrences(),
        "relationships": overlay.relationships(),
    });
    let Ok(mut encoded) = serde_json::to_vec(&output) else {
        return EXIT_SOFTWARE;
    };
    encoded.push(b'\n');
    if stdout.write_all(&encoded).is_ok() {
        EXIT_SUCCESS
    } else {
        EXIT_IO
    }
}

#[cfg(test)]
mod scip_import_command_tests {
    use super::*;

    #[test]
    fn import_failure_emits_one_redacted_category() {
        let error = repowitness_local::LocalScipOverlayImportError::InvalidSelection;
        let mut stderr = Vec::new();

        assert_eq!(emit_scip_import_failure(&mut stderr, &error), EXIT_SOFTWARE);
        assert_eq!(
            String::from_utf8(stderr).expect("diagnostic should be UTF-8"),
            "error: SCIP import failed (category=invalid_selection)\n"
        );
    }
}
