fn run_phase2_context_build(
    args: impl Iterator<Item = OsString>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
) -> u8 {
    let arguments: Vec<OsString> = args
        .take(MAX_PHASE2_CONTEXT_BUILD_ARGUMENTS + 1)
        .collect();
    if arguments.len() > MAX_PHASE2_CONTEXT_BUILD_ARGUMENTS {
        return emit_error(
            stderr,
            EXIT_USAGE,
            "error: phase2-context-build received too many arguments; use phase2-context-build --help\n",
        );
    }
    if matches!(arguments.as_slice(), [help] if help == OsStr::new("--help") || help == OsStr::new("-h"))
    {
        return emit_output(stdout, PHASE2_CONTEXT_BUILD_HELP);
    }
    let parsed = match parse_phase2_context_build_arguments(&arguments) {
        Ok(parsed) => parsed,
        Err(_) => {
            return emit_error(
                stderr,
                EXIT_USAGE,
                "error: invalid phase2-context-build arguments; use phase2-context-build --help\n",
            );
        }
    };
    let Phase2ContextInvocation {
        invocation,
        workspace,
        scip_symbol,
    } = parsed;
    let repository_identity = match invocation.repository_identity.to_str() {
        Some(value) => value,
        None => {
            return emit_error(
                stderr,
                EXIT_USAGE,
                "error: phase2-context-build repository identity must be valid UTF-8\n",
            );
        }
    };
    let intent = match invocation.intent.to_str() {
        Some(value) => value,
        None => {
            return emit_error(
                stderr,
                EXIT_USAGE,
                "error: phase2-context-build intent must be valid UTF-8\n",
            );
        }
    };
    let request = match workspace.as_ref() {
        Some((connected_workspace, source_slot)) => LocalPhase2ContextBuildRequest::for_connected_workspace(
            &invocation.root,
            &invocation.database,
            repository_identity,
            connected_workspace,
            source_slot,
            intent,
        ),
        None => LocalPhase2ContextBuildRequest::new(
            &invocation.root,
            &invocation.database,
            repository_identity,
            intent,
        ),
    };
    let request = match scip_symbol.as_deref() {
        Some(scip_symbol) => request.with_scip_symbol(scip_symbol),
        None => request,
    };
    let request = match request.with_budget_units(invocation.budget_units) {
        Ok(request) => request,
        Err(_) => {
            return emit_error(
                stderr,
                EXIT_USAGE,
                "error: invalid phase2-context-build limits; use phase2-context-build --help\n",
            );
        }
    };
    let request = match request.with_max_provider_results(invocation.max_provider_results) {
        Ok(request) => request,
        Err(_) => {
            return emit_error(
                stderr,
                EXIT_USAGE,
                "error: invalid phase2-context-build limits; use phase2-context-build --help\n",
            );
        }
    };
    match build_local_phase2_context(request, Arc::new(AtomicBool::new(false))) {
        Ok(result) => emit_phase2_context_report(stdout, &result),
        Err(_) => emit_error(
            stderr,
            EXIT_SOFTWARE,
            "error: phase2 context build failed\n",
        ),
    }
}

