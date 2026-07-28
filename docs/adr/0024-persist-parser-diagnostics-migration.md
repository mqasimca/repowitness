# ADR-0024: Persist recognized parser diagnostics through migration 2

- Status: Accepted
- Date: 2026-07-28
- Owners: Project maintainers
- Scope: SQLite schema identity, parser-diagnostic persistence, and version-1 compatibility

## Context

The accepted version-1 SQLite baseline predates the distinction between raw
Tree-sitter error or missing-node counts and the recognized subset attributed
to a known parser limitation. The application and wire contracts now expose
both counts without subtracting the recognized subset from the raw total.
Reusable analysis artifacts must persist both values to keep diagnostics exact
across restart and reuse.

ADR-0022 made the version-1 baseline immutable and requires the next compatible
storage change to use migration 2. Rewriting migration 1 would change its
checksum, reject already-created supported databases, and risk stranding local
approvals or review history that cannot be reconstructed from source.

## Decision

- Preserve migration 1 byte for byte, including its stable name and checksum.
- Add migration 2 named `persist_known_parser_limitations`.
- Set `PRAGMA user_version = 2` only after migration 2 and its ledger row commit
  in the same immediate transaction.
- Add `analysis_artifacts.known_parser_limitation_nodes` as a nonnegative,
  non-null integer constrained to be no greater than
  `syntax_error_nodes`.
- Backfill accepted version-1 artifacts with `0`. This is conservative:
  historical rows retain their raw errors without claiming that any were
  recognized.
- Keep the column default at `0` because SQLite cannot add a non-null column to
  populated tables without a non-null default. Production writers still supply
  the value explicitly.
- Replace the artifact semantic-immutability trigger in the same transaction so
  the new value cannot change after insertion.
- Accept exact supported version-1 and version-2 ledgers. Continue to reject
  retired development schemas and unrelated SQLite files before persistent
  connection configuration.

## Alternatives considered

### Rewrite the version-1 baseline

This keeps one migration for fresh databases but breaks the accepted checksum
contract and strands supported databases. It was rejected.

### Rebuild the artifact table

A table rebuild could remove the default or change physical column order, but
it adds copy/drop failure modes without improving the named-column persistence
contract.

### Derive the recognized count after reading

Derivation would depend on current parser knowledge instead of the exact
producer that created the artifact. It would make reused diagnostics
historically inaccurate.

### Store a nullable value

Null would create a third, ambiguous state and weaken the invariant that every
material diagnostic reports an exact raw count and recognized subset.

## Consequences

### Positive

- Existing supported databases upgrade without losing source, memory,
  approval, or review state.
- Fresh and upgraded databases share one exact version-2 catalog.
- Raw parser errors remain visible and the recognized subset remains
  non-subtractive.
- Migration identity and artifact immutability remain fail-closed.

### Negative and risks

- Fresh databases now apply two small transactions and carry two ledger rows.
- The added SQLite default remains part of the catalog even though current
  writers provide an explicit value.
- Version-1 compatibility requires a permanent migration regression fixture.

## Validation

- Assert the original migration-1 checksum as a golden vector.
- Assert the migration-2 checksum and complete version-2 catalog as golden
  vectors.
- Upgrade a populated version-1 database and verify exact ledger timestamps,
  preserved artifact bytes, conservative backfill, and the replaced
  immutability trigger.
- Reopen version 2 and prove that migration application is idempotent.
- Reject negative or greater-than-raw recognized counts at the schema boundary.
- Run integrity, foreign-key, recovery, publication, backup, clean-versus-
  incremental, CLI, and MCP tests.

## Follow-up

- Treat version 2 as the current local read/write format.
- Add future compatible changes as monotonically numbered migrations without
  editing migrations 1 or 2.

## Supersession

This ADR fulfills ADR-0022's post-baseline migration requirement and supersedes
only descriptions of version 1 as the current read/write format. ADR-0022's
baseline identity, retired-development rejection, and non-destructive handling
remain accepted.
