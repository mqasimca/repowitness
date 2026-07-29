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
- the exact opt-in compatibility alias inventory and schema receipts; and
- deterministic retention plan, stale-plan rejection, apply, restart, and
  no-op replay.

Every workload records aggregate counts and timings only. Normal output must
not contain source text, symbol names outside the public corpus oracle,
worktree paths, raw selector text, environment values, or credentials.

Run it against a clean checkout of the manifest-pinned public corpus:

```text
./scripts/run-phase1-benchmark /path/to/public-corpus
```

The runner first indexes two disposable worktrees through the public CLI,
prebuilds its release-mode test executables, verifies one warmup per workload,
and then repeats focused regression workloads. Focused test workloads verify
their expected executed-test counts, so an empty test filter cannot pass the
gate. Its p95 values measure the warmed complete test harness, so they are
useful for regression comparison but do not ratify the operation-level resource
budgets below. A ratification run must use an exact-revision operation probe
and record its clean environment attestation separately.

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
