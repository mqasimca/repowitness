# Phase 1 clean local benchmark

- Status: Completed local evidence; release-platform attestation pending
- Observed: 2026-07-31 UTC
- RepoWitness revision: `c61de8d79050cba96799cc27fa1d18299bc1478d`
- Corpus: `tokio-rs/mini-redis`
- Corpus revision: `3d93b42bc363220f85af4fc9e1bebd35b588a4a3`
- Benchmark manifest:
  [`../../benchmarks/phase1/manifest.json`](../../benchmarks/phase1/manifest.json)

## Method

The full runner started from a clean exact RepoWitness worktree and a clean
detached checkout of the manifest-pinned public corpus. It ran all twelve
adversarial release-matrix cases before the operation probe and all eight
declared workloads. The runner's final receipts verified the unchanged
RepoWitness revision and worktree, the pinned clean corpus revision, and the
clean disposable worktrees.

This Linux host does not provide `/usr/bin/time`. To preserve the runner's
fixed-path contract without modifying the host or project, the run used GNU
Time 1.9 in an unprivileged temporary mount namespace at that path. The source
archive's detached signature verified against the GNU project keyring. This is
local evidence only: it has no retained release-platform artifact or checksum
set and does not replace the required supported-platform attestation.

## Results

| Metric | Result | Ceiling at run time |
|---|---:|---:|
| Cold full index | 305.339 ms | 10,000 ms |
| Peak process RSS | 19,068 KiB | 384 MiB |
| Warm/native graph read p95 | 55.794 ms | 500 ms |
| Quiet-poll p95 | 751.029 ms | 1,000 ms |
| Retention plan p95 | 2.635 ms | 1,000 ms |
| Retention apply p95 | 10.061 ms | 2,000 ms |
| SQLite database | 5,292,032 bytes | 16,777,216 bytes |
| SQLite WAL after completion | 0 bytes | 0 bytes |

The operation probe reported `correctness_failures=0`, and the adversarial
matrix reported 12 cases with zero failures. Each declared workload passed:
atomic two-source publication, two worktrees for one repository, quiet polling,
moving-selector final fencing, package-scope clean/incremental equivalence,
native graph reads, compatibility schemas, and retention restart.

## Scope and remaining gate

This run proves the current exact revision satisfies the complete Phase 1
benchmark locally. It does not ratify budgets or proposed ADRs. Retained
supported-platform evidence and explicit maintainer decisions remain required.
