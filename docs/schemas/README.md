# Versioned schemas

These documents define concrete persistence and boundary encodings governed by
accepted or explicitly proposed ADRs.

- [Local configuration version 1](configuration-v1.md) — strict bounded TOML
  admission, deterministic preference provenance, monotonic policy resolution,
  and canonical semantic identity under accepted ADR-0025
- [Phase 0 engineering-memory record version 1](phase0-memory-v1.md) — accepted
  production domain, parser, canonicalizer, generated-YAML format, and
  worktree-import/write boundary; bounded observation-only Git-tree history
  import is implemented without changing the record format
- [Phase 1 SQLite schema version 3](phase1-sqlite-provisional-v3.md) — accepted
  read/write format with connected-workspace source slots, immutable views,
  generation-scoped Rust graph publication, and deterministic bounded retention
  plan/apply under ADR-0029
- [Phase 0 SQLite schema version 2](phase0-sqlite-current-v2.md) — accepted
  predecessor with two exact migrations and ledger rows, governed by accepted
  ADR-0022 and ADR-0024
- [Phase 0 SQLite baseline migration version 1](phase0-sqlite-baseline-v1.md) —
  immutable supported baseline and version-2 upgrade source

The owned production adapter creates version-3 databases and upgrades exact
accepted version-1 and version-2 databases. It
accepts only the RepoWitness application ID and an exact migration ledger
through the declared schema version. Retired development versions 1 through 8
are rejected without mutation and require an explicit rebuild. Automated tests
cover fresh creation, version-1 and populated version-2 upgrade,
legacy rejection, idempotent reopen, immutable generation publication, exact
artifact reuse, FTS5 rebuild, bounded/cancellable recovery, memory-journal
import/rollback/immutability, review-event idempotency, atomic
memory-projection activation, connected-workspace view publication,
backup/restore, and file-identity races.

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
