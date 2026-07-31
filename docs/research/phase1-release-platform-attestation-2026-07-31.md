# Phase 1 release-platform attestation

- Status: Completed
- Observed: 2026-07-31 UTC
- Workflow run:
  [`30622656120`](https://github.com/mqasimca/repowitness/actions/runs/30622656120)
- RepoWitness revision: `b34f2520132c54b97fd2969668d6bc48516643c0`
- Corpus: `tokio-rs/mini-redis`
- Corpus revision: `3d93b42bc363220f85af4fc9e1bebd35b588a4a3`
- Benchmark manifest:
  [`../../benchmarks/phase1/manifest.json`](../../benchmarks/phase1/manifest.json)

## Attestation

The manual `Phase 1 benchmark` workflow ran from `refs/heads/main` on Ubuntu
24.04. It checked out the exact RepoWitness revision, verified a clean
worktree, fetched only the allow-listed public corpus commit at depth one, and
completed successfully.

GitHub retained artifact
`phase1-benchmark-b34f2520132c54b97fd2969668d6bc48516643c0-1`. The artifact
API reported archive digest
`sha256:4c96cd94dcacb264a5e6f7dfe5f78ad528ac5d3178fc85e130b87db2ee5f7e16`
and retention through 2026-10-29 UTC. After download, `sha256sum -c
SHA256SUMS` verified the retained attestation, benchmark output, and manifest.
`scripts/check-phase1-benchmark-receipt` accepted the output for the exact
RepoWitness revision.

## Results

| Metric | Result | Ratified ceiling |
|---|---:|---:|
| Cold full index | 754.843 ms | 10,000 ms |
| Peak process RSS | 18,736 KiB | 384 MiB |
| Warm/native graph read p95 | 108.849 ms | 500 ms |
| Quiet-poll p95 | 751.043 ms | 1,000 ms |
| Retention plan p95 | 5.776 ms | 1,000 ms |
| Retention apply p95 | 107.731 ms | 2,000 ms |
| SQLite database | 5,292,032 bytes | 16,777,216 bytes |
| SQLite WAL after completion | 0 bytes | 0 bytes |

The benchmark reported `correctness_failures=0`, completed all eight declared
workloads, and completed all 12 adversarial release-matrix cases with zero
failures. Its receipt confirmed the exact pinned repository and corpus
revisions and the bounded zero-WAL outcome.

## Scope

This is retained release-platform evidence for the current Phase 1 revision.
It supports, but did not itself make, the individual maintainer decisions that
accepted the Phase 1 ADRs and ratified the associated budgets.
