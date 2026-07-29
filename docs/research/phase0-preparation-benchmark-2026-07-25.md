# Phase 0 local Rust preparation benchmark

- Status: Provisional measurement
- Observed: 2026-07-26 00:25:39 UTC
- Corpus: `tokio-rs/mini-redis`
- Corpus revision: `3d93b42bc363220f85af4fc9e1bebd35b588a4a3`
- Benchmark manifest: [`../../benchmarks/phase0/manifest.json`](../../benchmarks/phase0/manifest.json)

This historical preparation-only measurement is retained for comparison. The
current full-loop evidence is the
[Phase 0 product benchmark](phase0-product-benchmark-2026-07-28.md).

## Scope

This is the first measurement of the implemented local preparation slice:

1. resolve and sanitize Git worktree discovery;
2. discover tracked and untracked repository paths within fixed limits;
3. select case-sensitive `.rs` paths;
4. open every selected file through a no-follow directory capability;
5. hash exact source bytes and extract bounded direct-syntax facts;
6. build canonical source-manifest and artifact-key digests;
7. rediscover the path set and reread every selected source to reject changes
   observed during preparation.

It does not measure SQLite staging/activation, FTS5, retrieval, memory
revalidation, context compilation, MCP transport, or query latency. It
therefore cannot ratify the full-index or warm-query budgets.

## Environment

| Field | Observed value |
|---|---|
| RepoWitness revision | `be569b28025138123e0fdfc94cdbbe5289d33b64`, dirty working tree |
| Toolchain | `rustc 1.97.1 (8bab26f4f 2026-07-14)` |
| Operating system | Linux 7.1.4-1-cachyos x86_64 |
| CPU | AMD Ryzen 9 9950X3D 16-Core Processor |
| Logical CPUs | 32 |
| Memory | 61,937 MiB |
| Corpus filesystem | `tmpfs` |
| Build | Cargo release profile, locked dependency graph |
| Runs | 1 first-process preparation plus 9 warm preparations |
| Configuration digest | `2222222222222222222222222222222222222222222222222222222222222222` |

The configuration digest is a deterministic probe identity, not a ratified
production profile. Because the RepoWitness worktree was intentionally not
committed, these results are engineering evidence rather than release
attestation.

## Results

| Metric | Result |
|---|---:|
| First-process preparation | 20.031 ms |
| Warm p50 | 17.850 ms |
| Warm p95 | 18.280 ms |
| Process high-water RSS | 5,544 KiB |
| Discovered repository paths | 34 |
| Selected Rust files | 28 |
| Explicitly skipped non-Rust paths | 6 |
| Exact Rust source bytes | 149,820 |
| Extracted symbol facts | 206 |
| Tree-sitter syntax-error nodes | 0 |

All ten runs produced the same canonical snapshot digest:

```text
5daa46e50b5834a0a8e5e1e4903cbb07b58f39cd22a8bbcdfa5ea46720e251b6
```

The preparation-only wall time and process RSS are far below the manifest's
proposed full-index ceilings. This run intentionally excludes persistence,
retrieval, and MCP, so it cannot by itself pass the full-index gate.

## Reproduction

Use a clean external checkout at the pinned revision:

```text
./scripts/run-phase0-benchmark /path/to/mini-redis
```

The runner rejects the wrong corpus revision or a dirty corpus. It records the
RepoWitness revision and dirty state, verifies repeated logical equivalence,
and does not write to the corpus.

## Follow-up status and next measurement

The accepted SQLite v3 schema, exact reuse, evidence retrieval, and local stdio
MCP are now implemented. End-to-end persistence, repeat indexing, search, and
exact symbol retrieval pass on two neighboring cloned Rust repositories. That
integration evidence does not replace the pinned-corpus measurement because it
does not use the manifest's fixed revision, workload, or proposed budgets.

Extend the pinned runner through the implemented stages and record:

- clean database creation and atomic activation;
- warm no-change artifact reuse;
- one-file incremental reanalysis;
- database and WAL bytes;
- peak RSS and writer-queue high-water marks;
- FTS5 query p50/p95 and bounded result bytes;
- projection rebuild time;
- crash/cancellation preservation of the previous active generation; and
- installed CLI and stdio MCP logical equivalence.