fn parse_phase2_context_build_arguments(
    arguments: &[OsString],
) -> Result<Phase2ContextInvocation, ()> {
    let mut context_arguments = Vec::with_capacity(arguments.len());
    let mut connected_workspace = None;
    let mut source_slot = None;
    let mut scip_symbol = None;
    let mut index = 0_usize;
    while index < arguments.len() {
        let option = &arguments[index];
        let value = arguments.get(index + 1).ok_or(())?;
        if option == OsStr::new("--connected-workspace-id") {
            let value = value.to_str().filter(|value| !value.is_empty()).ok_or(())?;
            if connected_workspace.replace(value.to_owned()).is_some() {
                return Err(());
            }
        } else if option == OsStr::new("--source-slot-id") {
            let value = value.to_str().filter(|value| !value.is_empty()).ok_or(())?;
            if source_slot.replace(value.to_owned()).is_some() {
                return Err(());
            }
        } else if option == OsStr::new("--scip-symbol") {
            let value = value.to_str().filter(|value| !value.is_empty()).ok_or(())?;
            if scip_symbol.replace(value.to_owned()).is_some() {
                return Err(());
            }
        } else {
            context_arguments.push(option.clone());
            context_arguments.push(value.clone());
        }
        index += 2;
    }
    let workspace = match (connected_workspace, source_slot) {
        (None, None) => None,
        (Some(connected_workspace), Some(source_slot)) => Some((connected_workspace, source_slot)),
        (None, Some(_)) | (Some(_), None) => return Err(()),
    };
    parse_context_build_arguments(&context_arguments)
        .map(|invocation| Phase2ContextInvocation {
            invocation,
            workspace,
            scip_symbol,
        })
        .map_err(|_| ())
}

struct Phase2ContextInvocation {
    invocation: ContextInvocation,
    workspace: Option<(String, String)>,
    scip_symbol: Option<String>,
}

fn emit_phase2_context_report(
    writer: &mut impl Write,
    result: &repowitness_local::LocalPhase2ContextBuildResult,
) -> u8 {
    let mut encoded = Vec::new();
    if write_phase2_context_report(&mut encoded, result).is_err()
        || encoded.len() > MAX_CLI_CONTEXT_OUTPUT_BYTES
    {
        return EXIT_SOFTWARE;
    }
    if writer.write_all(&encoded).is_ok() {
        EXIT_SUCCESS
    } else {
        EXIT_IO
    }
}

fn write_phase2_context_report(
    writer: &mut impl Write,
    result: &repowitness_local::LocalPhase2ContextBuildResult,
) -> std::io::Result<()> {
    let scope = result.scope();
    writeln!(writer, "status=ok")?;
    writeln!(writer, "operation=phase2-context-build")?;
    writeln!(writer, "profile_id={}", result.profile().id())?;
    writeln!(writer, "profile_version={}", result.profile().version())?;
    writeln!(writer, "budget_estimator=utf8_bytes_upper_bound_v1")?;
    writeln!(writer, "budget_units={}", result.budget().units())?;
    writeln!(writer, "used_units={}", result.used_units())?;
    writeln!(writer, "repository_sha256={}", hex(scope.repository().as_bytes()))?;
    writeln!(
        writer,
        "connected_workspace_sha256={}",
        hex(scope.connected_workspace().as_bytes())
    )?;
    writeln!(writer, "workspace_view={}", scope.workspace_view())?;
    writeln!(writer, "source_slot_sha256={}", hex(scope.source_slot().as_bytes()))?;
    writeln!(writer, "source_epoch={}", scope.source_epoch())?;
    writeln!(writer, "generation={}", scope.generation())?;
    writeln!(writer, "snapshot_sha256={}", hex(scope.snapshot().as_bytes()))?;
    writeln!(writer, "manifest_sha256={}", hex(scope.manifest().as_bytes()))?;
    writeln!(writer, "provider_coverage={}", result.provider_coverage().len())?;
    for (index, coverage) in result.provider_coverage().iter().enumerate() {
        writeln!(writer, "provider_coverage_{index}_tier={}", phase2_tier(coverage.tier()))?;
        let availability = match coverage.availability() {
            repowitness_local::Phase2ContextProviderAvailability::Available => "available",
            repowitness_local::Phase2ContextProviderAvailability::Unavailable => "unavailable",
        };
        writeln!(writer, "provider_coverage_{index}_availability={availability}")?;
        writeln!(writer, "provider_coverage_{index}_candidate_count={}", coverage.candidate_count())?;
    }
    writeln!(writer, "omissions={}", result.omissions().len())?;
    for (index, omission) in result.omissions().iter().enumerate() {
        writeln!(writer, "omission_{index}_tier={}", phase2_tier(omission.tier()))?;
        writeln!(writer, "omission_{index}_count={}", omission.count())?;
    }
    writeln!(writer, "items={}", result.items().len())?;
    for (index, item) in result.items().iter().enumerate() {
        write_phase2_context_item(writer, index, item)?;
    }
    Ok(())
}

