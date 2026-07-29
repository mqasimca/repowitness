fn emit_configuration_report(writer: &mut impl Write, configuration: &ResolvedConfiguration) -> u8 {
    let mut output = Vec::with_capacity(8 * 1024);
    if write_configuration_report(&mut output, configuration).is_err() {
        return EXIT_SOFTWARE;
    }
    if output.len() > MAX_CLI_CONFIGURATION_OUTPUT_BYTES {
        return EXIT_SOFTWARE;
    }
    if writer.write_all(&output).is_ok() {
        EXIT_SUCCESS
    } else {
        EXIT_IO
    }
}

fn write_configuration_report(
    writer: &mut impl Write,
    configuration: &ResolvedConfiguration,
) -> std::io::Result<()> {
    writeln!(writer, "operation=config_explain")?;
    write_configuration_identity(writer, configuration)?;
    let preferences = configuration.preferences();
    write_preference(writer, "query_results", preferences.query_results())?;
    write_preference(writer, "context_bytes", preferences.context_bytes())?;
    write_preference(writer, "graph_depth", preferences.graph_depth())?;
    write_preference(writer, "graph_results", preferences.graph_results())?;
    write_preference(
        writer,
        "watcher_poll_interval_ms",
        preferences.watcher_poll_interval_ms(),
    )?;
    write_tool_profile(writer, preferences.mcp_tool_profile())?;
    let policy = configuration.policy();
    write_language_policy(writer, policy.allowed_languages())?;
    write_tool_profile_policy(writer, policy.allowed_mcp_tool_profiles())?;
    write_numeric_policy(
        writer,
        "max_source_file_bytes",
        policy.max_source_file_bytes(),
    )?;
    write_numeric_policy(writer, "max_source_files", policy.max_source_files())?;
    write_numeric_policy(writer, "max_query_results", policy.max_query_results())?;
    write_numeric_policy(writer, "max_context_bytes", policy.max_context_bytes())?;
    write_numeric_policy(writer, "max_graph_depth", policy.max_graph_depth())?;
    write_numeric_policy(writer, "max_graph_results", policy.max_graph_results())?;
    let retention = policy.retention();
    write_numeric_policy(
        writer,
        "retained_generations_per_source_slot",
        retention.retained_generations_per_source_slot(),
    )?;
    write_numeric_policy(
        writer,
        "max_retention_generation_candidates",
        retention.max_generation_candidates(),
    )?;
    write_numeric_policy(writer, "max_retention_rows", retention.max_rows())?;
    write_numeric_policy(writer, "max_retention_bytes", retention.max_bytes())?;
    write_boolean_policy(writer, "deny_memory_writes", policy.deny_memory_writes())?;
    write_boolean_policy(writer, "follow_symlinks", policy.follow_symlinks())?;
    let warning_count = u8::from(preferences.mcp_tool_profile().authorized().is_none())
        + u8::from(policy.allowed_languages().effective().is_empty());
    writeln!(writer, "warning_count={warning_count}")?;
    if preferences.mcp_tool_profile().authorized().is_none() {
        writeln!(writer, "warning_0=requested_mcp_tool_profile_unavailable")?;
    }
    let language_warning_index = usize::from(preferences.mcp_tool_profile().authorized().is_none());
    if policy.allowed_languages().effective().is_empty() {
        writeln!(
            writer,
            "warning_{language_warning_index}=no_source_language_enabled"
        )?;
    }
    writeln!(writer, "unsupported_setting_count=0")
}

fn write_configuration_identity(
    writer: &mut impl Write,
    configuration: &ResolvedConfiguration,
) -> std::io::Result<()> {
    writeln!(writer, "schema_version={}", configuration.schema_version())?;
    writeln!(
        writer,
        "resolver_version={}",
        configuration.resolver_version()
    )?;
    writeln!(writer, "profile={}", configuration.profile().as_str())?;
    writeln!(
        writer,
        "profile_supplied_by={}",
        configuration_layer_text(configuration.profile_supplied_by())
    )?;
    writeln!(
        writer,
        "configuration_digest_sha256={}",
        hex(configuration.digest().as_bytes())
    )
}

