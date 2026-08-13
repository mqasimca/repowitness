#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CliGraphOperation {
    Status,
    Search,
    Evidence,
    Architecture,
    Trace,
    Impact,
}

impl CliGraphOperation {
    fn parse(value: &OsStr) -> Option<Self> {
        if value == OsStr::new("status") {
            Some(Self::Status)
        } else if value == OsStr::new("search") {
            Some(Self::Search)
        } else if value == OsStr::new("evidence") {
            Some(Self::Evidence)
        } else if value == OsStr::new("architecture") {
            Some(Self::Architecture)
        } else if value == OsStr::new("trace") {
            Some(Self::Trace)
        } else if value == OsStr::new("impact") {
            Some(Self::Impact)
        } else {
            None
        }
    }

    const fn accepts_limits(self) -> bool {
        !matches!(self, Self::Status)
    }
}

struct GraphInvocation {
    database: PathBuf,
    workspace: GraphWorkspaceContext,
    request: GraphReadServiceRequest,
}

#[derive(Default)]
struct GraphWorkspaceArguments {
    repository_identity: Option<String>,
    connected_workspace: Option<String>,
    source_slot: Option<String>,
}

impl GraphWorkspaceArguments {
    fn accept_option(&mut self, option: &OsStr, value: &OsStr) -> Result<bool, &'static str> {
        if option == OsStr::new("--repository-id") {
            let text = graph_identity_text(
                value,
                "error: graph repository identity must be non-empty Unicode\n",
            )?;
            RepositoryIdentityTextV1::decode(text)
                .map_err(|_| "error: graph repository identity must be canonical rwi1:h: text\n")?;
            if self.repository_identity.replace(text.to_owned()).is_some() {
                return Err("error: graph accepts --repository-id only once\n");
            }
            return Ok(true);
        }
        if option == OsStr::new("--connected-workspace-id") {
            let text = graph_identity_text(
                value,
                "error: graph connected-workspace identity must be non-empty Unicode\n",
            )?;
            ConnectedWorkspaceIdTextV1::decode(text).map_err(|_| {
                "error: graph connected-workspace identity must be canonical cwi1:h: text\n"
            })?;
            if self.connected_workspace.replace(text.to_owned()).is_some() {
                return Err("error: graph accepts --connected-workspace-id only once\n");
            }
            return Ok(true);
        }
        if option == OsStr::new("--source-slot-id") {
            let text = graph_identity_text(
                value,
                "error: graph source-slot identity must be non-empty Unicode\n",
            )?;
            SourceSlotIdTextV1::decode(text)
                .map_err(|_| "error: graph source-slot identity must be canonical ssi1:h: text\n")?;
            if self.source_slot.replace(text.to_owned()).is_some() {
                return Err("error: graph accepts --source-slot-id only once\n");
            }
            return Ok(true);
        }
        Ok(false)
    }

    fn into_context(self) -> Result<GraphWorkspaceContext, &'static str> {
        match (
            self.repository_identity,
            self.connected_workspace,
            self.source_slot,
        ) {
            (Some(repository_identity), None, None) => {
                Ok(GraphWorkspaceContext::SingleRepository(repository_identity))
            }
            (None, Some(connected_workspace), Some(source_slot)) => {
                Ok(GraphWorkspaceContext::ConnectedWorkspace {
                    connected_workspace,
                    source_slot,
                })
            }
            (None, None, None) => Err("error: graph requires --repository-id or connected workspace selectors; use graph --help\n"),
            _ => Err("error: graph workspace selectors must be complete and unambiguous\n"),
        }
    }
}

