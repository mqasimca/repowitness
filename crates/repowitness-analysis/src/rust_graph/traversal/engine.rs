use std::{cmp::Ordering, collections::BTreeSet};

use super::super::RustGraphDefinitionIdentity;
use super::model::{
    RustGraphTraceDirection, RustGraphTraceError, RustGraphTraceStart, RustGraphTraversalEdge,
};
use super::request::{RustGraphTraceControl, RustGraphTraceLimits, RustGraphTraceRequest};
use super::result::{RustGraphTraceEdge, RustGraphTraceResult, RustGraphTraceTruncation};

const SORT_CHUNK: usize = 256;
const TRACE_OUTPUT_FIXED_BYTES: u64 = 96;
const TRACE_EDGE_FIXED_BYTES: u64 = 20;
const DEFINITION_FIXED_BYTES: u64 = 105;
const SITE_FIXED_BYTES: u64 = 101;

/// Traverses one complete immutable generation-local relationship slice.
///
/// Cancellation, deadline, malformed input, and byte-accounting failures
/// return no partial result. Resource ceilings that safely admit a bounded
/// prefix are returned through independent truncation flags.
pub fn trace_rust_graph(
    request: RustGraphTraceRequest<'_>,
) -> Result<RustGraphTraceResult, RustGraphTraceError> {
    request.control().outcome()?;
    let admitted = AdmittedEdges::new(request.edges(), request.limits(), request.control())?;
    let mut traversal = Traversal::new(&request, admitted.input_bytes)?;
    match request.start() {
        RustGraphTraceStart::Definition(definition) => {
            traversal.seen.insert(definition.clone());
            traversal.current.insert(definition.clone());
            traversal.run_from_depth(&admitted, 1)?;
        }
        RustGraphTraceStart::Site(site) => {
            traversal.run_from_site(&admitted, site)?;
        }
    }
    request.control().outcome()?;
    traversal.finish()
}

struct AdmittedEdges<'a> {
    edges: &'a [RustGraphTraversalEdge],
    by_site: Vec<usize>,
    outbound: Vec<usize>,
    inbound: Vec<usize>,
    input_bytes: u64,
}

impl<'a> AdmittedEdges<'a> {
    fn new(
        edges: &'a [RustGraphTraversalEdge],
        limits: RustGraphTraceLimits,
        control: RustGraphTraceControl<'_>,
    ) -> Result<Self, RustGraphTraceError> {
        let edge_count =
            u64::try_from(edges.len()).map_err(|_| RustGraphTraceError::CountOverflow)?;
        if edge_count > limits.max_input_edges() {
            return Err(RustGraphTraceError::InputEdgeLimitExceeded);
        }
        let mut input_bytes = 0_u64;
        for edge in edges {
            control.outcome()?;
            input_bytes = input_bytes
                .checked_add(encoded_relationship_bytes(edge)?)
                .ok_or(RustGraphTraceError::CountOverflow)?;
            if input_bytes > limits.max_input_bytes() {
                return Err(RustGraphTraceError::InputByteLimitExceeded);
            }
        }

        let mut by_site = (0..edges.len()).collect::<Vec<_>>();
        cancellable_sort_indices(
            &mut by_site,
            |left, right| compare_by_site(&edges[left], &edges[right]),
            control,
        )?;
        validate_site_groups(edges, &by_site, control)?;

        let mut outbound = by_site.clone();
        cancellable_sort_indices(
            &mut outbound,
            |left, right| compare_outbound(&edges[left], &edges[right]),
            control,
        )?;
        let mut inbound = by_site.clone();
        cancellable_sort_indices(
            &mut inbound,
            |left, right| compare_inbound(&edges[left], &edges[right]),
            control,
        )?;
        control.outcome()?;
        Ok(Self {
            edges,
            by_site,
            outbound,
            inbound,
            input_bytes,
        })
    }

    fn adjacent(
        &self,
        definition: &RustGraphDefinitionIdentity,
        direction: RustGraphTraceDirection,
    ) -> &[usize] {
        let indexes = match direction {
            RustGraphTraceDirection::Outbound => &self.outbound,
            RustGraphTraceDirection::Inbound => &self.inbound,
        };
        let first = indexes
            .partition_point(|index| oriented_origin(&self.edges[*index], direction) < definition);
        let end = first
            + indexes[first..].partition_point(|index| {
                oriented_origin(&self.edges[*index], direction) == definition
            });
        &indexes[first..end]
    }

    fn matching_site(&self, site: &super::super::RustGraphSiteIdentity) -> &[usize] {
        let first = self
            .by_site
            .partition_point(|index| self.edges[*index].site() < site);
        let end = first
            + self.by_site[first..].partition_point(|index| self.edges[*index].site() == site);
        &self.by_site[first..end]
    }
}

