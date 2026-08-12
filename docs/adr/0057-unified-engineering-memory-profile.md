# ADR-0057: Expose one engineering-memory profile

- Status: Accepted
- Date: 2026-08-11
- Supersedes the user-facing profile split in [ADR-0038](0038-phase3-memory-scopes-and-kinds.md)
  and the separate-profile boundary described by [ADR-0040](0040-phase3-task-checkpoints-and-verification.md).

## Context

The Phase 0 record and the Phase 3 additional-kinds profile were implemented
as compatible versions. That was useful for preserving the accepted v1
canonical fixtures, but exposing “v1” and “v2” as separate concepts makes
ordinary memory authoring harder than necessary. Users need one document shape,
one parser, one management workflow, and one logical memory model.

The compatibility requirement remains real: existing version-1 records must
retain their canonical bytes, revision digests, audit history, and trust
meaning. A silent rewrite would break Git review and make historical approvals
ambiguous.

## Decision

1. The current user-facing engineering-memory profile is schema version 2 and
   admits all accepted memory kinds.
2. `schema_version` is optional when authoring a current document. Omission
   selects the current profile. Explicit `schema_version: 1` is a legacy
   compatibility input, not a separate user workflow.
3. `memory-manage`, canonical writing, approval, observation-only history
   import, recall, and diagnostics use the same logical memory boundary for
   both current and legacy records.
4. Version-1 canonical bytes and journal rows remain immutable. Compatibility
   storage may retain separate physical tables during forward migration, but
   application code must not require users to choose a profile or command.
5. Procedure verification and policy non-authority remain semantic rules of
   the single model; merging the representation does not turn authored claims
   into execution authority.

## Alternatives considered

- Keep two public profiles. Rejected because it makes the normal authoring and
  management path needlessly version-aware.
- Rewrite every v1 record to v2. Rejected because it changes canonical
  identity and invalidates existing reviewable history.
- Keep the v2 physical tables forever as a public boundary. Rejected because
  storage layout is not a useful user contract; compatibility tables may remain
  while migration and recovery invariants require them.

## Consequences

The parser must distinguish omitted version (current) from explicit legacy
version 1, and tests must preserve v1 golden vectors. Migration work can be
incremental because the compatibility boundary is explicit, but every logical
reader and projection path must eventually consume both physical sources under
one bounded, deterministic contract. The internal table split is therefore a
temporary implementation detail, not permission to expose a second workflow.

## Validation

The current-profile parser accepts a document without `schema_version` and
canonicalizes it as version 2. Existing v1 fixtures still require and retain
version 1. The same `memory-manage write` path accepts both inputs. Migration
15 backfills temporary v2 rows into the unified normalized journal, and the
projection, retention, recall, and correspondence paths use that same storage
boundary.
