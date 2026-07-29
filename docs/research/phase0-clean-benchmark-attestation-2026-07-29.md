# Phase 0 clean benchmark attestation

- Status: Completed
- Observed: 2026-07-29 UTC
- Workflow run:
  [`30412050919`](https://github.com/mqasimca/repowitness/actions/runs/30412050919)
- RepoWitness revision: `6b77374ec303e5be8ca62c3828d477b6868519d6`
- Corpus: `tokio-rs/mini-redis`
- Corpus revision: `3d93b42bc363220f85af4fc9e1bebd35b588a4a3`
- Benchmark manifest:
  [`../../benchmarks/phase0/manifest.json`](../../benchmarks/phase0/manifest.json)

## Attestation

The manual `Phase 0 benchmark` workflow ran from `refs/heads/main` on Ubuntu
24.04. It checked out the exact RepoWitness revision, verified a clean
worktree, obtained only the allow-listed public corpus at its full manifest
revision, and completed with exit code zero.

GitHub retained one artifact named
`phase0-benchmark-6b77374ec303e5be8ca62c3828d477b6868519d6-1`.
The artifact API reported archive digest
`sha256:a12da515bd29be82783fed7a1c9758d25efb5367f7ae1fb721b6e2f089d33428`
and retention through 2026-10-27 UTC. Its attestation reported
`attestation_valid=true`. After download, `sha256sum -c SHA256SUMS` verified
the retained attestation, benchmark output, and manifest.

## Environment

| Field | Observed value |
|---|---|
| Toolchain | `rustc 1.97.1 (8bab26f4f 2026-07-14)` |
| Operating system | Linux 6.17.0-1020-azure x86_64 |
| CPU | AMD EPYC 9V74 80-Core Processor |
| Logical CPUs | 4 |
| Memory | 15,989 MiB |
| Corpus filesystem | ext4 |
| Repeated warm queries | 10 |
| Configuration digest | `254fb904ed557bfcbc404b1f6bf582457190327e70f0bc49a6a4a7bbae754680` |

## Results

| Metric | Result | Ratified ceiling | Ceiling used |
|---|---:|---:|---:|
| Cold full index | 155.655 ms | 10,000 ms | 1.6% |
| Unchanged warm index | 93.465 ms | 10,000 ms | 0.9% |
| Warm query p50 | 2.148 ms | — | — |
| Warm query p95 | 2.204 ms | 250 ms | 0.9% |
| Peak process RSS | 12,112 KiB | 256 MiB | 4.6% |
| MCP material result | 3,620 bytes | 49,152 bytes | 7.4% |
| SQLite database | 524,288 bytes | 4,194,304 bytes | 12.5% |
| SQLite WAL after completion | 0 bytes | 0 bytes | pass |

The run retrieved all nine required evidence occurrences. Unchanged indexing
reused 28 of 28 Rust artifacts. After one contained source edit, indexing
reused 27 of 28 artifacts. Current memory was included before the edit; stale
memory was excluded afterward. The controlled comparison changed the
engineering decision with the source, while the naive memory baseline exposed
one stale claim.

The run reported zero correctness failures, false confirmed claims, silent
truncations, mixed-generation reads, false automatic relinks, and exposed stale
RepoWitness claims. The default MCP server kept memory writes disabled.

## Ratification decision

The clean exact-revision release-platform run satisfies the remaining
measurement prerequisite in the
[Phase 0 ratification review](phase0-ratification-review-2026-07-28.md).
Maintainer direction ratifies the versioned manifest and its correctness,
latency, RSS, result-size, database, and WAL budgets without changing any
threshold.

The retained artifact contains the proposed manifest exactly as executed.
Ratification changes only the manifest status, budget status, and review date;
the corpus, workload, run-environment contract, configuration inputs, and
numeric thresholds are unchanged.

This decision is bounded to the named corpus, workload, run environment
contract, and semantics-affecting configuration identity. The broad margins
are intentional for the small Phase 0 corpus and do not establish repository
scaling behavior. The separate real design-partner outcome remains required
before the Phase 0 milestone is complete.
