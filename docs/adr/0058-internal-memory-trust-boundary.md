# ADR-0058: Keep memory trust receipts internal and minimal

- Status: Accepted
- Date: 2026-08-11
- Governing decision: [ADR-0057](0057-unified-engineering-memory-profile.md)

## Context

Memory audit rows currently serve two different purposes: historical
provenance and the current answer to “may this exact revision be trusted?”
Exposing those concerns together makes the product appear to require an audit
history feature, even though ordinary users need one memory workflow and a
simple current trust result.

Deleting provenance would remove useful recovery and conflict information, and
would make future team synchronization unable to explain or safely resolve
competing approvals.

## Decision

- Keep append-only observation, approval, and correspondence records as
  internal implementation state.
- Add a current-trust read boundary containing only locally approved exact
  revisions. Ordinary journal loading, projection, recall, and approval checks
  use that boundary instead of scanning historical audit rows.
- Do not expose audit tables, audit-history queries, or audit retention controls
  as a user-facing memory feature.
- Keep explicit history import and manual correspondence semantics available to
  internal adapters because they are provenance and trust inputs, not a general
  audit product.

## Consequences

The current trust view is deterministic and keeps the normal read path small,
while old provenance remains available for bounded recovery and future conflict
resolution. Storage is not immediately smaller; a later retention policy may
discard old observation detail only after proving that no trust, history, or
recovery contract depends on it. Existing immutable records and approvals are
not rewritten.

## Validation

Migration 14 creates `memory_current_trust` over both legacy and current
physical journals. Tests verify the view exists, ordinary reads use it, and
observation-only imports remain unable to activate memory.
