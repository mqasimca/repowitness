# RepoWitness benchmarks

Benchmark manifests pin public corpora, tasks, change scenarios, baselines, environments, and pass/fail budgets used by milestone reviews.

The manifests reference external repositories by exact commit. They do not vendor upstream source, tests, fixtures, or generated content. Fetch a corpus into a disposable location when running a benchmark and verify its revision before use.

The Phase 0 manifest remains proposed. Its local Rust preparation probe is
implemented and reproducible; persistence/retrieval/MCP are implemented
elsewhere in the test suite but still need integration into the pinned runner.
Memory-revalidation and context-quality tasks remain future gates.

Run the offline manifest checks with:

```text
./scripts/check-benchmarks
```

## Manifests

- [Phase 0 Rust evidence and memory](phase0/README.md)
