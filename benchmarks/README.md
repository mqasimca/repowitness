# RepoWitness benchmarks

Benchmark manifests pin public corpora, tasks, change scenarios, baselines, environments, and pass/fail budgets used by milestone reviews.

The manifests reference external repositories by exact commit. They do not vendor upstream source, tests, fixtures, or generated content. Fetch a corpus into a disposable location when running a benchmark and verify its revision before use.

The Phase 0 manifest and its budgets are ratified. Its local Rust preparation
probe is superseded by a reproducible full product-loop runner covering
persistence, exact reuse, retrieval, default-read-only MCP, canonical memory
management, source-change revalidation, recall, and context exclusion. A clean
exact-revision Ubuntu 24.04 attestation passes every numerical ceiling.
Separately, the adversarial release matrix passes the required local and CI
test profiles. A controlled public before/after evaluation compares the
declared lexical and naive-memory baselines and proves stale-memory exclusion.
An opt-in Codex evaluation checks the usefulness of the actual structured MCP
packet with runtime tools disabled, tool-event rejection, and exact
packet-evidence citation validation. A real design-partner outcome remains.

Run the offline manifest checks with:

```text
./scripts/check-benchmarks
```

The opt-in agent evaluation requires an installed authenticated Codex CLI and a
clean checkout of the public corpus:

```text
./scripts/run-phase0-codex-evaluation /path/to/mini-redis 1
```

The proposed Phase 1 correctness runner uses a clean checkout of its
manifest-pinned public corpus and emits aggregate workload timing only:

```text
./scripts/run-phase1-benchmark /path/to/public-corpus
```

## Manifests

- [Phase 0 Rust evidence and memory](phase0/README.md)
- [Phase 1 trustworthy local core](phase1/README.md)
