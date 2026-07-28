# Fuzz targets

The standalone `memory_record` target exercises arbitrary YAML bytes plus
structured mutations of both accepted version-1 golden profiles. Any accepted
record is generated again, reparsed, and required to preserve its domain value,
canonical JSON, and canonical digest.

Install the official runner and use a supported nightly toolchain:

```text
cargo install cargo-fuzz --locked
cargo +nightly fuzz run memory_record -- \
  -max_total_time=60 \
  -timeout=2 \
  -max_len=65537 \
  -print_final_stats=1
```

The fuzz crate has its own committed lockfile and is intentionally outside the
production workspace. `make fuzz-check` verifies that its target and lockfile
still build, while `make deny` audits both the production and fuzz dependency
graphs. The exact `libfuzzer-sys` 0.4.13 pin wraps LLVM libFuzzer, compiles a
native runtime through its build script, and has a compound
`(MIT OR Apache-2.0) AND NCSA` license; the NCSA term is admitted only for that
exact package in `deny.toml`.

Corpus, artifact, coverage, and target directories are ignored. Preserve any
reproducer that finds a defect as a reviewed regression fixture before
clearing generated artifacts.
