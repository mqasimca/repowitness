# Fuzz targets

The standalone `memory_record` target uses arbitrary YAML bytes and structured
mutations of both accepted version-1 golden profiles. For each accepted record,
it generates and parses the record again. The domain value, canonical JSON, and
canonical digest must stay the same.

The `phase1_inputs` target sends bounded arbitrary bytes to each strict Phase 1
native-graph and compatibility-MCP request decoder. It also sends these bytes
to the three configuration-file layers. The target applies bounded byte changes
to accepted synthetic configuration, status, search, and architecture request
seeds. This lets validation code process accepted input. The target does not use
the filesystem, database, Git, network, or MCP transport I/O.

Install the official runner and use a supported nightly toolchain:

```text
cargo install cargo-fuzz --locked
cargo +nightly fuzz run memory_record -- \
  -max_total_time=60 \
  -timeout=2 \
  -max_len=65537 \
  -print_final_stats=1

cargo +nightly fuzz run phase1_inputs -- \
  -max_total_time=60 \
  -timeout=2 \
  -max_len=65537 \
  -print_final_stats=1
```

The fuzz crate has its own committed lockfile. It is outside the production
workspace. `make fuzz-check` checks that its target and lockfile build.
`make deny` checks the production and fuzz dependency graphs. The exact
`libfuzzer-sys` 0.4.13 pin uses LLVM libFuzzer. Its build script compiles a
native runtime. Its license is `(MIT OR Apache-2.0) AND NCSA`. `deny.toml`
allows the NCSA term only for this package.

Corpus, artifact, coverage, and target directories are ignored. If a reproducer
finds a defect, keep it as a reviewed regression fixture. Then clear generated
artifacts.
