# Phase 0 SQLite current schema version 2

- Status: Implemented and current
- Governing decisions:
  [ADR-0012](../adr/0012-phase0-sqlite-schema-and-ownership.md),
  [ADR-0022](../adr/0022-squash-pre-release-sqlite-schema.md), and
  [ADR-0024](../adr/0024-persist-parser-diagnostics-migration.md)
- Compatibility: fresh databases and exact supported versions 1 and 2

## Identity

The database uses:

- `PRAGMA application_id = 0x52575031` (`RWP1`);
- `PRAGMA user_version = 2`;
- migration 1 `phase0_design_partner_baseline` with SHA-256 checksum
  `47cae51f5f5fa839d0cde3dcb85348787e0c9de76ab408d8d30648831dc276d9`;
- migration 2 `persist_known_parser_limitations` with SHA-256 checksum
  `20efea28a3139dfe67cf226431b56e0df0dbfe2deb35bb964251ac47d788c339`.

Fresh creation applies both exact migrations. An existing database is accepted
only when every ledger row through its `user_version` has the exact version,
name, and checksum. Applying migration 2 and recording its ledger row,
application ID, and user version occurs in one immediate transaction.

## Migration 2

Migration 2 adds
`analysis_artifacts.known_parser_limitation_nodes` as a non-null integer with a
default of `0` and these constraints:

```text
0 <= known_parser_limitation_nodes <= syntax_error_nodes
```

The default conservatively maps version-1 artifacts to no recognized
limitations while preserving every raw syntax-error node. Current writers
provide the exact recognized count explicitly. Migration 2 also replaces the
artifact semantic-immutability trigger so the new value cannot be updated.

The migration does not rebuild or delete a table. Source generations, reusable
artifacts, memory versions, approvals, correspondence reviews, active
projections, and audit history remain in place.

## Compatibility policy

- The exact accepted version-1 baseline upgrades in place.
- Exact version 2 reopens without applying another migration.
- Retired development versions 1 and 2 have different ledgers and fail exact
  ledger validation.
- Retired development versions 3 through 8 are unsupported schema versions.
- Rejection occurs before persistent journal configuration and does not delete,
  reset, or adopt the file.

## Validation

Automated validation covers:

- stable migration-1 and migration-2 checksum vectors;
- exact two-row ledger identity and idempotent reopen;
- a populated version-1 upgrade that preserves immutable artifact state and
  backfills the conservative recognized count;
- the recognized-subset constraint and semantic-immutability trigger;
- complete catalog, integrity, foreign-key, publication, recovery, backup,
  clean-versus-incremental, CLI, and MCP behavior.
