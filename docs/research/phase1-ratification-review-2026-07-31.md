# Phase 1 ratification review

- Status: Recommendation
- Reviewed: 2026-07-31 UTC
- Scope: ADR-0025 through ADR-0034 and the proposed Phase 1 benchmark profile

## Review boundary

This review assesses whether the available Phase 1 evidence supports maintainer
acceptance. It does not change an ADR or budget status. Acceptance requires
explicit maintainer action.

The two clean Ubuntu 24.04 benchmark attestations are for committed revision
`006197af6bc2a43d77cfa94c0b599b2e28d67704`, which predates the evaluator-only
graph-envelope work committed in `f911d1f`. The isolated version-2 evaluator
has since passed at clean revision `ebd329e`, but the complete current Phase 1
profile does not have a clean exact-revision release-platform attestation yet.

## Evidence considered

- Two checksum-verified clean Phase 1 benchmark runs at the same committed
  revision, with all eight declared workloads and zero correctness failures:
  [attestation](phase1-clean-benchmark-attestation-2026-07-30.md).
- Three historical base/changed Codex pairs that cited graph and source
  evidence, used current memory only at the base revision, cited no memory at
  the changed revision, and emitted no tool events. They used the superseded
  version-1 envelope and must be repeated under version 2:
  [evaluation](phase1-codex-graph-evaluation-2026-07-30.md).
- A subsequent local repeat of those three pairs using the version-2 envelope
  passed every decision, source-grounding, memory-use, stale-memory, and tool
  event check. It ran from the current dirty working tree, so it is useful
  implementation evidence but not a clean exact-revision attestation:
  [evaluation](phase1-codex-graph-evaluation-2026-07-30.md).
- Three clean exact-revision version-2 base/changed pairs at `ebd329e`, with
  the same required decisions, evidence use, stale-memory exclusion, and zero
  tool events: [evaluation](phase1-codex-graph-evaluation-2026-07-30.md).
- The bounded 12-case Phase 1 adversarial matrix in release mode, covering
  migration/recovery, retention, source fencing, watcher/configuration,
  mutation outcomes, compatibility, and shutdown:
  [local matrix result](phase1-adversarial-release-matrix-2026-07-31.md).
- A clean exact-revision local full benchmark at `c61de8d`, with all eight
  workloads, the adversarial matrix, resource budgets, and final source/corpus
  integrity receipts passing: [local benchmark](phase1-clean-local-benchmark-2026-07-31.md).
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

- The clean release-platform evidence does not include the current version-2
  graph-envelope/evaluator implementation. The current exact revision passes
  both its isolated evaluator and full benchmark locally, but has no retained
  supported-platform receipt.
- macOS and Windows full-workspace CI passed for the current committed base,
  but it predates the current evaluator/matrix scripts and does not retain
  a platform-specific release receipt or resource measurement.
- The integrated adversarial matrix has a passing local release-mode result,
  including a clean exact-revision full benchmark, but it is not yet a retained
  supported-platform attestation.
- The incumbent aliases deliberately claim name compatibility only, so their
  request, response, and behavior compatibility has not been measured.
- ADR-0034 now has a versioned envelope schema, canonical golden fixture, and
  hostile-input self-tests and a clean exact-revision isolated-pair result, but
  the current implementation still lacks a full release-platform attestation.

## ADR recommendations

| ADR | Recommendation | Reason |
|---|---|---|
| [0025](../adr/0025-versioned-local-configuration-and-policy.md) | Hold | The current exact revision passes locally, but it lacks retained supported-platform evidence. |
| [0026](../adr/0026-connected-workspace-source-slots-and-views.md) | Hold | The migration-3/workspace-view contract needs its complete graph, retention, release, and platform evidence. |
| [0027](../adr/0027-phase1-rust-syntax-graph.md) | Hold | Native graph reads and clean local Codex evaluation pass, but supported-platform and connected-workspace evidence is missing. |
| [0028](../adr/0028-reconciliation-watching-and-source-epochs.md) | Hold | Portable CI covers the committed core, but the release gate still requires current exact-revision watcher, shutdown, and recovery evidence. |
| [0029](../adr/0029-bounded-generation-retention-and-garbage-collection.md) | Hold | Default retention floor and resource budgets remain proposed pending release evidence. |
| [0030](../adr/0030-bounded-incumbent-mcp-compatibility.md) | Hold | The implementation intentionally claims names only; compatibility levels have not been independently measured. |
| [0031](../adr/0031-source-slot-selectors-and-package-scopes.md) | Hold | The committed portable core passes, but current exact-revision platform and resource-budget evidence remains a stated gate. |
| [0032](../adr/0032-explicit-connected-workspace-manifest.md) | Hold with the connected-workspace cluster | Its boundary implementation passes locally, but its prerequisite source-slot/migration contracts remain proposed. |
| [0033](../adr/0033-bounded-mutation-outcome-resolution.md) | Hold | The 250-millisecond outcome-resolution grace needs supported-release-platform measurement before acceptance. |
| [0034](../adr/0034-phase1-codex-graph-evaluation.md) | Hold | The clean isolated evaluation and full local benchmark pass, but a retained release-platform attestation is still needed. |

No proposed Phase 1 budget should be ratified at this time. The observed
Ubuntu resource margins are useful evidence, not multi-platform capacity or
stability proof.

## Recommended completion sequence

1. Repeat the current clean local benchmark on the supported release platform;
   verify retained checksums and receipts as for the prior Phase 1 runs.
2. Collect the required supported-platform outcomes for the integrated matrix:
   migration-3 upgrade/recovery, retention roots and restart, source-change
   fencing, mutation-outcome resolution, and incumbent-alias boundaries.
3. Rerun portable CI after the clean commit and collect the required
   supported-platform evidence for configuration, selectors/path behavior,
   watcher cancellation/shutdown, and outcome grace.
4. Either measure and document a deliberately bounded incumbent compatibility
   level or keep ADR-0030 name-only and out of the ratification set.
5. Retain ADR-0034's clean isolated-pair result, versioned envelope schema,
   and focused golden/adversarial fixture checks with the release evidence.
6. Return to this review for explicit maintainer decisions on the individual
   ADRs and budgets; do not accept the group as one undifferentiated change.

## Maintainer decision required

The immediate decision is to retain every Phase 1 ADR and budget as proposed
while completing the sequence above. A later maintainer decision may accept
individual contracts only when their stated evidence gates are complete.