enum GraphArgumentKind {
    Numeric(&'static str),
    Text(&'static str),
    Json(&'static str),
    EdgeKind,
}

const GRAPH_VALUE_FLAGS: &[&str] = &[
    "--repository-id",
    "--database",
    "--workspace-view",
    "--graph-generation",
    "--timeout-ms",
    "--max-input-edges",
    "--max-input-bytes",
    "--max-depth",
    "--max-results",
    "--max-visited-nodes",
    "--max-visited-edges",
    "--max-frontier",
    "--max-output-bytes",
    "--query",
    "--site-json",
    "--start-json",
    "--direction",
    "--edge-kind",
];

fn parse_graph_arguments(arguments: &[OsString]) -> Result<GraphInvocation, &'static str> {
    let (operation_text, options) = arguments
        .split_first()
        .ok_or("error: graph requires an operation; use graph --help\n")?;
    let operation = CliGraphOperation::parse(operation_text)
        .ok_or("error: unknown graph operation; use graph --help\n")?;
    let mut workspace_arguments = GraphWorkspaceArguments::default();
    let mut database = None;
    let mut input = serde_json::Map::new();
    let mut edge_kinds = Vec::new();
    let mut index = 0_usize;

    while index < options.len() {
        let option = &options[index];
        let value = options
            .get(index + 1)
            .ok_or("error: graph option requires a value; use graph --help\n")?;
        index += 2;
        if workspace_arguments.accept_option(option, value)? {
            continue;
        }
        if option == OsStr::new("--database") {
            if value.is_empty() {
                return Err("error: graph database path must not be empty\n");
            }
            if database.replace(PathBuf::from(value)).is_some() {
                return Err("error: graph accepts --database only once\n");
            }
            continue;
        }

        let kind = graph_argument_kind(operation, option)
            .ok_or("error: unsupported graph operation option; use graph --help\n")?;
        match kind {
            GraphArgumentKind::EdgeKind => {
                let text = value
                    .to_str()
                    .filter(|value| !value.is_empty())
                    .ok_or("error: graph edge kind must be non-empty Unicode\n")?;
                edge_kinds.push(serde_json::Value::String(text.to_owned()));
            }
            GraphArgumentKind::Numeric(field) => {
                let number = parse_graph_u64(value)?;
                insert_graph_field(&mut input, field, serde_json::Value::from(number))?;
            }
            GraphArgumentKind::Text(field) => {
                let text = value
                    .to_str()
                    .filter(|value| !value.is_empty())
                    .ok_or("error: graph text option must be non-empty Unicode\n")?;
                insert_graph_field(
                    &mut input,
                    field,
                    serde_json::Value::String(text.to_owned()),
                )?;
            }
            GraphArgumentKind::Json(field) => {
                let text = value
                    .to_str()
                    .filter(|value| !value.is_empty() && value.len() <= 64 * 1024)
                    .ok_or("error: graph selector JSON is invalid or exceeds 64 KiB\n")?;
                let parsed: serde_json::Value = serde_json::from_str(text)
                    .map_err(|_| "error: graph selector JSON is invalid or exceeds 64 KiB\n")?;
                if !parsed.is_object() {
                    return Err("error: graph selector JSON must be an object\n");
                }
                insert_graph_field(&mut input, field, parsed)?;
            }
        }
    }
    if !edge_kinds.is_empty() {
        input.insert(
            "edge_kinds".to_owned(),
            serde_json::Value::Array(edge_kinds),
        );
    }
    let database = database.ok_or("error: graph requires --database; use graph --help\n")?;
    let workspace = workspace_arguments.into_context()?;
    let request = validate_graph_input(operation, serde_json::Value::Object(input))?;
    Ok(GraphInvocation {
        database,
        workspace,
        request,
    })
}

fn graph_identity_text<'a>(value: &'a OsStr, error: &'static str) -> Result<&'a str, &'static str> {
    value.to_str().filter(|value| !value.is_empty()).ok_or(error)
}

fn graph_argument_kind(operation: CliGraphOperation, option: &OsStr) -> Option<GraphArgumentKind> {
    let common = if option == OsStr::new("--workspace-view") {
        Some(GraphArgumentKind::Numeric("workspace_view"))
    } else if option == OsStr::new("--graph-generation") {
        Some(GraphArgumentKind::Numeric("graph_generation"))
    } else if option == OsStr::new("--timeout-ms") {
        Some(GraphArgumentKind::Numeric("timeout_ms"))
    } else {
        None
    };
    if common.is_some() {
        return common;
    }
    if operation.accepts_limits() {
        let limit = graph_limit_argument(option);
        if limit.is_some() {
            return limit;
        }
    }
    match operation {
        CliGraphOperation::Search if option == OsStr::new("--query") => {
            Some(GraphArgumentKind::Text("query"))
        }
        CliGraphOperation::Evidence if option == OsStr::new("--site-json") => {
            Some(GraphArgumentKind::Json("site"))
        }
        CliGraphOperation::Trace if option == OsStr::new("--start-json") => {
            Some(GraphArgumentKind::Json("start"))
        }
        CliGraphOperation::Trace if option == OsStr::new("--direction") => {
            Some(GraphArgumentKind::Text("direction"))
        }
        CliGraphOperation::Trace | CliGraphOperation::Impact
            if option == OsStr::new("--edge-kind") =>
        {
            Some(GraphArgumentKind::EdgeKind)
        }
        CliGraphOperation::Impact if option == OsStr::new("--start-json") => {
            Some(GraphArgumentKind::Json("start"))
        }
        _ => None,
    }
}

fn graph_limit_argument(option: &OsStr) -> Option<GraphArgumentKind> {
    if option == OsStr::new("--max-input-edges") {
        Some(GraphArgumentKind::Numeric("max_input_edges"))
    } else if option == OsStr::new("--max-input-bytes") {
        Some(GraphArgumentKind::Numeric("max_input_bytes"))
    } else if option == OsStr::new("--max-depth") {
        Some(GraphArgumentKind::Numeric("max_depth"))
    } else if option == OsStr::new("--max-results") {
        Some(GraphArgumentKind::Numeric("max_results"))
    } else if option == OsStr::new("--max-visited-nodes") {
        Some(GraphArgumentKind::Numeric("max_visited_nodes"))
    } else if option == OsStr::new("--max-visited-edges") {
        Some(GraphArgumentKind::Numeric("max_visited_edges"))
    } else if option == OsStr::new("--max-frontier") {
        Some(GraphArgumentKind::Numeric("max_frontier"))
    } else if option == OsStr::new("--max-output-bytes") {
        Some(GraphArgumentKind::Numeric("max_output_bytes"))
    } else {
        None
    }
}

fn insert_graph_field(
    input: &mut serde_json::Map<String, serde_json::Value>,
    field: &'static str,
    value: serde_json::Value,
) -> Result<(), &'static str> {
    if input.insert(field.to_owned(), value).is_some() {
        Err("error: graph accepts each option only once\n")
    } else {
        Ok(())
    }
}

