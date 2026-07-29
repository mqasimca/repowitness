fn emit_generated_identity(writer: &mut impl Write, identity: &str) -> u8 {
    if writeln!(writer, "{identity}").is_ok() {
        EXIT_SUCCESS
    } else {
        EXIT_IO
    }
}
