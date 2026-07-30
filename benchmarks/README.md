# RepoWitness benchmarks

Benchmark manifests pin public corpora, tasks, change scenarios, baselines,
environments, and pass/fail budgets for milestone reviews.

The manifests identify external repositories by exact commit. They do not vendor
upstream source, tests, fixtures, or generated content. Fetch a corpus into a
temporary location. Check its revision before you run a benchmark.

The Phase 0 manifest and its budgets are ratified. A reproducible full
product-loop runner replaces the local Rust preparation probe. It tests
persistence, exact reuse, retrieval, the default read-only MCP server, canonical
memory management, source-change revalidation, recall, and context exclusion.
A clean Ubuntu 24.04 attestation at the exact revision passes every numeric
limit. The adversarial release matrix passes the required local and CI profiles.
A controlled public evaluation compares the declared lexical and simple-memory
baselines. It confirms that stale memory is excluded. An opt-in Codex evaluation
checks the structured MCP packet. It disables runtime tools, rejects tool
events, and validates each packet-evidence citation. A real design-partner
result is still required.

Run the offline manifest checks with:

```text
./scripts/check-benchmarks
```

The opt-in agent evaluation requires an installed and authenticated Codex CLI
and a clean checkout of the public corpus:

```text
./scripts/run-phase0-codex-evaluation /path/to/mini-redis 1
```

The proposed Phase 1 correctness runner uses a clean checkout of its
manifest-pinned public corpus. It emits aggregate workload timing only:

```text
./scripts/run-phase1-benchmark /path/to/public-corpus
```

## Manifests

- [Phase 0 Rust evidence and memory](phase0/README.md)
- [Phase 1 trustworthy local core](phase1/README.md)
