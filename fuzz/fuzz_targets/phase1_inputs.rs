#![no_main]

use libfuzzer_sys::fuzz_target;
use repowitness_local::{ConfigurationFileLayer, parse_configuration_file};
use repowitness_mcp::{
    ArchitectureMapInput, ArchitectureOverviewInput, ChangeReviewInput, CodeGraphQueryInput,
    CodeSearchInput, CrossRepositorySearchInput, DiagnosticsInput, EvidenceContextBuildInput,
    GraphArchitectureInput, GraphEvidenceInput, GraphImpactInput, GraphSearchInput,
    GraphStatusInput, GraphTraceInput, MemoryManageInput, MemoryRecallInput, OutboundSitesInput,
    RelevantPathsInput, RepositoryTopologyInput, SymbolGetInput, SymbolSearchInput,
    SyntaxSiteSearchInput, TestMarkersInput,
};

const MAX_INPUT_BYTES: usize = 64 * 1024;
const VALID_CONFIGURATION: &[u8] = b"schema_version = 1\n";
const VALID_STATUS: &[u8] = br#"{}"#;
const VALID_SEARCH: &[u8] = br#"{"query":"fixture"}"#;
const VALID_ARCHITECTURE: &[u8] = br#"{}"#;

fn mutate_seed(data: &[u8], seed: &[u8]) -> Vec<u8> {
    let mut candidate = seed.to_vec();
    for pair in data.chunks_exact(2) {
        let offset = usize::from(pair[0]) % candidate.len();
        candidate[offset] ^= pair[1];
    }
    candidate
}

fn exercise_configuration(input: &[u8]) {
    for layer in [
        ConfigurationFileLayer::User,
        ConfigurationFileLayer::Workspace,
        ConfigurationFileLayer::Repository,
    ] {
        let _ = parse_configuration_file(input, layer);
    }
}

fn exercise_graph_wire(input: &[u8]) {
    if let Ok(request) = serde_json::from_slice::<GraphStatusInput>(input) {
        let _ = request.validate();
    }
    if let Ok(request) = serde_json::from_slice::<GraphSearchInput>(input) {
        let _ = request.validate();
    }
    if let Ok(request) = serde_json::from_slice::<GraphEvidenceInput>(input) {
        let _ = request.validate();
    }
    if let Ok(request) = serde_json::from_slice::<GraphArchitectureInput>(input) {
        let _ = request.validate();
    }
    if let Ok(request) = serde_json::from_slice::<GraphTraceInput>(input) {
        let _ = request.validate();
    }
    if let Ok(request) = serde_json::from_slice::<GraphImpactInput>(input) {
        let _ = request.validate();
    }
}

fn exercise_wire(input: &[u8]) {
    for result in [
        serde_json::from_slice::<ArchitectureMapInput>(input).is_ok(),
        serde_json::from_slice::<ArchitectureOverviewInput>(input).is_ok(),
        serde_json::from_slice::<ChangeReviewInput>(input).is_ok(),
        serde_json::from_slice::<CodeGraphQueryInput>(input).is_ok(),
        serde_json::from_slice::<CodeSearchInput>(input).is_ok(),
        serde_json::from_slice::<CrossRepositorySearchInput>(input).is_ok(),
        serde_json::from_slice::<DiagnosticsInput>(input).is_ok(),
        serde_json::from_slice::<EvidenceContextBuildInput>(input).is_ok(),
        serde_json::from_slice::<MemoryManageInput>(input).is_ok(),
        serde_json::from_slice::<MemoryRecallInput>(input).is_ok(),
        serde_json::from_slice::<OutboundSitesInput>(input).is_ok(),
        serde_json::from_slice::<RelevantPathsInput>(input).is_ok(),
        serde_json::from_slice::<RepositoryTopologyInput>(input).is_ok(),
        serde_json::from_slice::<SymbolGetInput>(input).is_ok(),
        serde_json::from_slice::<SymbolSearchInput>(input).is_ok(),
        serde_json::from_slice::<SyntaxSiteSearchInput>(input).is_ok(),
        serde_json::from_slice::<TestMarkersInput>(input).is_ok(),
    ] {
        std::hint::black_box(result);
    }
}

fuzz_target!(|data: &[u8]| {
    let input = &data[..data.len().min(MAX_INPUT_BYTES)];
    exercise_configuration(input);
    exercise_graph_wire(input);
    exercise_wire(input);

    let seed = match input.first().copied().unwrap_or_default() % 4 {
        0 => VALID_CONFIGURATION,
        1 => VALID_STATUS,
        2 => VALID_SEARCH,
        _ => VALID_ARCHITECTURE,
    };
    let mutated_seed = mutate_seed(input.get(1..).unwrap_or_default(), seed);
    exercise_configuration(&mutated_seed);
    exercise_graph_wire(&mutated_seed);
    exercise_wire(&mutated_seed);
});
