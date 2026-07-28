# RepoWitness benchmarks

Benchmark manifests pin public corpora, tasks, change scenarios, baselines, environments, and pass/fail budgets used by milestone reviews.

The manifests reference external repositories by exact commit. They do not vendor upstream source, tests, fixtures, or generated content. Fetch a corpus into a disposable location when running a benchmark and verify its revision before use.

The Phase 0 manifest remains proposed. Its local Rust preparation probe is
superseded by a reproducible full product-loop runner covering persistence,
exact reuse, retrieval, default-read-only MCP, canonical memory management,
source-change revalidation, recall, and context exclusion. The current dirty
development run passes every proposed numerical ceiling. Explicit budget
ratification, clean-revision attestation, residual release-matrix cases, and a
comparative design-partner outcome remain.

Run the offline manifest checks with:

```text
./scripts/check-benchmarks
```

## Manifests

- [Phase 0 Rust evidence and memory](phase0/README.md)
