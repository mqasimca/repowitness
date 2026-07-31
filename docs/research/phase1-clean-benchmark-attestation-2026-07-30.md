# Phase 1 clean benchmark attestation

- Status: Completed
- Observed: 2026-07-30 UTC
- Workflow run:
  [`30585411303`](https://github.com/mqasimca/repowitness/actions/runs/30585411303)
- RepoWitness revision: `006197af6bc2a43d77cfa94c0b599b2e28d67704`
- Corpus: `tokio-rs/mini-redis`
- Corpus revision: `3d93b42bc363220f85af4fc9e1bebd35b588a4a3`
- Benchmark manifest:
  [`../../benchmarks/phase1/manifest.json`](../../benchmarks/phase1/manifest.json)

## Attestation

The manual `Phase 1 benchmark` workflow ran from `refs/heads/main` on Ubuntu
24.04. It checked out the exact RepoWitness revision, verified a clean
worktree, fetched only the allow-listed public corpus commit at depth one, and
completed successfully.

GitHub retained one artifact named
`phase1-benchmark-006197af6bc2a43d77cfa94c0b599b2e28d67704-1`. The artifact
API reported archive digest
`sha256:137d9d6261ef68aa3e579d4cd472324e848de507d71556b8a40e162a50a7e45e`
and retention through 2026-10-28 UTC. After download, `sha256sum -c
SHA256SUMS` verified the retained attestation, benchmark output, and manifest.
`scripts/check-phase1-benchmark-receipt` accepted the output for the exact
RepoWitness revision.

## Results

| Metric | Result | Ceiling at run time |
|---|---:|---:|
| Cold full index | 686.636 ms | 10,000 ms |
| Peak process RSS | 18,932 KiB | 384 MiB |
| Warm/native graph read p95 | 120.328 ms | 500 ms |
| Quiet-poll p95 | 751.029 ms | 1,000 ms |
| Retention plan p95 | 8.621 ms | 1,000 ms |
| Retention apply p95 | 34.445 ms | 2,000 ms |
| SQLite database | 5,292,032 bytes | 16,777,216 bytes |
| SQLite WAL after completion | 0 bytes | 0 bytes |

The benchmark reported `correctness_failures=0` and completed all eight
declared workloads. Its receipt confirmed the exact pinned repository and
corpus revisions and the bounded zero-WAL outcome.

## Repeated run

A second independent clean workflow run,
[`30586880908`](https://github.com/mqasimca/repowitness/actions/runs/30586880908),
also completed successfully for the same exact RepoWitness revision and
manifest. Its retained artifact reported archive digest
`sha256:32b741199181801a2badf60a7478afda7d9c5ee366e0ec9a7178871b98ce8809`
and retention through 2026-10-28 UTC. Its checksum set and exact receipt
validator both passed.

The repeated run reported a 696.198 ms cold index, 18,460 KiB peak RSS,
123.982 ms warm/native-graph p95, 751.092 ms quiet-poll p95, 5,292,032 bytes
of database storage, zero WAL bytes, and zero correctness failures. It also
completed all eight declared workloads.

## Scope and remaining gate

These are two clean release-platform attestations for the proposed Phase 1
profile. They do not ratify the profile, resource budgets, migration 3, or any
proposed ADR. Ratification still requires repeated clean evidence, the complete
adversarial matrix, repeated isolated Codex evaluation of the actual MCP graph
packet, and maintainer review.
