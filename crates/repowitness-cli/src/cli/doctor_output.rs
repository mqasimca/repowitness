fn emit_doctor_report(
    writer: &mut impl Write,
    configuration: &ResolvedConfiguration,
    report: LocalDoctorReport,
) -> u8 {
    let mut output = Vec::with_capacity(2 * 1024);
    if write_doctor_report(&mut output, configuration, report).is_err()
        || output.len() > MAX_CLI_CONFIGURATION_OUTPUT_BYTES
    {
        return EXIT_SOFTWARE;
    }
    if writer.write_all(&output).is_err() {
        EXIT_IO
    } else if report.error_count() == 0 {
        EXIT_SUCCESS
    } else {
        EXIT_SOFTWARE
    }
}

fn write_doctor_report(
    writer: &mut impl Write,
    configuration: &ResolvedConfiguration,
    report: LocalDoctorReport,
) -> std::io::Result<()> {
    writeln!(writer, "operation=doctor")?;
    writeln!(writer, "status={}", report.status().as_str())?;
    write_configuration_identity(writer, configuration)?;
    let tool_profile = configuration.preferences().mcp_tool_profile();
    writeln!(
        writer,
        "requested_mcp_tool_profile={}",
        tool_profile.requested().as_str()
    )?;
    writeln!(
        writer,
        "authorized_mcp_tool_profile={}",
        tool_profile
            .authorized()
            .map_or("none", McpToolProfile::as_str)
    )?;
    writeln!(
        writer,
        "enabled_language_adapter_count={}",
        report.enabled_language_adapter_count()
    )?;
    writeln!(
        writer,
        "compiled_language_adapter_count={}",
        report.compiled_language_adapter_count()
    )?;
    write_doctor_checks(writer, report)?;
    writeln!(
        writer,
        "database_state={}",
        report.database_state().as_str()
    )?;
    match report.sqlite_runtime_version_number() {
        Some(version) => writeln!(writer, "sqlite_runtime_version_number={version}")?,
        None => writeln!(writer, "sqlite_runtime_version_number=not_run")?,
    }
    writeln!(writer, "error_count={}", report.error_count())?;
    writeln!(writer, "warning_count={}", report.warning_count())?;
    write_doctor_warnings(writer, report)
}

fn write_doctor_checks(writer: &mut impl Write, report: LocalDoctorReport) -> std::io::Result<()> {
    for (name, status) in [
        ("configuration", report.configuration()),
        ("language_adapters", report.language_adapters()),
        ("mcp_tool_profile", report.mcp_tool_profile()),
        ("incompatible_settings", report.incompatible_settings()),
        ("repository_capability", report.repository_capability()),
        ("database_placement", report.database_placement()),
        ("database_capability", report.database_capability()),
        ("sqlite_runtime", report.sqlite_runtime()),
        ("sqlite_compile_options", report.sqlite_compile_options()),
        ("database_schema", report.database_schema()),
    ] {
        writeln!(writer, "check_{name}={}", status.as_str())?;
    }
    Ok(())
}

fn write_doctor_warnings(
    writer: &mut impl Write,
    report: LocalDoctorReport,
) -> std::io::Result<()> {
    let mut index = 0;
    if report.language_adapters() == repowitness_local::DoctorCheckStatus::Warning {
        writeln!(writer, "warning_{index}=no_language_adapters_enabled")?;
        index += 1;
    }
    if !report.target_checks_requested() {
        writeln!(writer, "warning_{index}=target_checks_not_requested")?;
    } else if report.database_state() == repowitness_local::DoctorDatabaseState::Missing {
        writeln!(writer, "warning_{index}=database_missing")?;
    }
    Ok(())
}

fn emit_doctor_configuration_failure(writer: &mut impl Write, targets_requested: bool) -> u8 {
    let database_state = if targets_requested {
        "unavailable"
    } else {
        "not_requested"
    };
    let mut output = Vec::with_capacity(768);
    let result = (|| -> std::io::Result<()> {
        writeln!(output, "operation=doctor")?;
        writeln!(output, "status=error")?;
        writeln!(output, "requested_mcp_tool_profile=not_run")?;
        writeln!(output, "authorized_mcp_tool_profile=not_run")?;
        writeln!(output, "enabled_language_adapter_count=not_run")?;
        writeln!(output, "compiled_language_adapter_count=5")?;
        writeln!(output, "check_configuration=error")?;
        for name in [
            "language_adapters",
            "mcp_tool_profile",
            "incompatible_settings",
            "repository_capability",
            "database_placement",
            "database_capability",
            "sqlite_runtime",
            "sqlite_compile_options",
            "database_schema",
        ] {
            writeln!(output, "check_{name}=not_run")?;
        }
        writeln!(output, "database_state={database_state}")?;
        writeln!(output, "sqlite_runtime_version_number=not_run")?;
        writeln!(output, "error_count=1")?;
        writeln!(output, "warning_count=0")
    })();
    if result.is_err() || output.len() > MAX_CLI_CONFIGURATION_OUTPUT_BYTES {
        return EXIT_SOFTWARE;
    }
    if writer.write_all(&output).is_ok() {
        EXIT_SOFTWARE
    } else {
        EXIT_IO
    }
}