struct Traversal<'a> {
    request: &'a RustGraphTraceRequest<'a>,
    seen: BTreeSet<RustGraphDefinitionIdentity>,
    current: BTreeSet<RustGraphDefinitionIdentity>,
    next: BTreeSet<RustGraphDefinitionIdentity>,
    results: Vec<RustGraphTraceEdge>,
    visited_edges: u64,
    maximum_completed_depth: u32,
    truncation: RustGraphTraceTruncation,
    input_bytes: u64,
    output_bytes: u64,
    stop: bool,
}

impl<'a> Traversal<'a> {
    fn new(
        request: &'a RustGraphTraceRequest<'a>,
        input_bytes: u64,
    ) -> Result<Self, RustGraphTraceError> {
        if TRACE_OUTPUT_FIXED_BYTES > request.limits().max_output_bytes() {
            return Err(RustGraphTraceError::OutputLimitExceeded);
        }
        Ok(Self {
            request,
            seen: BTreeSet::new(),
            current: BTreeSet::new(),
            next: BTreeSet::new(),
            results: Vec::new(),
            visited_edges: 0,
            maximum_completed_depth: 0,
            truncation: RustGraphTraceTruncation::default(),
            input_bytes,
            output_bytes: TRACE_OUTPUT_FIXED_BYTES,
            stop: false,
        })
    }

    fn run_from_site(
        &mut self,
        admitted: &AdmittedEdges<'_>,
        site: &super::super::RustGraphSiteIdentity,
    ) -> Result<(), RustGraphTraceError> {
        let matching = admitted.matching_site(site);
        if matching.is_empty() {
            return Err(RustGraphTraceError::StartUnavailable);
        }
        let mut selected = Vec::new();
        for index in matching {
            self.request.control().outcome()?;
            let edge = &admitted.edges[*index];
            if self.request.edge_kinds().allows(edge.kind()) {
                if !self.admit_visited_edge()? {
                    break;
                }
                selected.push(*index);
            }
        }
        cancellable_sort_indices(
            &mut selected,
            |left, right| compare_relationship(&admitted.edges[left], &admitted.edges[right]),
            self.request.control(),
        )?;

        for index in &selected {
            let edge = &admitted.edges[*index];
            let origin = oriented_origin(edge, self.request.direction()).clone();
            if !self.seen.contains(&origin) && !self.admit_node(origin) {
                self.truncation.set_visited_nodes();
            }
        }
        self.process_selected_edges(admitted, &selected, 1)?;
        if !self.stop && !selected.is_empty() {
            self.maximum_completed_depth = 1;
        }
        if self.request.limits().max_depth() == 1 {
            if self.frontier_has_allowed_edges(admitted)? {
                self.truncation.set_depth();
            }
            return Ok(());
        }
        self.current = std::mem::take(&mut self.next);
        self.run_from_depth(admitted, 2)
    }

    fn run_from_depth(
        &mut self,
        admitted: &AdmittedEdges<'_>,
        first_depth: u32,
    ) -> Result<(), RustGraphTraceError> {
        for depth in first_depth..=self.request.limits().max_depth() {
            self.request.control().outcome()?;
            if self.current.is_empty() || self.stop {
                break;
            }
            let mut selected = Vec::new();
            let current = std::mem::take(&mut self.current);
            for definition in &current {
                self.request.control().outcome()?;
                for index in admitted.adjacent(definition, self.request.direction()) {
                    let edge = &admitted.edges[*index];
                    if !self.request.edge_kinds().allows(edge.kind()) {
                        continue;
                    }
                    if !self.admit_visited_edge()? {
                        break;
                    }
                    selected.push(*index);
                }
                if self.stop {
                    break;
                }
            }
            cancellable_sort_indices(
                &mut selected,
                |left, right| compare_relationship(&admitted.edges[left], &admitted.edges[right]),
                self.request.control(),
            )?;
            self.process_selected_edges(admitted, &selected, depth)?;
            if !self.stop {
                self.maximum_completed_depth = depth;
            }
            if depth == self.request.limits().max_depth() {
                if self.frontier_has_allowed_edges(admitted)? {
                    self.truncation.set_depth();
                }
                break;
            }
            self.current = std::mem::take(&mut self.next);
        }
        Ok(())
    }

