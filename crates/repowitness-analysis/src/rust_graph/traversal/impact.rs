use std::collections::BTreeMap;

use super::super::{RustGraphResolutionEvidence, RustGraphSiteEvidence};
use super::engine::{encoded_definition_bytes, trace_rust_graph};
use super::model::{RustGraphImpactClass, RustGraphTraceError, RustGraphTraversalEdge};
use super::result::{RustGraphImpact, RustGraphImpactRequest, RustGraphImpactResult};

const IMPACT_OUTPUT_FIXED_BYTES: u64 = 64;
const IMPACT_ITEM_FIXED_BYTES: u64 = 8;

/// Computes conservative inbound impact from one exact declaration.
///
/// A complete unique path with only direct syntax evidence may support
/// `directly_connected`. Ambiguous or heuristic paths remain `possible`.
/// Unsupported and truncated work is reported separately as unknown coverage.
pub fn analyze_rust_graph_impact(
    request: RustGraphImpactRequest<'_>,
) -> Result<RustGraphImpactResult, RustGraphTraceError> {
    let start = request.start().clone();
    let limits = request.limits();
    let trace = trace_rust_graph(request.trace_request())?;
    let mut strongest = BTreeMap::new();
    strongest.insert(
        start.clone(),
        (RustGraphImpactClass::DirectlyConnected, 0_u32),
    );

    let mut relationship_unknown = false;
    for traced in trace.edges() {
        let relationship = traced.relationship();
        let Some((parent_class, _)) = strongest.get(relationship.target()).copied() else {
            return Err(RustGraphTraceError::InvalidEdge);
        };
        let class = propagated_class(parent_class, relationship);
        relationship_unknown |= relationship.cardinality().candidates_truncated();
        strongest
            .entry(relationship.source().clone())
            .and_modify(|(current_class, current_depth)| {
                if class < *current_class {
                    *current_class = class;
                }
                *current_depth = (*current_depth).min(traced.depth());
            })
            .or_insert((class, traced.depth()));
    }

    let mut impacted = strongest
        .into_iter()
        .filter(|(definition, _)| definition != &start)
        .map(|(definition, (class, minimum_depth))| {
            RustGraphImpact::new(class, definition, minimum_depth)
        })
        .collect::<Vec<_>>();
    impacted.sort_by(|left, right| {
        left.minimum_depth()
            .cmp(&right.minimum_depth())
            .then_with(|| left.class().cmp(&right.class()))
            .then_with(|| left.definition().cmp(right.definition()))
    });

    let mut output_bytes = trace
        .output_bytes()
        .checked_add(IMPACT_OUTPUT_FIXED_BYTES)
        .ok_or(RustGraphTraceError::CountOverflow)?;
    for item in &impacted {
        let definition_bytes = encoded_definition_bytes(item.definition())?;
        output_bytes = output_bytes
            .checked_add(IMPACT_ITEM_FIXED_BYTES)
            .and_then(|bytes| bytes.checked_add(definition_bytes))
            .ok_or(RustGraphTraceError::CountOverflow)?;
        if output_bytes > limits.max_output_bytes() {
            return Err(RustGraphTraceError::OutputLimitExceeded);
        }
    }
    let unknown_coverage =
        trace.coverage().has_unknown_impact() || trace.truncation().any() || relationship_unknown;
    Ok(RustGraphImpactResult::new(
        trace,
        impacted.into_boxed_slice(),
        unknown_coverage,
        output_bytes,
    ))
}

fn propagated_class(
    parent: RustGraphImpactClass,
    relationship: &RustGraphTraversalEdge,
) -> RustGraphImpactClass {
    if parent != RustGraphImpactClass::DirectlyConnected
        || relationship.cardinality().is_ambiguous()
        || relationship.extraction_evidence() == RustGraphSiteEvidence::SyntaxHeuristic
        || relationship.resolution_evidence() == RustGraphResolutionEvidence::ExactNameHeuristic
    {
        RustGraphImpactClass::Possible
    } else {
        RustGraphImpactClass::DirectlyConnected
    }
}
