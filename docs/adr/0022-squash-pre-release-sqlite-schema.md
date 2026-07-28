# ADR-0022: Squash the pre-release SQLite chain into one baseline

- Status: Accepted
- Date: 2026-07-27
- Owners: Project maintainers
- Scope: Phase 0 SQLite schema identity, compatibility, and migration history

## Context

RepoWitness has not released a stable database format or promised compatibility
for persisted development databases. During Phase 0 implementation, eight
schema migrations were added as indexing, retrieval, multi-language, memory,
revalidation, and review capabilities landed. Those migrations were useful
checkpoints, but treating all eight as supported upgrade inputs now creates
production code, fixtures, documentation, and failure paths for a compatibility
promise the product has not made.

The current database can also contain locally approved memory and manual review
events that are not necessarily reconstructable from repository source. A
pre-release squash must therefore reject old files without modifying or
deleting them; it must not silently reset a database merely because most index
rows are reproducible.

## Decision

- Replace the development migration chain with one clean schema version 1
  baseline named `phase0_design_partner_baseline`.
- Keep application ID `0x52575031` (`RWP1`), the exact migration checksum
  ledger, transactional migration application, and all ownership, recovery,
  backup, and generation-publication rules from
  [ADR-0012](0012-phase0-sqlite-schema-and-ownership.md).
- Assemble the one logical migration from responsibility-based source,
  memory-journal, and memory-projection SQL fragments. The fragments execute in
  one immediate transaction and produce one ledger row.
- Define the baseline directly as the final schema. It contains no temporary
  migration tables, historical backfills, table rebuilds, or transitional
  `ALTER TABLE` or `DROP TABLE` statements.
- Accept a pre-existing database only when its application ID, `user_version =
  1`, migration name, and exact checksum match the new baseline. The retired
  development version 1 fails ledger validation; retired versions 2 through 8
  fail schema-version validation. Rejection occurs before persistent journal
  configuration and does not modify the file or create WAL sidecars.
- Provide no in-place upgrade from the retired development chain. An operator
  who needs local approval or review history must preserve/export it with the
  matching old build before rebuilding the database.
- Retain the migration mechanism. The first post-baseline compatibility change
  becomes migration 2 after a persistence-compatibility boundary is declared.
  Another pre-release squash requires an explicit superseding decision.

## Alternatives considered

### Keep all eight development migrations

This preserves every development database, but permanently carries an
unreleased compatibility matrix and obscures the actual initial product
schema.

### Concatenate the historical migrations into one ledger entry

This produces one recorded version but still creates, copies, alters, and drops
legacy structures on every fresh database. It retains complexity without
providing compatibility.

### Automatically delete and rebuild old databases

Index facts are reproducible, but local approvals and manual reviews may not be.
Automatic deletion would be destructive and would violate the fail-closed
database-identity boundary.

## Consequences

### Positive

- Fresh startup has one schema transition and one exact ledger row.
- The baseline describes the product's real schema instead of its development
  sequence.
- Migration tests focus on current integrity and explicit legacy rejection.
- Future migrations start from one well-defined compatibility boundary.

### Negative and risks

- Retired version 1 through 8 development databases cannot be opened by the new
  build.
- Developers must rebuild disposable indexes and intentionally preserve any
  non-reconstructable local trust history before switching builds.
- Historical schema documents remain as provenance and must be clearly marked
  unsupported.

## Validation

- Compare the baseline `sqlite_schema` catalog to the final retired version-8
  catalog, including tables, indexes, triggers, FTS5 virtual tables, and seed
  state.
- Assert one exact migration checksum and one ledger row on fresh creation and
  idempotent reopen.
- Assert the baseline contains no transitional DDL.
- Reject retired versions 1 through 8 byte-for-byte without WAL or shared-memory
  sidecars.
- Run schema introspection, foreign-key, integrity, immutability, publication,
  recovery, backup, clean-versus-incremental, CLI, MCP, and real-repository
  tests.

## Follow-up

- Treat [the baseline schema](../schemas/phase0-sqlite-baseline-v1.md) as the
  only supported Phase 0 SQLite format.
- Require an explicit migration-2 decision before promising an in-place schema
  upgrade.

## Supersession

This ADR supersedes the exact migration identity and pre-release upgrade-chain
parts of ADR-0012 and the schema-version/migration compatibility clauses in
ADRs 0015 through 0018, 0020, and 0021. Their language, memory, correspondence,
trust, ownership, and generation decisions remain unchanged.
