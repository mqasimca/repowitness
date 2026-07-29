# Phase 0 ratification review

- Status: Recommendation
- Reviewed: 2026-07-28
- Decision update: 2026-07-29 UTC
- Scope: ADR-0017, ADR-0018, ADR-0019, ADR-0021, ADR-0023, and
  Phase 0 benchmark budgets

## Review boundary

This review evaluates whether the implemented contracts have enough evidence
for maintainer ratification. The original review did not change an ADR or
budget status. Accepted decisions require explicit maintainer action. At the
time of the review, the working tree was intentionally left uncommitted, so its
result was not a clean-revision release attestation.

## Maintainer decision update

The subsequent
[clean Ubuntu 24.04 attestation](phase0-clean-benchmark-attestation-2026-07-29.md)
passed every unchanged correctness and resource ceiling. Maintainers adopted
the recommendations to accept ADR-0017, ADR-0019, and ADR-0023 and ratified
the manifest and its budgets. ADR-0018 remains proposed pending the real
design-partner comparison. ADR-0021 remains proposed because ADR-0018 is still
a prerequisite; its clean-benchmark prerequisite is complete.

The review uses:

- the complete unit, property, integration, migration, crash/recovery,
  clean-versus-incremental, CLI, and MCP contract matrix;
- the
  [public product-loop benchmark](phase0-product-benchmark-2026-07-28.md);
- the
  [controlled baseline comparison](phase0-comparative-evaluation-2026-07-28.md);
- the [Codex utility evaluation](phase0-codex-utility-evaluation-2026-07-28.md);
  and
- the checksum-pinned vendored-grammar provenance and validation gate.

## ADR recommendations

| ADR | Recommendation | Evidence and remaining condition |
|---|---|---|
| [ADR-0017](../adr/0017-phase0-memory-journal.md) | Accept the technical contract | Append-only import, trust separation, idempotency, corruption, rollback, reopen, backup, and hostile-path tests pass. Its historical migration-version clauses are already explicitly superseded by accepted ADR-0022. |
| [ADR-0018](../adr/0018-phase0-memory-revalidation.md) | Keep proposed for now | The correspondence, ambiguity, Git-DAG, review, publication, and stale-memory matrix passes. The ADR itself requires a comparative design-partner evaluation before ratification; the controlled public and Codex evaluations are not a real design-partner outcome. |
| [ADR-0019](../adr/0019-phase0-context-compilation-and-diagnostics.md) | Accept the technical contract | Generation pinning, current-memory admission, omissions, diagnostics, CLI/MCP schemas, and adversarial tests pass. The Codex evaluation found and closed the unreadable hexadecimal source-presentation defect. |
| [ADR-0021](../adr/0021-phase0-memory-management-and-review.md) | Keep proposed for now | The local trust, write, approval, history, review, authorization, and fault matrix passes. ADR-0018 and the real design-partner outcome still block acceptance; the clean benchmark prerequisite is complete. |
| [ADR-0023](../adr/0023-vendor-typescript-grammar-fix.md) | Accept the bounded vendor decision | Provenance, inventory, checksums, regeneration inputs, capability scan, parser regression, language matrix, dependency policy, and full build gates pass. The upstream replacement condition remains explicit. |

This separates technical readiness from product-outcome evidence. Holding
ADR-0018 and ADR-0021 does not identify an implementation defect; it preserves
their own stated ratification prerequisites.

## Budget review

The manifest now declares and the product probe enforces every listed resource
ceiling, including database and post-completion WAL size. Missing database
metadata fails closed instead of becoming a zero-byte measurement.

| Metric | Development result | Proposed ceiling | Ceiling used |
|---|---:|---:|---:|
| Cold full index | 421.332 ms | 10,000 ms | 4.2% |
| Warm query p95 | 3.022 ms | 250 ms | 1.2% |
| Peak RSS | 11.906 MiB | 256 MiB | 4.7% |
| MCP material result | 3,620 bytes | 49,152 bytes | 7.4% |
| SQLite database | 528,384 bytes | 4,194,304 bytes | 12.6% |
| SQLite WAL after completion | 0 bytes | 0 bytes | pass |

The zero-tolerance correctness budgets are architectural invariants and are
ready to ratify:

- zero false confirmed claims;
- zero silent truncations;
- zero mixed-generation reads; and
- zero false automatic relinks.

The current resource values need no adjustment based on the observed public
corpus. They should remain proposed until the complete runner passes from a
clean exact RepoWitness revision on the release CI platform. The margins are
deliberately broad for a small corpus and do not establish scaling behavior.
The opt-in Codex token totals are observations, not resource budgets.

The clean release-platform run subsequently satisfied that condition.
Maintainers ratified the unchanged correctness and resource budgets for the
versioned Phase 0 profile.

## Defects closed during review

1. Exact UTF-8 source was encoded as hexadecimal at the CLI/MCP presentation
   boundary. The wire now carries readable display-safe UTF-8 with a labeled
   hexadecimal fallback for invalid or display-unsafe bytes, and the CLI uses
   injection-safe JSON escaping.
2. Database and WAL sizes were reported but not enforced. The product probe now
   receives and checks the bounded manifest ceilings, reports the resolved
   profile, and fails closed on unavailable required metadata.
3. The benchmark manifest named a Codex response contract without validating
   the contract file. The offline benchmark check now rejects a missing,
   symlinked, oversized, or invalid JSON schema.
4. The installed-binary MCP contract did not directly assert the
   `context_build` source representation. It now verifies schema version 2,
   source kind, language, name, encoding, and exact declaration text.

## Remaining Phase 0 actions

1. Completed 2026-07-29: accept ADR-0017, ADR-0019, and ADR-0023.
2. Run and record one real design-partner comparison, then decide ADR-0018.
3. Completed 2026-07-29: rerun the complete benchmark from a clean exact
   revision on the release CI platform and ratify the unchanged budgets.
4. Decide ADR-0021 after ADR-0018's design-partner prerequisite is satisfied.

No broader feature work is required to complete the current Phase 0 contract.
