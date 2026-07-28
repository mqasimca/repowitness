# Phase 0 SQLite baseline schema version 1

- Status: Implemented and current
- Governing decisions:
  [ADR-0012](../adr/0012-phase0-sqlite-schema-and-ownership.md) and
  [ADR-0022](../adr/0022-squash-pre-release-sqlite-schema.md)
- Compatibility: fresh databases and exact baseline-version-1 databases only

## Identity

The database uses:

- `PRAGMA application_id = 0x52575031` (`RWP1`);
- `PRAGMA user_version = 1`;
- one `schema_migrations` row with the stable name
  `phase0_design_partner_baseline`;
- SHA-256 migration checksum
  `47cae51f5f5fa839d0cde3dcb85348787e0c9de76ab408d8d30648831dc276d9`.

The checksum covers the exact concatenation of these responsibility-based SQL
fragments:

- `baseline_1_core.sql`;
- `baseline_1_memory_journal.sql`;
- `baseline_1_memory_projection.sql`.

They execute as one immediate transaction and record one ledger row. Splitting
the source text does not create multiple migrations.

## Schema groups

The source-indexing and retrieval group contains workspace identity, immutable
source snapshots and manifests, language-specific reusable artifacts and facts,
Rust correspondence fingerprints, immutable index generations, generation
membership, double-buffered FTS5 search, and projection-slot state.

The append-only engineering-memory group contains immutable record versions,
parents, Git validity commits, exact source evidence, relationships, trusted
observation/approval audit events, and manual correspondence review events.

The revalidation group contains immutable memory-projection generations,
projected record state, evidence resolution, correspondence candidates, and one
active projection pointer per workspace.

All material identity, bounds, lifecycle, immutability, referential-integrity,
and atomic-publication constraints from the retired final development schema
are present directly in the baseline. Fresh creation performs no historical
backfill or table rebuild.

## Compatibility policy

The earlier development chain with user versions 1 through 8 is retired. It was
never a released compatibility contract:

- the retired version 1 has a different migration name and checksum and fails
  exact ledger validation;
- retired versions 2 through 8 are unsupported schema versions;
- rejection happens before persistent journal-mode configuration;
- RepoWitness does not modify, delete, or automatically reset a rejected file.

Most index data can be rebuilt. Local approvals and manual review events may not
be reconstructable, so an operator must preserve or export them using the
matching old build before rebuilding.

## Migration policy

The migration and checksum machinery remains active. Once maintainers declare a
persistence-compatibility boundary, the next compatible schema change is
version 2 and must include a forward migration, backup/recovery analysis, and
fixtures from every then-supported version. A further pre-release squash
requires another explicit superseding ADR.

## Validation

Automated validation covers:

- exact application ID, version, migration name, checksum, timestamp, and one-row
  ledger;
- complete table, index, trigger, FTS5, and seed-state introspection;
- `integrity_check` and `foreign_key_check`;
- semantic immutability and lifecycle triggers;
- atomic source and memory generation publication;
- restart recovery, cancellation, mutation leases, checkpointing, and online
  backup;
- exact rejection and byte preservation of retired development versions 1
  through 8;
- clean-versus-incremental indexing, retrieval, memory revalidation, CLI, MCP,
  and real-repository behavior.
