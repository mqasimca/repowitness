const MAX_IDENTITY_ARGUMENTS: usize = 2;

const IDENTITY_HELP: &str = concat!(
    "Generate one canonical identity from operating-system secure randomness.\n\n",
    "Usage:\n",
    "  repowitness identity generate repository\n",
    "  repowitness identity generate connected-workspace\n",
    "  repowitness identity generate source-slot\n\n",
    "The command prints only the canonical versioned identity. It performs no\n",
    "repository, configuration, Git, or database access.\n",
);

fn run_identity(
    args: impl Iterator<Item = OsString>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
    generator: &impl IdentityGenerator,
) -> u8 {
    let arguments = args.take(MAX_IDENTITY_ARGUMENTS + 1).collect::<Vec<_>>();
    if arguments.len() > MAX_IDENTITY_ARGUMENTS {
        return emit_error(
            stderr,
            EXIT_USAGE,
            "error: identity received too many arguments; use identity --help\n",
        );
    }
    if identity_help_requested(&arguments) {
        return emit_output(stdout, IDENTITY_HELP);
    }
    let kind = match parse_identity_arguments(&arguments) {
        Ok(kind) => kind,
        Err(message) => return emit_error(stderr, EXIT_USAGE, message),
    };
    match generator.generate(kind) {
        Ok(identity) => emit_generated_identity(stdout, &identity),
        Err(_) => emit_error(stderr, EXIT_SOFTWARE, "error: identity generation failed\n"),
    }
}

fn identity_help_requested(arguments: &[OsString]) -> bool {
    matches!(arguments, [help] if help == OsStr::new("--help") || help == OsStr::new("-h"))
        || matches!(
            arguments,
            [subcommand, help]
                if subcommand == OsStr::new("generate")
                    && (help == OsStr::new("--help") || help == OsStr::new("-h"))
        )
}

fn parse_identity_arguments(arguments: &[OsString]) -> Result<LocalIdentityKind, &'static str> {
    let [subcommand, kind] = arguments else {
        return Err(
            "error: identity requires generate and one identity kind; use identity --help\n",
        );
    };
    if subcommand != OsStr::new("generate") {
        return Err("error: unknown identity command; use identity --help\n");
    }
    if kind == OsStr::new("repository") {
        Ok(LocalIdentityKind::Repository)
    } else if kind == OsStr::new("connected-workspace") {
        Ok(LocalIdentityKind::ConnectedWorkspace)
    } else if kind == OsStr::new("source-slot") {
        Ok(LocalIdentityKind::SourceSlot)
    } else {
        Err("error: unknown identity kind; use identity --help\n")
    }
}

trait IdentityGenerator {
    fn generate(&self, kind: LocalIdentityKind) -> Result<String, LocalIdentityGenerationError>;
}

struct OsIdentityGenerator;

impl IdentityGenerator for OsIdentityGenerator {
    fn generate(&self, kind: LocalIdentityKind) -> Result<String, LocalIdentityGenerationError> {
        generate_local_identity(kind).map(GeneratedLocalIdentity::into_string)
    }
}
