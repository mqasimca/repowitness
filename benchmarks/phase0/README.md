# Phase 0 Rust evidence and memory benchmark

- Status: Proposed
- Last reviewed: 2026-07-26
- Manifest: [`manifest.json`](manifest.json)

## Corpus choice

The first public corpus is [`tokio-rs/mini-redis`](https://github.com/tokio-rs/mini-redis) pinned to commit `3d93b42bc363220f85af4fc9e1bebd35b588a4a3`.

It is an MIT-licensed, Rust-only teaching project with a client, server, command modules, shared state, protocol framing, pub/sub, graceful shutdown, and asynchronous tests. At the pinned revision it contains 28 Rust files and 4,354 Rust source lines. That is large enough to exercise cross-module evidence while remaining small enough for Phase 0 crash, clean-versus-incremental, and repeated-query runs.

RepoWitness does not copy this corpus. A benchmark runner fetches the repository separately and checks out the exact manifest revision.

Run the local preparation and resource probe against a clean external checkout:

```text
./scripts/run-phase0-benchmark /path/to/mini-redis 5
```

The runner verifies the exact manifest revision before building the release
probe. The first preparation is reported as cold, later preparations as warm,
and repeated logical output must remain identical. On systems with
`/usr/bin/time`, the runner also reports peak resident memory. It never writes
to the corpus or creates a RepoWitness database.

## Initial tasks

The manifest defines:

- current-revision evidence retrieval for negative RESP bulk-length validation;
- a cross-module graceful-shutdown trace;
- current-revision evidence retrieval for SET expiration encoding;
- a source-only revalidation scenario for the SET expiration fix;
- a source-plus-regression-test revalidation scenario for negative bulk-length validation.

The lexical/source-only and naive-memory-text baselines use the same corpus revision and task wording.

## Current execution coverage

The pinned preparation runner is implemented and records cold/warm time, peak
RSS when available, repository/Rust-file/byte/fact counts, syntax errors, and
canonical snapshot identity. Repeated runs require identical logical output.

The product path now also implements SQLite persistence and exact reuse,
bounded FTS5 search, exact declaration retrieval, CLI commands, and local stdio
MCP. Those stages pass end-to-end tests on temporary and neighboring cloned
Rust repositories, but they are not yet wired into this pinned manifest runner.
Memory revalidation, context compilation, lexical/naive-memory comparisons,
and the design-partner task are not implemented.

## Budgets

Correctness budgets are zero-tolerance for false confirmed claims, silent truncation, mixed-generation reads, and false automatic relinks. Initial resource and latency numbers are proposals for this small corpus, not accepted release gates. Record the required environment data during the first benchmark run, publish cold and warm results, and ratify or revise the numeric budgets before optimizing against them.

## Provenance

- Repository: <https://github.com/tokio-rs/mini-redis.git>
- Pinned branch observation: `refs/heads/master`
- Revision verified: 2026-07-23
- License: MIT
- Reuse mode: external reference only; no upstream content is incorporated into RepoWitness
