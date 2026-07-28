# Versioned schemas

These documents define concrete persistence and boundary encodings governed by
accepted or explicitly proposed ADRs.

- [Phase 0 engineering-memory record version 1](phase0-memory-v1.md) — accepted
  production domain, parser, canonicalizer, generated-YAML format, and
  worktree-import/write boundary; bounded observation-only Git-tree history
  import is implemented without changing the record format
- [Phase 0 SQLite current schema version 2](phase0-sqlite-current-v2.md) —
  implemented current read/write format with two exact migrations and ledger
  rows, governed by accepted ADR-0022 and ADR-0024
- [Phase 0 SQLite baseline migration version 1](phase0-sqlite-baseline-v1.md) —
  immutable supported baseline and version-2 upgrade source

The owned production adapter creates version-2 databases and upgrades exact
accepted version-1 databases. It accepts only the RepoWitness application ID
and an exact migration ledger through the declared schema version. Retired
development versions 1 through 8 are rejected without mutation and require an
explicit rebuild. Automated tests cover fresh creation, version-1 upgrade,
legacy rejection, idempotent reopen, immutable generation publication, exact
artifact reuse, FTS5 rebuild, bounded/cancellable recovery, memory-journal
import/rollback/immutability, review-event idempotency, atomic
memory-projection activation, backup/restore, and file-identity races.

The retired development schema documents remain available only as design
provenance:

- [development version 1](phase0-sqlite-v1.md)
- [development version 2](phase0-sqlite-v2.md)
- [development version 3](phase0-sqlite-v3.md)
- [development version 4](phase0-sqlite-v4.md)
- [development version 5](phase0-sqlite-v5.md)
- [development version 6](phase0-sqlite-v6.md)
- [development version 7](phase0-sqlite-v7.md)
- [development version 8](phase0-sqlite-v8.md)

Changing the committed format requires a forward migration and updated
checksum. Do not edit a supported migration in place.
