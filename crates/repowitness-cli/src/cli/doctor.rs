#[derive(Clone, Eq, PartialEq)]
struct DoctorTargetsInvocation {
    repository: PathBuf,
    database: PathBuf,
}

impl std::fmt::Debug for DoctorTargetsInvocation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DoctorTargetsInvocation")
            .field("repository", &"<redacted-path>")
            .field("database", &"<redacted-path>")
            .finish()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DoctorInvocation {
    configuration: ConfigurationInvocation,
    targets: Option<DoctorTargetsInvocation>,
}

trait DoctorInspector {
    fn inspect(
        &self,
        configuration: &ResolvedConfiguration,
        targets: Option<&DoctorTargetsInvocation>,
    ) -> LocalDoctorReport;
}

struct LocalDoctorInspector;

impl DoctorInspector for LocalDoctorInspector {
    fn inspect(
        &self,
        configuration: &ResolvedConfiguration,
        targets: Option<&DoctorTargetsInvocation>,
    ) -> LocalDoctorReport {
        inspect_local_doctor(
            configuration,
            targets.map(|targets| LocalDoctorTargets::new(&targets.repository, &targets.database)),
        )
    }
}

fn run_doctor(
    args: impl Iterator<Item = OsString>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
    loader: &impl ConfigurationLoader,
) -> u8 {
    run_doctor_with_inspector(args, stdout, stderr, loader, &LocalDoctorInspector)
}

fn run_doctor_with_inspector(
    args: impl Iterator<Item = OsString>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
    loader: &impl ConfigurationLoader,
    inspector: &impl DoctorInspector,
) -> u8 {
    let arguments: Vec<OsString> = args.take(MAX_DOCTOR_ARGUMENTS + 1).collect();
    if arguments.len() > MAX_DOCTOR_ARGUMENTS {
        return emit_error(
            stderr,
            EXIT_USAGE,
            "error: doctor received too many arguments; use doctor --help\n",
        );
    }
    if matches!(arguments.as_slice(), [help] if help == OsStr::new("--help") || help == OsStr::new("-h"))
    {
        return emit_output(stdout, DOCTOR_HELP);
    }
    let invocation = match parse_doctor_arguments(&arguments) {
        Ok(invocation) => invocation,
        Err(message) => return emit_error(stderr, EXIT_USAGE, message),
    };
    match loader.load(&invocation.configuration) {
        Ok(configuration) => {
            let report = inspector.inspect(&configuration, invocation.targets.as_ref());
            emit_doctor_report(stdout, &configuration, report)
        }
        Err(_) => emit_doctor_configuration_failure(stdout, invocation.targets.is_some()),
    }
}

fn parse_doctor_arguments(arguments: &[OsString]) -> Result<DoctorInvocation, &'static str> {
    let mut configuration_arguments = Vec::with_capacity(6);
    let mut repository = None;
    let mut database = None;
    let mut chunks = arguments.chunks_exact(2);
    for pair in &mut chunks {
        let option = pair[0].as_os_str();
        let value = pair[1].as_os_str();
        if value.is_empty() {
            return Err("error: doctor path must not be empty\n");
        }
        if is_configuration_option(option) {
            configuration_arguments.extend_from_slice(pair);
        } else if option == OsStr::new("--repository") {
            if repository.replace(PathBuf::from(value)).is_some() {
                return Err("error: each doctor target may be supplied only once\n");
            }
        } else if option == OsStr::new("--database") {
            if database.replace(PathBuf::from(value)).is_some() {
                return Err("error: each doctor target may be supplied only once\n");
            }
        } else {
            return Err("error: unknown doctor option; use doctor --help\n");
        }
    }
    if !chunks.remainder().is_empty() {
        return Err("error: doctor option requires a path\n");
    }
    let configuration = parse_configuration_arguments(&configuration_arguments, "doctor")?;
    let targets = match (repository, database) {
        (None, None) => None,
        (Some(repository), Some(database)) => Some(DoctorTargetsInvocation {
            repository,
            database,
        }),
        _ => return Err("error: --repository and --database must be supplied together\n"),
    };
    Ok(DoctorInvocation {
        configuration,
        targets,
    })
}

fn is_configuration_option(option: &OsStr) -> bool {
    option == OsStr::new("--user-config")
        || option == OsStr::new("--workspace-config")
        || option == OsStr::new("--repository-config")
}
