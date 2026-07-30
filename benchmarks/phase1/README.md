# Phase 1 trustworthy local-core benchmark

- Status: Proposed
- Last reviewed: 2026-07-29
- Manifest: [`manifest.json`](manifest.json)

## Purpose

This gate extends the ratified Phase 0 product loop with the Phase 1 trust
boundaries: immutable connected-workspace views, exact source selectors,
explicit package scopes, polling reconciliation, Rust syntax-graph reads,
bounded retention, and the opt-in compatibility surface.

The benchmark uses only the externally referenced public corpus declared in
the manifest. It does not vendor corpus source, tests, fixtures, or generated
content. Multi-source cases use separate disposable worktrees of that same
public corpus so the gate can exercise source-slot identity without depending
on any maintainer-local repository.

## Workloads

The local runner exercises:

- clean and unchanged indexing;
- an atomic two-source workspace publication and a same-repository,
  two-worktree publication;
- selector capture and final-fence rejection after a moving-ref change;
- whole-repository and explicit package-scope reconciliation;
- quiet polling cycles that publish no new epoch or generation;
- bounded native graph status, search, evidence, architecture, trace, and
  impact reads;
- the exact opt-in compatibility alias inventory, local `tools/list` golden,
  conservative name-only receipts, and all-alias boundary rejection; and
- deterministic retention plan, stale-plan rejection, apply, restart, and
  no-op replay, including direct persisted-root and transactional-rollback
  regressions.

Every workload records aggregate counts and timings only. Normal output must
not contain source text, symbol names outside the public corpus oracle,
worktree paths, raw selector text, credentials, secrets, or arbitrary
environment variables. It does contain only the allow-listed aggregate
platform and toolchain fields declared by the manifest.

Run it against a clean checkout of the manifest-pinned public corpus:

```text
./scripts/run-phase1-benchmark /path/to/public-corpus
```

The runner indexes two disposable worktrees through the public CLI into a new
database for every warmup and measured publication sample, and runs a
release-mode operation probe against a separate fresh database. The probe
measures complete quiet reconciliation sessions, all six native graph read
operations, and real retention plan/apply cycles. The native graph suite is
the declared warm-query workload: its run count and latency budget must match
the manifest's warm-query contract, and its receipt must match the native
graph receipt exactly. Each workspace publication must also report complete
post-commit maintenance and remain within the database and zero-WAL budgets.
Its fresh database directory is removed immediately after those checks. Quiet
polling uses a fixed 750-millisecond manifest session, leaving a separate
1000-millisecond p95 ceiling. The session must admit one complete startup
reconciliation; quiet and graph latency gates compare their calculated p95
rather than mistaking the budget for a per-sample maximum.
The retention workload adds one bounded RepoWitness-authored Rust source to
the disposable checkout and changes it once per generation so collection has
real unreachable work. It removes that source on success and failure, and the
runner's final clean-worktree fence rejects incomplete cleanup.
The runner executes same-revision regressions that prove missing history stays
indeterminate, ambiguous correspondence does not auto-link, and truncation is
reported before it emits the corresponding zero-tolerance counters. It fails
when a declared
full-index wall, peak-resident-memory, graph-output, database-size,
post-completion-WAL, operation-latency, retention, or correctness budget is
exceeded. Graph receipts state the enforced material-output bound; they do not
misrepresent unavailable per-result accounting as a measured evidence size.
The runner also repeats focused release-mode regression workloads; those
timings use a monotonic clock and are labelled
`measurement=test-harness` and are comparison evidence, not operation latency.
Every focused workload verifies its executed-test count, so an empty filter
cannot pass the gate. The operation probe's percentile, digest, storage, and
warm-query contract unit tests also run explicitly. Every material build,
test, Git, environment-probe, and workload subprocess has a bounded combined
capture and an absolute monotonic deadline. Local receipt parsers and small
text utilities consume only those already bounded files.

The manual `Phase 1 benchmark` GitHub Actions workflow checks out one exact
`main` revision, fetches only the allow-listed corpus's pinned commit with
depth one, requires both standalone depth-one disposable worktrees to be clean,
records the declared environment fields, validates the complete receipt against an exact
allow-list, rechecks the final repository and corpus revisions and statuses,
and retains checksummed evidence. The local runner also verifies both
disposable corpus worktrees at setup and after all workloads. A result becomes
an attestation only after that workflow completes successfully for a committed
exact revision.

`scripts/check-benchmarks` also runs isolated receipt-parser and bounded-capture
regressions. These cover duplicate, missing, unavailable, over-budget, and
unexpected receipt data, plus capture overflow, deadline, and child-failure
handling without running the corpus workload.

## Budgets

Correctness budgets are zero-tolerance. Resource budgets are proposed before
the first ratification run and remain deliberately broad until repeated clean
release-platform evidence exists. Ratification requires the full adversarial
test matrix plus repeated isolated Codex runs that judge the actual MCP graph
packet useful and cite its evidence identifiers.

The benchmark is not a scale claim. Changing the corpus, workload,
semantics-affecting configuration, or a numeric budget requires a new review
and clean attestation.

## Provenance

- Repository: <https://github.com/tokio-rs/mini-redis.git>
- Pinned branch observation: `refs/heads/master`
- Revision verified: 2026-07-23
- License: MIT
- Reuse mode: external reference only; no upstream content is incorporated
  into RepoWitness
