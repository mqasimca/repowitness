use std::{
    error::Error,
    path::Path,
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};

use repowitness_local::{
    ApplicationRustGraphSiteSelector, LocalRustGraphReadOutput, LocalRustGraphReadRequest,
    LocalRustGraphReadResult, ResolvedConfiguration, RustGraphDefinitionRecord,
    RustGraphDefinitionSelector, RustGraphEdgeKinds, RustGraphReadOperation, RustGraphSiteSelector,
    RustGraphSymbolQuery, RustGraphTraceDirection, RustGraphTraceLimits,
    RustGraphTraceStartSelector, read_local_rust_graph,
};

use super::{metrics, redact_stage_failure};

type GraphResult<T> = Result<T, Box<dyn Error>>;

const OPERATIONS_PER_RUN: u64 = 6;
const SEARCH_QUERY: &str = "run";

#[derive(Clone, Copy)]
pub(super) struct GraphMetrics {
    pub(super) p95: Duration,
    pub(super) operations_per_run: u64,
    pub(super) material_result_bound_bytes: u64,
    pub(super) mixed_generation_reads: u64,
}

#[derive(Clone)]
struct GraphInputs {
    query: RustGraphSymbolQuery,
    definition: RustGraphDefinitionSelector,
    site: ApplicationRustGraphSiteSelector,
    edge_kinds: RustGraphEdgeKinds,
    limits: RustGraphTraceLimits,
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct GraphPin {
    workspace_view: i64,
    graph_generation: i64,
}

pub(super) fn measure(
    database: &Path,
    repository_identity: &str,
    configuration: &ResolvedConfiguration,
    runs: usize,
    max_wall_ms: u64,
    max_result_bytes: u64,
) -> GraphResult<GraphMetrics> {
    let inputs = prepare_inputs(
        database,
        repository_identity,
        configuration,
        max_result_bytes,
    )?;
    let expected_pin = pin(&read(
        database,
        repository_identity,
        configuration,
        RustGraphReadOperation::Status,
    )?);
    let mut samples = Vec::with_capacity(runs);

    for _ in 0..runs {
        let started = Instant::now();
        run_suite(
            database,
            repository_identity,
            configuration,
            &inputs,
            expected_pin,
        )?;
        let elapsed = started.elapsed();
        samples.push(elapsed);
    }

    let p95 = redact_stage_failure(
        metrics::nearest_rank_p95(&mut samples),
        "native graph percentile measurement failed",
    )?;
    if p95 > Duration::from_millis(max_wall_ms) {
        return Err("native graph read suite p95 exceeded the resource budget".into());
    }
    Ok(GraphMetrics {
        p95,
        operations_per_run: OPERATIONS_PER_RUN,
        material_result_bound_bytes: max_result_bytes,
        mixed_generation_reads: 0,
    })
}

fn prepare_inputs(
    database: &Path,
    repository_identity: &str,
    configuration: &ResolvedConfiguration,
    max_result_bytes: u64,
) -> GraphResult<GraphInputs> {
    let query = redact_stage_failure(
        RustGraphSymbolQuery::try_new(SEARCH_QUERY),
        "native graph query setup failed",
    )?;
    let limits = bounded_limits(max_result_bytes)?;
    let edge_kinds = redact_stage_failure(
        RustGraphEdgeKinds::try_new(true, true, true),
        "native graph edge-kind setup failed",
    )?;
    let searched = read(
        database,
        repository_identity,
        configuration,
        RustGraphReadOperation::Search {
            query: query.clone(),
            limits,
        },
    )?;
    let definitions = match searched.output() {
        LocalRustGraphReadOutput::Search(result) => result.definitions(),
        _ => return Err("graph search returned the wrong operation result".into()),
    };
    for record in definitions {
        let definition = definition_selector(record)?;
        let traced = read(
            database,
            repository_identity,
            configuration,
            RustGraphReadOperation::Trace {
                start: RustGraphTraceStartSelector::Definition(definition.clone()),
                direction: RustGraphTraceDirection::Outbound,
                edge_kinds,
                limits,
            },
        )?;
        let LocalRustGraphReadOutput::Trace(trace) = traced.output() else {
            return Err("graph trace returned the wrong operation result".into());
        };
        if let Some(edge) = trace.edges().first() {
            return Ok(GraphInputs {
                query,
                definition,
                site: site_selector(edge.site())?,
                edge_kinds,
                limits,
            });
        }
    }
    Err("public benchmark graph had no traceable search result".into())
}

fn bounded_limits(max_result_bytes: u64) -> GraphResult<RustGraphTraceLimits> {
    let defaults = RustGraphTraceLimits::default();
    redact_stage_failure(
        RustGraphTraceLimits::try_new(
            defaults.max_input_edges(),
            defaults.max_input_bytes(),
            defaults.max_depth(),
            defaults.max_results(),
            defaults.max_visited_nodes(),
            defaults.max_visited_edges(),
            defaults.max_frontier(),
            max_result_bytes,
        ),
        "native graph limit setup failed",
    )
}

fn run_suite(
    database: &Path,
    repository_identity: &str,
    configuration: &ResolvedConfiguration,
    inputs: &GraphInputs,
    expected_pin: GraphPin,
) -> GraphResult<()> {
    let operations = [
        RustGraphReadOperation::Status,
        RustGraphReadOperation::Search {
            query: inputs.query.clone(),
            limits: inputs.limits,
        },
        RustGraphReadOperation::Evidence {
            site: inputs.site.clone(),
            limits: inputs.limits,
        },
        RustGraphReadOperation::Architecture {
            limits: inputs.limits,
        },
        RustGraphReadOperation::Trace {
            start: RustGraphTraceStartSelector::Definition(inputs.definition.clone()),
            direction: RustGraphTraceDirection::Outbound,
            edge_kinds: inputs.edge_kinds,
            limits: inputs.limits,
        },
        RustGraphReadOperation::Impact {
            start: inputs.definition.clone(),
            edge_kinds: inputs.edge_kinds,
            limits: inputs.limits,
        },
    ];
    for operation in operations {
        let result = read(database, repository_identity, configuration, operation)?;
        if pin(&result) != expected_pin {
            return Err("native graph suite mixed immutable generations".into());
        }
        validate_output(result.into_output())?;
    }
    Ok(())
}

fn validate_output(output: LocalRustGraphReadOutput) -> GraphResult<()> {
    match output {
        LocalRustGraphReadOutput::Status(_) => Ok(()),
        LocalRustGraphReadOutput::Search(result) => {
            if result.definitions().is_empty() {
                return Err("native graph search returned no benchmark definition".into());
            }
            Ok(())
        }
        LocalRustGraphReadOutput::Evidence(result) => {
            if result.evidence().is_none() {
                return Err("native graph evidence did not resolve its exact selector".into());
            }
            Ok(())
        }
        LocalRustGraphReadOutput::Architecture(result) => {
            if result.publication().definition_count() == 0 {
                return Err("native graph architecture reported no definitions".into());
            }
            Ok(())
        }
        LocalRustGraphReadOutput::Trace(result) => {
            if result.edges().is_empty() {
                return Err("native graph trace lost its prepared relationship".into());
            }
            Ok(())
        }
        LocalRustGraphReadOutput::Impact(_) => Ok(()),
    }
}

fn read(
    database: &Path,
    repository_identity: &str,
    configuration: &ResolvedConfiguration,
    operation: RustGraphReadOperation,
) -> GraphResult<LocalRustGraphReadResult> {
    redact_stage_failure(
        read_local_rust_graph(
            LocalRustGraphReadRequest::new(database, repository_identity, operation)
                .with_configuration(configuration),
            Arc::new(AtomicBool::new(false)),
        ),
        "native graph read operation failed",
    )
}

fn pin(result: &LocalRustGraphReadResult) -> GraphPin {
    GraphPin {
        workspace_view: result.workspace_view(),
        graph_generation: result.graph_generation(),
    }
}

fn definition_selector(
    record: &RustGraphDefinitionRecord,
) -> GraphResult<RustGraphDefinitionSelector> {
    redact_stage_failure(
        RustGraphDefinitionSelector::try_new(
            record.source_slot(),
            record.source_generation().get(),
            record.path().clone(),
            record.content_digest(),
            record.artifact(),
            record.fact_ordinal(),
            record.kind(),
            record.name().to_owned(),
            record.qualified_name().to_owned(),
            record.name_span(),
            record.declaration_span(),
        ),
        "native graph definition selector setup failed",
    )
}

fn site_selector(site: &RustGraphSiteSelector) -> GraphResult<ApplicationRustGraphSiteSelector> {
    redact_stage_failure(
        ApplicationRustGraphSiteSelector::try_new(
            site.source_slot(),
            site.path().clone(),
            site.artifact(),
            site.ordinal(),
            site.kind(),
            site.occurrence_span(),
            site.target_span(),
        ),
        "native graph site selector setup failed",
    )
}

#[cfg(test)]
mod tests {
    use super::bounded_limits;

    #[test]
    fn material_output_limit_is_the_exact_manifest_derived_value() {
        let limits = bounded_limits(65_536).expect("manifest budget should validate");
        assert_eq!(limits.max_output_bytes(), 65_536);
    }
}
