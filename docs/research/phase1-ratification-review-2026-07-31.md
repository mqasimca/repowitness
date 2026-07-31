# Phase 1 ratification review

- Status: Recommendation
- Reviewed: 2026-07-31 UTC
- Scope: ADR-0025 through ADR-0034 and the Phase 1 benchmark profile

## Review boundary

This review assesses whether the available Phase 1 evidence supports maintainer
acceptance. It does not change an ADR or budget status. Acceptance requires
explicit maintainer action.

The current merge commit has a clean Ubuntu 24.04 release-platform benchmark
attestation, current macOS 15 and Windows 2025 CI, and a clean version-2 Codex
evaluator result. The evidence supports individual maintainer decisions; it
does not change an ADR or budget status by itself.

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
- A checksum-verified clean Ubuntu 24.04 benchmark at `b34f252`, with all
  eight workloads, all 12 adversarial cases, and zero correctness failures:
  [release attestation](phase1-release-platform-attestation-2026-07-31.md).
- Successful full-workspace, all-feature, macOS 15, Windows 2025, and Linux CI
  jobs at `b34f252`:
  [CI run](https://github.com/mqasimca/repowitness/actions/runs/30622582139).
- Full local validation of the current working tree: `make ci`, `make
  test-all`, the opt-in SQLite resource probe, fuzz-crate compilation and
  dependency policy, vendored-grammar integrity/regeneration, benchmark and
  documentation checks, and the Phase 1 evaluator self-tests.
- The scoped dependency review for configuration and filesystem-boundary
  additions: [review](phase1-configuration-dependency-review-2026-07-29.md).

## Findings

The implementation and evidence support ratification of the Phase 1 contracts
except for ADR-0030's deliberately name-only compatibility profile. That ADR
has no request, response, or behavior compatibility evidence and remains out
of the ratification set unless maintainers elect a separately measured scope.

## ADR recommendations

| ADR | Recommendation | Reason |
|---|---|---|
| [0025](../adr/0025-versioned-local-configuration-and-policy.md) | Ready | Current release benchmark and portable CI evidence pass. |
| [0026](../adr/0026-connected-workspace-source-slots-and-views.md) | Ready | Migration, source-view, retention, and release evidence pass. |
| [0027](../adr/0027-phase1-rust-syntax-graph.md) | Ready | Native graph, version-2 evaluator, release benchmark, and portable CI evidence pass. |
| [0028](../adr/0028-reconciliation-watching-and-source-epochs.md) | Ready | Watcher, shutdown, recovery, and release evidence pass. |
| [0029](../adr/0029-bounded-generation-retention-and-garbage-collection.md) | Ready | Retention and resource-budget evidence pass. |
| [0030](../adr/0030-bounded-incumbent-mcp-compatibility.md) | Exclude | Retain the deliberate name-only profile; do not claim unmeasured compatibility levels. |
| [0031](../adr/0031-source-slot-selectors-and-package-scopes.md) | Ready | Selector, package-scope, platform, and resource evidence pass. |
| [0032](../adr/0032-explicit-connected-workspace-manifest.md) | Ready | Its source-slot and connected-workspace prerequisites now pass. |
| [0033](../adr/0033-bounded-mutation-outcome-resolution.md) | Ready | The outcome grace is covered by the release and portable evidence. |
| [0034](../adr/0034-phase1-codex-graph-evaluation.md) | Ready | Version-2 fixtures, clean isolated pairs, and release evidence pass. |

The ratified budgets are supported by the retained Ubuntu receipt and current
portable CI, which establish the declared Phase 1 gate. Future revisions still
require their own evidence.

## Recommended completion sequence

1. Retain the release receipt, checksum set, and current portable-CI evidence.
2. Keep ADR-0030 name-only and outside the ratification set unless maintainers
   select a separately measured compatibility scope.
3. Make explicit individual maintainer decisions on ADR-0025 through ADR-0029
   and ADR-0031 through ADR-0034, plus their associated budgets.

## Maintainer decision required

Maintainers accepted ADR-0025 through ADR-0029 and ADR-0031 through ADR-0034
individually, and ratified their associated budgets. ADR-0030 remains proposed
and explicitly name-only unless a later measured compatibility decision
supersedes it.
