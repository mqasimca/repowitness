# Phase 0 SQLite schema version 8

> Historical pre-baseline development schema. The current runtime does not
> accept this format. It is retained only as design provenance and was
> superseded by [ADR-0022](../adr/0022-squash-pre-release-sqlite-schema.md) and
> the [current schema](phase0-sqlite-current-v2.md).

- Status: Implemented
- Date: 2026-07-28
- Governing decision:
  [ADR-0021](../adr/0021-phase0-memory-management-and-review.md) (accepted)
- Previous version: [Phase 0 SQLite schema version 7](phase0-sqlite-v7.md)
- Implementation:
  [`crates/repowitness-local/src/sqlite/`](../../crates/repowitness-local/src/sqlite/)

Version 8 preserves every version-7 source-generation, analysis-artifact,
memory-journal, and current-memory-projection contract. It extends the
projection format for explicit correspondence review and makes exact review
event retries idempotent.

## Reviewed correspondence

`memory_projection_evidence.outcome` admits `reviewed_link`, and its
`assurance` must be `reviewed`. A reviewed link carries the same complete
target occurrence identity as an automatic exact, rename, or move result.
Unlike an automatic result, its candidate coverage may be `complete` or
`partial` because an explicit trusted review can resolve one target without
claiming that automatic candidate discovery was exhaustive.

The existing `indeterminate` outcome may also retain partial candidate
coverage. Conflicting approvals, or an approval and rejection of the same
target, therefore remain explicit instead of selecting a winner.

Candidate rows remain review-required automatic proposals. They can be staged
only for an `ambiguous` evidence outcome and remain immutable after projection
publication.

## Idempotent review audit

The `unique_memory_correspondence_event` index covers the complete review
identity:

- workspace, record, revision, and evidence ordinal;
- operation;
- exact historical source occurrence;
- exact target occurrence;
- review method and version; and
- trusted actor kind and identifier.

Repeating the same trusted assertion therefore preserves one audit event.
Different actors, operations, targets, or source occurrences remain separate
conflict-preserving evidence.

## Forward migration

SQLite cannot alter the projection table checks in place. Migration 8 runs in
one immediate transaction that:

1. copies version-7 projection evidence and candidates into transaction-local
   backup tables;
2. rebuilds both tables with the reviewed-link and partial-coverage checks;
3. restores the copied rows;
4. recreates the staging-only insert and immutable-complete triggers; and
5. creates the exact correspondence-event uniqueness index.

Version-7 rows satisfy the version-8 checks without reinterpretation.
Migrations 1 through 7 and their checksums are unchanged.

## Validation

Automated tests cover:

- fresh version-8 creation and the exact eight-row migration ledger;
- upgrades from every historical version;
- a populated version-7 upgrade preserving source, memory, projection, and
  candidate rows;
- reviewed-link admission with reviewed assurance and partial coverage;
- rejection of invalid outcome, assurance, and coverage combinations;
- exact review-event idempotency while preserving conflicting assertions;
- projection staging and completed-generation immutability; and
- idempotent reopen, recovery, publication, projection, and backup behavior
  through the existing schema suite.
