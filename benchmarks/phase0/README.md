# Phase 0 Rust evidence and memory benchmark

- Status: Ratified
- Last reviewed: 2026-07-29
- Manifest: [`manifest.json`](manifest.json)

## Corpus choice

The first public corpus is [`tokio-rs/mini-redis`](https://github.com/tokio-rs/mini-redis) pinned to commit `3d93b42bc363220f85af4fc9e1bebd35b588a4a3`.

It is an MIT-licensed, Rust-only teaching project with a client, server, command modules, shared state, protocol framing, pub/sub, graceful shutdown, and asynchronous tests. At the pinned revision it contains 28 Rust files and 4,354 Rust source lines. That is large enough to exercise cross-module evidence while remaining small enough for Phase 0 crash, clean-versus-incremental, and repeated-query runs.

RepoWitness does not copy this corpus. A benchmark runner fetches the repository separately and checks out the exact manifest revision.

Run the local product-loop and resource probe against a clean external
checkout:

```text
./scripts/run-phase0-benchmark /path/to/mini-redis
```

The runner verifies the exact manifest revision before making a disposable
`--no-local` product clone and a separate comparison clone before building the
release CLI and probes. It creates SQLite databases and memory records only in
those clones, measures cold and unchanged indexing plus repeated warm queries,
changes one attached declaration, and runs the manifest's historical
before/after comparison. It validates and passes the manifest's bounded
resource profile explicitly into the probe, which reports the resolved values.
The manifest fixes the warm-query sample count at ten; callers cannot
substitute an unrecorded count.
It removes all disposable state on exit. The supplied corpus checkout remains
read-only. On supported systems, `/usr/bin/time` also records peak resident
memory.

Maintainers can run the `Phase 0 benchmark` workflow manually on `main`. It
checks out the exact dispatched revision on Ubuntu 24.04, obtains only the
manifest-pinned public corpus, verifies both clean worktrees, executes this
runner, checks the required attestation fields, and retains the public output
and SHA-256 checksums as a workflow artifact for 90 days. It has read-only
repository permission and does not reference repository secrets.

Run the separate opt-in Codex utility evaluation with:

```text
./scripts/run-phase0-codex-evaluation /path/to/mini-redis 1
```

It obtains the actual structured `context_build` packet at the comparison's
base and changed revisions and supplies that evidence to an ephemeral read-only
Codex process with runtime tools disabled. It rejects tool events, validates
every cited evidence identifier against a packet item, and checks the response
against the manifest's versioned agent contract. Each model run has fixed time
and output limits.

## Initial tasks

The manifest defines:

- current-revision evidence retrieval for negative RESP bulk-length validation;
- a cross-module graceful-shutdown trace;
- current-revision evidence retrieval for SET expiration encoding;
- a source-only revalidation scenario for the SET expiration fix;
- a source-plus-regression-test revalidation scenario for negative bulk-length
  validation; and
- a controlled temporal-decision comparison across the negative bulk-length
  fix.

The lexical/source-only and naive-memory-text baselines use the same corpus revision and task wording.

## Current execution coverage

The pinned runner records environment identity, cold and unchanged indexing,
peak RSS, repository/file/byte/fact counts, syntax errors, exact artifact
reuse, one-file invalidation, query p50/p95, result size, database/WAL size,
generation identity, and default MCP tool count. It resolves all nine required
evidence occurrences, writes and separately approves one canonical decision,
proves current recall and source-plus-memory context, changes its exact source,
then proves stale recall and stale-memory context exclusion.

The
[clean Ubuntu 24.04 attestation](../../docs/research/phase0-clean-benchmark-attestation-2026-07-29.md)
passes every ratified numeric ceiling. The earlier
[2026-07-28 provisional run](../../docs/research/phase0-product-benchmark-2026-07-28.md)
records the development baseline. The
[controlled comparative evaluation](../../docs/research/phase0-comparative-evaluation-2026-07-28.md)
also passes: lexical evidence changes with the source, naive memory continues
to expose the obsolete claim, and RepoWitness marks it stale and excludes it.
The [Codex utility evaluation](../../docs/research/phase0-codex-utility-evaluation-2026-07-28.md)
passes three paired runs: every decision is correct and source-grounded,
current memory is used, stale memory is ignored, and every packet is rated
useful. A real design-partner engineering-decision comparison remains.

## Budgets

Correctness budgets are zero-tolerance for false confirmed claims, silent
truncation, mixed-generation reads, and false automatic relinks. The resource
and latency budgets are ratified for this versioned profile after the clean
exact-revision release-platform run. Their broad margins on this small corpus
do not establish scaling behavior. A corpus, workload, semantics-affecting
configuration, or budget change requires a new review and clean attestation.

## Provenance

- Repository: <https://github.com/tokio-rs/mini-redis.git>
- Pinned branch observation: `refs/heads/master`
- Revision verified: 2026-07-23
- License: MIT
- Reuse mode: external reference only; no upstream content is incorporated into RepoWitness