fn parse_graph_u64(value: &OsStr) -> Result<u64, &'static str> {
    value
        .to_str()
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or("error: graph numeric option must be a non-negative integer\n")
}

fn validate_graph_input(
    operation: CliGraphOperation,
    input: serde_json::Value,
) -> Result<GraphReadServiceRequest, &'static str> {
    const INVALID: &str = "error: graph request is invalid or exceeds a resource bound\n";
    match operation {
        CliGraphOperation::Status => serde_json::from_value::<GraphStatusInput>(input)
            .map_err(|_| INVALID)?
            .validate()
            .map_err(|_| INVALID),
        CliGraphOperation::Search => serde_json::from_value::<GraphSearchInput>(input)
            .map_err(|_| INVALID)?
            .validate()
            .map_err(|_| INVALID),
        CliGraphOperation::Evidence => serde_json::from_value::<GraphEvidenceInput>(input)
            .map_err(|_| INVALID)?
            .validate()
            .map_err(|_| INVALID),
        CliGraphOperation::Architecture => serde_json::from_value::<GraphArchitectureInput>(input)
            .map_err(|_| INVALID)?
            .validate()
            .map_err(|_| INVALID),
        CliGraphOperation::Trace => serde_json::from_value::<GraphTraceInput>(input)
            .map_err(|_| INVALID)?
            .validate()
            .map_err(|_| INVALID),
        CliGraphOperation::Impact => serde_json::from_value::<GraphImpactInput>(input)
            .map_err(|_| INVALID)?
            .validate()
            .map_err(|_| INVALID),
    }
}
