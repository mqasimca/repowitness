# Phase 0 product-loop benchmark

- Status: Provisional measurement
- Observed: 2026-07-28 UTC
- Corpus: `tokio-rs/mini-redis`
- Corpus revision: `3d93b42bc363220f85af4fc9e1bebd35b588a4a3`
- Benchmark manifest:
  [`../../benchmarks/phase0/manifest.json`](../../benchmarks/phase0/manifest.json)

## Scope

This run exercises the implemented Phase 0 product loop through public local
boundaries:

1. clone the clean pinned corpus into a disposable worktree;
2. create and atomically activate a cold SQLite index;
3. repeat unchanged indexing and verify exact artifact reuse;
4. resolve every required manifest evidence target through bounded search;
5. measure repeated warm query latency;
6. compile a source-only context pack;
7. write and locally approve one canonical decision record;
8. revalidate, recall, and compile source-plus-current-memory context;
9. change one attached declaration in the disposable clone;
10. reindex one changed file, revalidate the memory as stale, and prove stale
    memory is excluded from context; and
11. negotiate local stdio MCP, list the default read-only tools, and retrieve
    exact material evidence.

The runner does not modify the supplied corpus checkout. All source mutation,
memory files, and SQLite files live in a disposable clone and are removed when
the runner exits.

## Environment

| Field | Observed value |
|---|---|
| RepoWitness revision | `809d825fad7a2884698168ffc93e7200a9c63cc2`, dirty working tree |
| Toolchain | pinned repository Rust toolchain, release profile, locked dependency graph |
| Operating system | Darwin 25.5.0 arm64 |
| CPU | Apple M4 Pro |
| Logical CPUs | 14 |
| Memory | 24,576 MiB |
| Corpus filesystem | APFS |
| Repeated warm queries | 5 |
| Configuration digest | `5c7c3e4bf8e03ffd6680ccdfe3b2bfc464c535c2cc77cf430efffd8acbd5108a` |

Because the user required that the implementation remain uncommitted, this is
development evidence rather than a clean-revision release attestation.

## Results

| Metric | Result | Proposed ceiling |
|---|---:|---:|
| Cold full index | 451.160 ms | 10,000 ms |
| Unchanged warm index | 293.652 ms | 10,000 ms |
| Warm query p50 | 1.336 ms | — |
| Warm query p95 | 1.481 ms | 250 ms |
| Peak process RSS | 11,904 KiB | 256 MiB |
| SQLite database | 544,768 bytes | — |
| SQLite WAL after completion | 0 bytes | — |
| MCP material result | 3,620 bytes | 49,152 bytes |
| Canonical memory record | 1,408 bytes | record-format bound |

The corpus result contained 34 repository paths, 28 Rust files, 149,820 exact
source bytes, 206 facts, and zero Tree-sitter syntax-error nodes. All nine
required evidence occurrences were retrieved.

The unchanged generation reused 28 of 28 artifacts and analyzed none. After
one contained source edit, the next generation reused 27 of 28 and analyzed
only the changed file. The source generations advanced from 1 to 2 to 3.

Before the edit, the approved record was recalled as current with exact
evidence and was eligible for context. After the edit, it was recalled as
stale with changed evidence and was explicitly excluded from context. The
default MCP server listed exactly five read-only tools; write-capable
`memory_manage` remained disabled.

The run reported zero false confirmed claims, silent truncations,
mixed-generation reads, or false automatic relinks.

## Interpretation

This development run passes every numerical ceiling currently proposed by the
manifest and demonstrates the complete source-change-to-stale-context loop.
It does not ratify those budgets: the manifest and its budget block remain
`proposed`, and ADR-0017 through ADR-0019 and ADR-0021 remain proposed.

It also does not satisfy the separate design-partner outcome gate. The
disposable edit is a deterministic correctness scenario, not evidence that
memory changed a real engineering decision relative to the lexical/source-only
and naive-memory baselines.

## Reproduction

Use a clean external checkout at the pinned revision:

```text
./scripts/run-phase0-benchmark /path/to/mini-redis 5
```

The runner rejects a wrong revision or dirty source checkout, clones with
`--no-local`, builds release binaries with the locked dependency graph,
captures environment and peak RSS, enforces the proposed resource ceilings,
and removes its disposable worktree and database on exit.

For a release attestation, rerun from an exact clean RepoWitness revision after
maintainer review, then explicitly ratify or revise the manifest and budgets.