fn write_preference(
    writer: &mut impl Write,
    name: &str,
    preference: &ResolvedPreference<u64>,
) -> std::io::Result<()> {
    writeln!(
        writer,
        "preference_{name}_requested={}",
        preference.requested()
    )?;
    writeln!(
        writer,
        "preference_{name}_effective={}",
        preference.effective()
    )?;
    writeln!(
        writer,
        "preference_{name}_supplied_by={}",
        configuration_layer_text(preference.supplied_by())
    )?;
    write_layers(
        writer,
        &format!("preference_{name}_constrained_by"),
        preference.constrained_by(),
    )
}

fn write_tool_profile(
    writer: &mut impl Write,
    preference: &ResolvedToolProfilePreference,
) -> std::io::Result<()> {
    writeln!(
        writer,
        "preference_mcp_tool_profile_requested={}",
        preference.requested().as_str()
    )?;
    writeln!(
        writer,
        "preference_mcp_tool_profile_authorized={}",
        preference
            .authorized()
            .map_or("none", McpToolProfile::as_str)
    )?;
    writeln!(
        writer,
        "preference_mcp_tool_profile_supplied_by={}",
        configuration_layer_text(preference.supplied_by())
    )?;
    write_layers(
        writer,
        "preference_mcp_tool_profile_constrained_by",
        preference.constrained_by(),
    )
}

fn write_language_policy(
    writer: &mut impl Write,
    policy: &PolicyValue<std::collections::BTreeSet<SourceLanguage>>,
) -> std::io::Result<()> {
    writeln!(
        writer,
        "policy_allowed_languages_count={}",
        policy.effective().len()
    )?;
    for (index, language) in policy.effective().iter().enumerate() {
        writeln!(
            writer,
            "policy_allowed_language_{index}={}",
            language.as_str()
        )?;
    }
    write_layers(
        writer,
        "policy_allowed_languages_constrained_by",
        policy.constraining_layers(),
    )
}

fn write_tool_profile_policy(
    writer: &mut impl Write,
    policy: &PolicyValue<std::collections::BTreeSet<McpToolProfile>>,
) -> std::io::Result<()> {
    writeln!(
        writer,
        "policy_allowed_mcp_tool_profiles_count={}",
        policy.effective().len()
    )?;
    for (index, profile) in policy.effective().iter().enumerate() {
        writeln!(
            writer,
            "policy_allowed_mcp_tool_profile_{index}={}",
            profile.as_str()
        )?;
    }
    write_layers(
        writer,
        "policy_allowed_mcp_tool_profiles_constrained_by",
        policy.constraining_layers(),
    )
}

fn write_numeric_policy(
    writer: &mut impl Write,
    name: &str,
    policy: &PolicyValue<u64>,
) -> std::io::Result<()> {
    writeln!(writer, "policy_{name}={}", policy.effective())?;
    write_layers(
        writer,
        &format!("policy_{name}_constrained_by"),
        policy.constraining_layers(),
    )
}

fn write_boolean_policy(
    writer: &mut impl Write,
    name: &str,
    policy: &PolicyValue<bool>,
) -> std::io::Result<()> {
    writeln!(writer, "policy_{name}={}", policy.effective())?;
    write_layers(
        writer,
        &format!("policy_{name}_constrained_by"),
        policy.constraining_layers(),
    )
}

fn write_layers(
    writer: &mut impl Write,
    field: &str,
    layers: &[ConfigurationLayerKind],
) -> std::io::Result<()> {
    writeln!(writer, "{field}_count={}", layers.len())?;
    for (index, layer) in layers.iter().copied().enumerate() {
        writeln!(
            writer,
            "{field}_{index}={}",
            configuration_layer_text(layer)
        )?;
    }
    Ok(())
}