    fn frontier_has_allowed_edges(
        &self,
        admitted: &AdmittedEdges<'_>,
    ) -> Result<bool, RustGraphTraceError> {
        for definition in &self.next {
            self.request.control().outcome()?;
            for index in admitted.adjacent(definition, self.request.direction()) {
                self.request.control().outcome()?;
                if self
                    .request
                    .edge_kinds()
                    .allows(admitted.edges[*index].kind())
                {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    fn process_selected_edges(
        &mut self,
        admitted: &AdmittedEdges<'_>,
        selected: &[usize],
        depth: u32,
    ) -> Result<(), RustGraphTraceError> {
        for index in selected {
            self.request.control().outcome()?;
            let edge = &admitted.edges[*index];
            let origin = oriented_origin(edge, self.request.direction());
            if !self.seen.contains(origin) {
                continue;
            }
            let neighbor = oriented_neighbor(edge, self.request.direction());
            let is_new = !self.seen.contains(neighbor);
            if is_new {
                if !self.admit_node(neighbor.clone()) {
                    self.truncation.set_visited_nodes();
                    continue;
                }
                if self.next.len()
                    >= usize::try_from(self.request.limits().max_frontier()).unwrap_or(usize::MAX)
                {
                    self.truncation.set_frontier();
                } else {
                    self.next.insert(neighbor.clone());
                }
            }
            if self.results.len()
                >= usize::try_from(self.request.limits().max_results()).unwrap_or(usize::MAX)
            {
                self.truncation.set_results();
                self.stop = true;
                break;
            }
            self.add_output_bytes(
                TRACE_EDGE_FIXED_BYTES
                    .checked_add(encoded_relationship_bytes(edge)?)
                    .ok_or(RustGraphTraceError::CountOverflow)?,
            )?;
            self.results
                .push(RustGraphTraceEdge::new(depth, edge.clone()));
        }
        Ok(())
    }

    fn admit_visited_edge(&mut self) -> Result<bool, RustGraphTraceError> {
        if self.visited_edges >= self.request.limits().max_visited_edges() {
            self.truncation.set_visited_edges();
            self.stop = true;
            return Ok(false);
        }
        self.visited_edges = self
            .visited_edges
            .checked_add(1)
            .ok_or(RustGraphTraceError::CountOverflow)?;
        Ok(true)
    }

    fn admit_node(&mut self, definition: RustGraphDefinitionIdentity) -> bool {
        if u64::try_from(self.seen.len()).unwrap_or(u64::MAX)
            >= self.request.limits().max_visited_nodes()
        {
            return false;
        }
        self.seen.insert(definition);
        true
    }

    fn add_output_bytes(&mut self, bytes: u64) -> Result<(), RustGraphTraceError> {
        self.output_bytes = self
            .output_bytes
            .checked_add(bytes)
            .ok_or(RustGraphTraceError::CountOverflow)?;
        if self.output_bytes > self.request.limits().max_output_bytes() {
            return Err(RustGraphTraceError::OutputLimitExceeded);
        }
        Ok(())
    }

    fn finish(self) -> Result<RustGraphTraceResult, RustGraphTraceError> {
        self.request.control().outcome()?;
        Ok(RustGraphTraceResult::new(
            self.results.into_boxed_slice(),
            u64::try_from(self.seen.len()).map_err(|_| RustGraphTraceError::CountOverflow)?,
            self.visited_edges,
            self.maximum_completed_depth,
            self.truncation,
            self.request.coverage(),
            self.input_bytes,
            self.output_bytes,
        ))
    }
}

fn validate_site_groups(
    edges: &[RustGraphTraversalEdge],
    indexes: &[usize],
    control: RustGraphTraceControl<'_>,
) -> Result<(), RustGraphTraceError> {
    let mut start = 0;
    while start < indexes.len() {
        control.outcome()?;
        let first = &edges[indexes[start]];
        let mut end = start + 1;
        while end < indexes.len() && edges[indexes[end]].site() == first.site() {
            control.outcome()?;
            let current = &edges[indexes[end]];
            if current.source() != first.source()
                || current.extraction_evidence() != first.extraction_evidence()
                || current.cardinality() != first.cardinality()
            {
                return Err(RustGraphTraceError::InvalidEdge);
            }
            if current.target() == edges[indexes[end - 1]].target() {
                return Err(RustGraphTraceError::DuplicateEdge);
            }
            end += 1;
        }
        let retained =
            u32::try_from(end - start).map_err(|_| RustGraphTraceError::CountOverflow)?;
        if retained != first.cardinality().retained_candidates() {
            return Err(RustGraphTraceError::InvalidEdge);
        }
        start = end;
    }
    Ok(())
}

fn compare_by_site(left: &RustGraphTraversalEdge, right: &RustGraphTraversalEdge) -> Ordering {
    left.site()
        .cmp(right.site())
        .then_with(|| left.target().cmp(right.target()))
        .then_with(|| left.source().cmp(right.source()))
        .then_with(|| left.cardinality().cmp(&right.cardinality()))
        .then_with(|| left.extraction_evidence().cmp(&right.extraction_evidence()))
        .then_with(|| left.resolution_evidence().cmp(&right.resolution_evidence()))
}

fn compare_relationship(left: &RustGraphTraversalEdge, right: &RustGraphTraversalEdge) -> Ordering {
    left.kind()
        .cmp(&right.kind())
        .then_with(|| left.resolution_evidence().cmp(&right.resolution_evidence()))
        .then_with(|| left.extraction_evidence().cmp(&right.extraction_evidence()))
        .then_with(|| left.source().cmp(right.source()))
        .then_with(|| left.site().cmp(right.site()))
        .then_with(|| left.target().cmp(right.target()))
        .then_with(|| left.cardinality().cmp(&right.cardinality()))
}

fn compare_outbound(left: &RustGraphTraversalEdge, right: &RustGraphTraversalEdge) -> Ordering {
    left.source()
        .cmp(right.source())
        .then_with(|| compare_relationship(left, right))
}

fn compare_inbound(left: &RustGraphTraversalEdge, right: &RustGraphTraversalEdge) -> Ordering {
    left.target()
        .cmp(right.target())
        .then_with(|| compare_relationship(left, right))
}

fn oriented_origin(
    edge: &RustGraphTraversalEdge,
    direction: RustGraphTraceDirection,
) -> &RustGraphDefinitionIdentity {
    match direction {
        RustGraphTraceDirection::Outbound => edge.source(),
        RustGraphTraceDirection::Inbound => edge.target(),
    }
}

fn oriented_neighbor(
    edge: &RustGraphTraversalEdge,
    direction: RustGraphTraceDirection,
) -> &RustGraphDefinitionIdentity {
    match direction {
        RustGraphTraceDirection::Outbound => edge.target(),
        RustGraphTraceDirection::Inbound => edge.source(),
    }
}

fn encoded_relationship_bytes(edge: &RustGraphTraversalEdge) -> Result<u64, RustGraphTraceError> {
    let source = encoded_definition_bytes(edge.source())?;
    let target = encoded_definition_bytes(edge.target())?;
    let site = SITE_FIXED_BYTES
        .checked_add(len_u64(edge.site().path().as_bytes().len())?)
        .ok_or(RustGraphTraceError::CountOverflow)?;
    source
        .checked_add(target)
        .and_then(|bytes| bytes.checked_add(site))
        .and_then(|bytes| bytes.checked_add(16))
        .ok_or(RustGraphTraceError::CountOverflow)
}

pub(super) fn encoded_definition_bytes(
    definition: &RustGraphDefinitionIdentity,
) -> Result<u64, RustGraphTraceError> {
    DEFINITION_FIXED_BYTES
        .checked_add(len_u64(definition.path().as_bytes().len())?)
        .ok_or(RustGraphTraceError::CountOverflow)
}

fn len_u64(value: usize) -> Result<u64, RustGraphTraceError> {
    u64::try_from(value).map_err(|_| RustGraphTraceError::CountOverflow)
}

fn cancellable_sort_indices(
    indexes: &mut [usize],
    compare: impl Fn(usize, usize) -> Ordering + Copy,
    control: RustGraphTraceControl<'_>,
) -> Result<(), RustGraphTraceError> {
    for chunk in indexes.chunks_mut(SORT_CHUNK) {
        control.outcome()?;
        chunk.sort_unstable_by(|left, right| compare(*left, *right));
    }
    if indexes.len() <= SORT_CHUNK {
        return control.outcome();
    }

    let mut scratch = indexes.to_vec();
    let mut width = SORT_CHUNK;
    let mut source_is_indexes = true;
    while width < indexes.len() {
        control.outcome()?;
        if source_is_indexes {
            merge_runs(indexes, &mut scratch, width, compare, control)?;
        } else {
            merge_runs(&scratch, indexes, width, compare, control)?;
        }
        source_is_indexes = !source_is_indexes;
        width = width
            .checked_mul(2)
            .ok_or(RustGraphTraceError::CountOverflow)?;
    }
    if !source_is_indexes {
        indexes.copy_from_slice(&scratch);
    }
    control.outcome()
}

fn merge_runs(
    source: &[usize],
    destination: &mut [usize],
    width: usize,
    compare: impl Fn(usize, usize) -> Ordering + Copy,
    control: RustGraphTraceControl<'_>,
) -> Result<(), RustGraphTraceError> {
    let run = width
        .checked_mul(2)
        .ok_or(RustGraphTraceError::CountOverflow)?;
    for start in (0..source.len()).step_by(run) {
        control.outcome()?;
        let middle = start.saturating_add(width).min(source.len());
        let end = start.saturating_add(run).min(source.len());
        let (mut left, mut right) = (start, middle);
        for output in &mut destination[start..end] {
            if (left + right) & 0xFF == 0 {
                control.outcome()?;
            }
            let take_left = right >= end
                || (left < middle && compare(source[left], source[right]) != Ordering::Greater);
            if take_left {
                *output = source[left];
                left += 1;
            } else {
                *output = source[right];
                right += 1;
            }
        }
    }
    Ok(())
}
