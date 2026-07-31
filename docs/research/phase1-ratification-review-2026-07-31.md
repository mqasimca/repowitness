# Phase 1 ratification review

- Status: Recommendation
- Reviewed: 2026-07-31 UTC
- Scope: ADR-0025 through ADR-0034 and the proposed Phase 1 benchmark profile

## Review boundary

This review assesses whether the available Phase 1 evidence supports maintainer
acceptance. It does not change an ADR or budget status. Acceptance requires
explicit maintainer action.

The two clean Ubuntu 24.04 benchmark attestations are for committed revision
`006197af6bc2a43d77cfa94c0b599b2e28d67704`. The current implementation and
this review include uncommitted changes, including the evaluator-only graph
envelope. Therefore, neither those changes nor ADR-0034 have a clean
exact-revision release attestation yet.

## Evidence considered

- Two checksum-verified clean Phase 1 benchmark runs at the same committed
  revision, with all eight declared workloads and zero correctness failures:
  [attestation](phase1-clean-benchmark-attestation-2026-07-30.md).
- Three isolated base/changed Codex pairs that cited generation-pinned graph and
  source evidence, used current memory only at the base revision, cited no
  memory at the changed revision, and emitted no tool events:
  [evaluation](phase1-codex-graph-evaluation-2026-07-30.md).
- The bounded 12-case Phase 1 adversarial matrix in release mode, covering
  migration/recovery, retention, source fencing, watcher/configuration,
  mutation outcomes, compatibility, and shutdown:
  [local matrix result](phase1-adversarial-release-matrix-2026-07-31.md).
- Successful full-workspace, all-feature, macOS 15 and Windows 2025 CI jobs at
  the current committed base revision:
  [portable-core validation](phase1-portable-core-validation-2026-07-30.md).
- Full local validation of the current working tree: `make ci`, `make
  test-all`, the opt-in SQLite resource probe, fuzz-crate compilation and
  dependency policy, vendored-grammar integrity/regeneration, benchmark and
  documentation checks, and the Phase 1 evaluator self-tests.
- The scoped dependency review for configuration and filesystem-boundary
  additions: [review](phase1-configuration-dependency-review-2026-07-29.md).

## Findings

The implementation and local evidence are strong enough to continue the Phase
1 release-candidate process. They are not sufficient to ratify the Phase 1
profile as a whole:

- The clean release-platform evidence does not include the uncommitted graph
  envelope/evaluator changes.
- macOS and Windows full-workspace CI passed for the current committed base,
  but it predates the uncommitted evaluator/matrix scripts and does not retain
  a platform-specific release receipt or resource measurement.
- The integrated adversarial matrix has a passing local release-mode result,
  but it is not yet a clean exact-revision GitHub attestation or the required
  supported-platform evidence.
- The incumbent aliases deliberately claim name compatibility only, so their
  request, response, and behavior compatibility has not been measured.
- ADR-0034 now has a versioned envelope schema, canonical golden fixture, and
  hostile-input self-tests, but the current implementation still lacks a clean
  exact-revision release attestation.

## ADR recommendations

| ADR | Recommendation | Reason |
|---|---|---|
| [0025](../adr/0025-versioned-local-configuration-and-policy.md) | Hold | macOS/Windows all-feature CI passes at the base revision, but the current worktree lacks clean exact-revision platform evidence. |
| [0026](../adr/0026-connected-workspace-source-slots-and-views.md) | Hold | The migration-3/workspace-view contract needs its complete graph, retention, release, and platform evidence. |
| [0027](../adr/0027-phase1-rust-syntax-graph.md) | Hold | Native graph reads and the Codex evaluation pass, but clean evidence for the current evaluator plus the connected-workspace/migration decision is missing. |
| [0028](../adr/0028-reconciliation-watching-and-source-epochs.md) | Hold | Portable CI covers the committed core, but the release gate still requires current exact-revision watcher, shutdown, and recovery evidence. |
| [0029](../adr/0029-bounded-generation-retention-and-garbage-collection.md) | Hold | Default retention floor and resource budgets remain proposed pending release evidence. |
| [0030](../adr/0030-bounded-incumbent-mcp-compatibility.md) | Hold | The implementation intentionally claims names only; compatibility levels have not been independently measured. |
| [0031](../adr/0031-source-slot-selectors-and-package-scopes.md) | Hold | The committed portable core passes, but current exact-revision platform and resource-budget evidence remains a stated gate. |
| [0032](../adr/0032-explicit-connected-workspace-manifest.md) | Hold with the connected-workspace cluster | Its boundary implementation passes locally, but its prerequisite source-slot/migration contracts remain proposed. |
| [0033](../adr/0033-bounded-mutation-outcome-resolution.md) | Hold | The 250-millisecond outcome-resolution grace needs supported-release-platform measurement before acceptance. |
| [0034](../adr/0034-phase1-codex-graph-evaluation.md) | Hold | The local schema and fixture contract now passes, but a committed clean evaluation run and release evidence are still needed. |

No proposed Phase 1 budget should be ratified at this time. The observed
Ubuntu resource margins are useful evidence, not multi-platform capacity or
stability proof.

## Recommended completion sequence

1. Review, commit, and cleanly attest the current evaluator/envelope changes on
   the release platform; verify checksums and receipts as for the prior Phase 1
   runs.
2. Execute the integrated Phase 1 adversarial release matrix from the clean
   exact revision and collect the required supported-platform outcomes for
   migration-3 upgrade/recovery, retention roots and restart, source-change
   fencing, mutation-outcome resolution, and incumbent-alias boundaries.
3. Rerun portable CI after the clean commit and collect the required
   supported-platform evidence for configuration, selectors/path behavior,
   watcher cancellation/shutdown, and outcome grace.
4. Either measure and document a deliberately bounded incumbent compatibility
   level or keep ADR-0030 name-only and out of the ratification set.
5. Rerun ADR-0034's three isolated pairs from the clean attested revision,
   retaining its versioned envelope schema and focused golden/adversarial
   fixture checks.
6. Return to this review for explicit maintainer decisions on the individual
   ADRs and budgets; do not accept the group as one undifferentiated change.

## Maintainer decision required

The immediate decision is to retain every Phase 1 ADR and budget as proposed
while completing the sequence above. A later maintainer decision may accept
individual contracts only when their stated evidence gates are complete.