#[allow(
    clippy::too_many_lines,
    reason = "one exhaustive payload encoder keeps the externally versioned Phase 2 receipt shape auditable"
)]
fn write_phase2_context_item(
    writer: &mut impl Write,
    index: usize,
    item: &Phase2ContextCandidate<LocalPhase2ContextItem>,
) -> std::io::Result<()> {
    let prefix = format!("context_item_{index}");
    writeln!(writer, "{prefix}_tier={}", phase2_tier(item.tier()))?;
    writeln!(writer, "{prefix}_provider_rank={}", item.provider_rank())?;
    writeln!(writer, "{prefix}_estimated_units={}", item.estimated_units())?;
    writeln!(writer, "{prefix}_identity_sha256={}", hex(item.identity().as_bytes()))?;
    writeln!(writer, "{prefix}_providers={}", item.attributions().len())?;
    for (provider_index, attribution) in item.attributions().iter().enumerate() {
        writeln!(
            writer,
            "{prefix}_provider_{provider_index}_identity_sha256={}",
            hex(attribution.provider().as_bytes())
        )?;
        writeln!(
            writer,
            "{prefix}_provider_{provider_index}_tier={}",
            phase2_tier(attribution.tier())
        )?;
        writeln!(
            writer,
            "{prefix}_provider_{provider_index}_rank={}",
            attribution.provider_rank()
        )?;
    }
    match item.payload() {
        LocalPhase2ContextItem::Syntax(candidate) => {
            writeln!(writer, "{prefix}_kind=syntax")?;
            writeln!(writer, "{prefix}_path_encoding=lowercase_hex")?;
            writeln!(writer, "{prefix}_path_hex={}", hex(candidate.selector().path().as_bytes()))?;
            writeln!(
                writer,
                "{prefix}_content_sha256={}",
                hex(candidate.selector().content_digest().as_bytes())
            )?;
            writeln!(
                writer,
                "{prefix}_artifact_sha256={}",
                hex(candidate.selector().artifact_digest().as_bytes())
            )?;
            writeln!(writer, "{prefix}_fact_ordinal={}", candidate.selector().fact_ordinal())?;
            writeln!(
                writer,
                "{prefix}_declaration_encoding={}",
                encoded_source_bytes(candidate.declaration()).encoding
            )?;
            let declaration = encoded_source_bytes(candidate.declaration());
            let declaration = serde_json::to_string(&declaration.data)
                .map_err(std::io::Error::other)?;
            writeln!(writer, "{prefix}_declaration_data_json={declaration}")
        }
        LocalPhase2ContextItem::Memory(record) => {
            let memory = record
                .record()
                .ok_or_else(|| std::io::Error::other("invalid current memory payload"))?;
            writeln!(writer, "{prefix}_kind=memory")?;
            writeln!(
                writer,
                "{prefix}_record_id_sha256={}",
                hex(record.record_id().as_bytes())
            )?;
            let title = serde_json::to_string(memory.claim().title().as_str())
                .map_err(std::io::Error::other)?;
            let body = serde_json::to_string(memory.claim().body().as_str())
                .map_err(std::io::Error::other)?;
            writeln!(writer, "{prefix}_title_json={title}")?;
            writeln!(writer, "{prefix}_body_json={body}")
        }
        LocalPhase2ContextItem::History(item) => {
            let memory = item
                .record()
                .record()
                .ok_or_else(|| std::io::Error::other("invalid current history payload"))?;
            writeln!(writer, "{prefix}_kind=history")?;
            writeln!(writer, "{prefix}_record_id_sha256={}", hex(item.record().record_id().as_bytes()))?;
            let format = match item.commit().object_format() {
                repowitness_local::MemoryObjectFormat::Sha1 => "sha1",
                repowitness_local::MemoryObjectFormat::Sha256 => "sha256",
            };
            writeln!(writer, "{prefix}_commit_object_format={format}")?;
            writeln!(writer, "{prefix}_commit_object_id_hex={}", hex(item.commit().as_bytes()))?;
            let title = serde_json::to_string(memory.claim().title().as_str())
                .map_err(std::io::Error::other)?;
            let body = serde_json::to_string(memory.claim().body().as_str())
                .map_err(std::io::Error::other)?;
            writeln!(writer, "{prefix}_title_json={title}")?;
            writeln!(writer, "{prefix}_body_json={body}")
        }
        LocalPhase2ContextItem::PreciseOverlay(item) => {
            let occurrence = item.occurrence();
            writeln!(writer, "{prefix}_kind=precise_overlay")?;
            writeln!(writer, "{prefix}_overlay_sha256={}", hex(item.overlay().digest().as_bytes()))?;
            writeln!(writer, "{prefix}_path_encoding=lowercase_hex")?;
            writeln!(writer, "{prefix}_path_hex={}", hex(occurrence.path().as_bytes()))?;
            writeln!(writer, "{prefix}_content_sha256={}", hex(occurrence.content().as_bytes()))?;
            writeln!(writer, "{prefix}_span_start={}", occurrence.span().start().get())?;
            writeln!(writer, "{prefix}_span_end={}", occurrence.span().end().get())?;
            writeln!(writer, "{prefix}_roles={}", occurrence.roles().bits())?;
            writeln!(writer, "{prefix}_relationship_count={}", item.relationship_count())?;
            let source = encoded_source_bytes(item.source());
            writeln!(writer, "{prefix}_source_encoding={}", source.encoding)?;
            let source = serde_json::to_string(&source.data).map_err(std::io::Error::other)?;
            writeln!(writer, "{prefix}_source_data_json={source}")
        }
        LocalPhase2ContextItem::GraphRelation(item) => {
            let candidate = item.candidate();
            writeln!(writer, "{prefix}_kind=graph_relation")?;
            writeln!(writer, "{prefix}_edge_kind={}", item.edge_kind().as_str())?;
            writeln!(writer, "{prefix}_depth={}", item.depth())?;
            writeln!(writer, "{prefix}_path_encoding=lowercase_hex")?;
            writeln!(writer, "{prefix}_path_hex={}", hex(candidate.selector().path().as_bytes()))?;
            writeln!(writer, "{prefix}_content_sha256={}", hex(candidate.selector().content_digest().as_bytes()))?;
            writeln!(writer, "{prefix}_artifact_sha256={}", hex(candidate.selector().artifact_digest().as_bytes()))?;
            writeln!(writer, "{prefix}_fact_ordinal={}", candidate.selector().fact_ordinal())?;
            let declaration = encoded_source_bytes(candidate.declaration());
            writeln!(writer, "{prefix}_declaration_encoding={}", declaration.encoding)?;
            let declaration = serde_json::to_string(&declaration.data).map_err(std::io::Error::other)?;
            writeln!(writer, "{prefix}_declaration_data_json={declaration}")
        }
    }
}

fn phase2_tier(tier: Phase2ContextTier) -> &'static str {
    match tier {
        Phase2ContextTier::Anchor => "anchor",
        Phase2ContextTier::PreciseOverlay => "precise_overlay",
        Phase2ContextTier::Syntax => "syntax",
        Phase2ContextTier::Structural => "structural",
        Phase2ContextTier::References => "references",
        Phase2ContextTier::Memory => "memory",
        Phase2ContextTier::History => "history",
        Phase2ContextTier::Unresolved => "unresolved",
    }
}
