# Phase 1 adversarial release matrix

- Status: Provisional local evidence
- Observed: 2026-07-31 UTC
- Scope: Historical pre-merge local worktree on the local Linux release-test
  profile; rerun against the current exact revision before using it as release
  evidence

## Result

`./scripts/run-phase1-adversarial-matrix` completed all 12 exact regressions
with `--release --locked` and `--test-threads=1`:

| Category | Result |
|---|---|
| Version-2 to version-3 migration upgrade | Passed |
| Cancellation after committed migration | Passed |
| Atomic retention plan/apply/replay | Passed |
| Retention apply process-restart recovery | Passed |
| Moving source selector at final fence | Passed |
| Watcher overflow and unsupported hints | Passed |
| Configuration invalid-input rejection | Passed |
| Backup receipt resolution timeout | Passed |
| Unresolved mutation fencing | Passed |
| Compatibility alias input boundary | Passed |
| Conservative name-only compatibility claim | Passed |
| First-signal watch shutdown | Passed |

The runner emits only a fixed profile and aggregate counts. Every Cargo child
uses the Phase 1 bounded-capture helper, which limits combined output and
enforces an absolute deadline. The Phase 1 benchmark runner invokes this same
matrix before workload measurements, and its receipt validator requires the
exact profile, case count, and zero failure count. A later successful clean
workflow will therefore include it in retained benchmark evidence.

## Limits

This is not a release attestation: the evaluated worktree was not a committed,
clean exact revision, and it covers only the local Linux profile. It does not
replace the macOS and Windows path, watcher, cancellation, mutation-outcome,
and configuration evidence required before Phase 1 ADR or budget ratification.
