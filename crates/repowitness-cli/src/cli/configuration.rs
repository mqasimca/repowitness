#[derive(Clone, Default, Eq, PartialEq)]
struct ConfigurationInvocation {
    user: Option<PathBuf>,
    workspace: Option<PathBuf>,
    repository: Option<PathBuf>,
}

impl std::fmt::Debug for ConfigurationInvocation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ConfigurationInvocation")
            .field("user", &self.user.as_ref().map(|_| "<redacted-path>"))
            .field(
                "workspace",
                &self.workspace.as_ref().map(|_| "<redacted-path>"),
            )
            .field(
                "repository",
                &self.repository.as_ref().map(|_| "<redacted-path>"),
            )
            .finish()
    }
}

fn extract_configuration_arguments(
    arguments: &[OsString],
    flag_options: &[&str],
) -> Result<(Vec<OsString>, ConfigurationInvocation), &'static str> {
    let mut remaining = Vec::with_capacity(arguments.len());
    let mut invocation = ConfigurationInvocation::default();
    let mut positional_only = false;
    let mut index = 0_usize;

    while index < arguments.len() {
        let argument = &arguments[index];
        if positional_only {
            remaining.push(argument.clone());
            index += 1;
            continue;
        }
        if argument == OsStr::new("--") {
            positional_only = true;
            remaining.push(argument.clone());
            index += 1;
            continue;
        }
        if let Some(target) = configuration_target(&mut invocation, argument) {
            let value = arguments
                .get(index + 1)
                .ok_or("error: configuration option requires a path\n")?;
            if value.is_empty() {
                return Err("error: configuration path must not be empty\n");
            }
            if target.replace(PathBuf::from(value)).is_some() {
                return Err("error: each configuration layer may be supplied only once\n");
            }
            index += 2;
            continue;
        }

        remaining.push(argument.clone());
        index += 1;
        if flag_options.iter().any(|flag| argument == OsStr::new(flag))
            || !os_string_starts_with_hyphen(argument)
        {
            continue;
        }
        if let Some(value) = arguments.get(index) {
            remaining.push(value.clone());
            index += 1;
        }
    }

    Ok((remaining, invocation))
}

fn configuration_target<'a>(
    invocation: &'a mut ConfigurationInvocation,
    option: &OsStr,
) -> Option<&'a mut Option<PathBuf>> {
    if option == OsStr::new("--user-config") {
        Some(&mut invocation.user)
    } else if option == OsStr::new("--workspace-config") {
        Some(&mut invocation.workspace)
    } else if option == OsStr::new("--repository-config") {
        Some(&mut invocation.repository)
    } else {
        None
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ConfigurationLoadError {
    Unavailable,
    Invalid,
}

trait ConfigurationLoader {
    fn load(
        &self,
        invocation: &ConfigurationInvocation,
    ) -> Result<ResolvedConfiguration, ConfigurationLoadError>;
}

struct LocalConfigurationLoader;

impl ConfigurationLoader for LocalConfigurationLoader {
    fn load(
        &self,
        invocation: &ConfigurationInvocation,
    ) -> Result<ResolvedConfiguration, ConfigurationLoadError> {
        let mut layers = Vec::with_capacity(3);
        load_optional_configuration(
            invocation.user.as_deref(),
            ConfigurationFileLayer::User,
            &mut layers,
        )?;
        load_optional_configuration(
            invocation.workspace.as_deref(),
            ConfigurationFileLayer::Workspace,
            &mut layers,
        )?;
        load_optional_configuration(
            invocation.repository.as_deref(),
            ConfigurationFileLayer::Repository,
            &mut layers,
        )?;
        resolve_configuration(&layers).map_err(|_| ConfigurationLoadError::Invalid)
    }
}

fn load_optional_configuration(
    path: Option<&Path>,
    layer: ConfigurationFileLayer,
    layers: &mut Vec<ConfigurationLayer>,
) -> Result<(), ConfigurationLoadError> {
    let Some(path) = path else {
        return Ok(());
    };
    let bytes = read_bounded_configuration(path)?;
    let parsed =
        parse_configuration_file(&bytes, layer).map_err(|_| ConfigurationLoadError::Invalid)?;
    layers.push(parsed);
    Ok(())
}

fn read_bounded_configuration(path: &Path) -> Result<Vec<u8>, ConfigurationLoadError> {
    read_bounded_regular_file(path, MAX_CONFIGURATION_FILE_BYTES).map_err(map_bounded_file_error)
}

fn map_bounded_file_error(error: BoundedFileReadError) -> ConfigurationLoadError {
    match error {
        BoundedFileReadError::Unavailable
        | BoundedFileReadError::Changed
        | BoundedFileReadError::InvalidRequest => ConfigurationLoadError::Unavailable,
        BoundedFileReadError::TooLarge => ConfigurationLoadError::Invalid,
    }
}

fn configuration_layer_text(layer: ConfigurationLayerKind) -> &'static str {
    match layer {
        ConfigurationLayerKind::BuiltInDefaults => "built_in_defaults",
        ConfigurationLayerKind::NamedProfile => "named_profile",
        ConfigurationLayerKind::User => "user",
        ConfigurationLayerKind::Workspace => "workspace",
        ConfigurationLayerKind::Repository => "repository",
        ConfigurationLayerKind::Environment => "environment",
        ConfigurationLayerKind::Cli => "cli",
    }
}
