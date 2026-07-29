fn emit_graph_output(writer: &mut impl Write, output: &GraphReadServiceOutput) -> u8 {
    let encoded = match output {
        GraphReadServiceOutput::Status(output) => serde_json::to_vec(output),
        GraphReadServiceOutput::Search(output) => serde_json::to_vec(output),
        GraphReadServiceOutput::Evidence(output) => serde_json::to_vec(output),
        GraphReadServiceOutput::Architecture(output) => serde_json::to_vec(output),
        GraphReadServiceOutput::Trace(output) => serde_json::to_vec(output),
        GraphReadServiceOutput::Impact(output) => serde_json::to_vec(output),
    };
    let Ok(mut encoded) = encoded else {
        return EXIT_SOFTWARE;
    };
    if encoded.len() >= MAX_CLI_GRAPH_OUTPUT_BYTES {
        return EXIT_SOFTWARE;
    }
    encoded.push(b'\n');
    if writer.write_all(&encoded).is_ok() {
        EXIT_SUCCESS
    } else {
        EXIT_IO
    }
}
