# Versioned schemas

These documents define concrete persistence and boundary encodings governed by
accepted ADRs.

- [Phase 0 SQLite schema version 3](phase0-sqlite-v3.md) — current read/write
  format
- [Phase 0 SQLite schema version 2](phase0-sqlite-v2.md) — implemented and
  supported migration input
- [Phase 0 SQLite schema version 1](phase0-sqlite-v1.md) — implemented and
  supported migration input

The owned production adapter creates version 3 databases, validates every
historical migration name and checksum, and migrates versions 1 and 2 forward.
Automated tests cover fresh creation, both upgrades, idempotent reopen,
immutable generation publication, exact artifact reuse, FTS5 rebuild,
bounded/cancellable recovery, backup/restore, and file-identity races.

Changing a committed persistence format requires a forward migration and an
updated checksum. Do not edit a historical migration in place after release.
