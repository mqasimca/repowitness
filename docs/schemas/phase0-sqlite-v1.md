# Phase 0 SQLite schema version 1

> Historical pre-baseline development schema. The current runtime does not
> accept this format. It is retained only as design provenance and was
> superseded by [ADR-0022](../adr/0022-squash-pre-release-sqlite-schema.md) and
> the [current schema](phase0-sqlite-current-v2.md).

- Status: Implemented
- Date: 2026-07-25
- Current successor: [Phase 0 SQLite schema version 2](phase0-sqlite-v2.md)
- Governing decision:
  [ADR-0012](../adr/0012-phase0-sqlite-schema-and-ownership.md)
- Implementation:
  [`crates/repowitness-local/src/sqlite/`](../../crates/repowitness-local/src/sqlite/)

## File and migration identity

Schema version 1 uses SQLite application ID `0x52575031` (`RWP1`) and
`PRAGMA user_version = 1`. The exact migration text is compiled into
`schema.rs`; its SHA-256 digest, stable name, version, and injected diagnostic
timestamp are recorded in `schema_migrations`. Startup rejects a different
application ID, user version, migration count, migration name, or checksum.

The bundled SQLite must be at least 3.51.3 and must report `ENABLE_FTS5`.
Writer connections enable foreign keys, disable trusted schema, use WAL,
disable automatic checkpoints, set `synchronous=FULL`, and use a fixed busy
timeout. Reader and backup-source connections are read-only and query-only.

## Canonical identities

Every persisted digest is exactly 32 bytes and is mapped through a distinct
domain newtype. Repository, Git-state, worktree/submodule, configuration,
producer/grammar, analysis-schema, source-content, manifest, snapshot, and
artifact identities are never interchangeable.

The Phase 0 source-manifest digest is:

```text
SHA-256(
  "RepoWitness\0source-manifest\0" ||
  manifest_version:u32be ||
  file_count:u64be ||
  for each exact path-ordered entry:
    path_byte_count:u64be ||
    path_bytes ||
    file_kind:u8 ||
    content_digest[32]
)
```

The Phase 0 Rust source-snapshot digest is:

```text
SHA-256(
  "RepoWitness\0rust-source-snapshot\0" ||
  snapshot_version:u32be ||
  repository_identity[32] ||
  git_state_digest[32] ||
  worktree_state_digest[32] ||
  configuration_digest[32] ||
  producer_manifest_digest[32] ||
  analysis_schema_digest[32] ||
  canonicalization_version:u32be ||
  source_manifest_digest[32]
)
```

The adapter that constructs `RustSourceSnapshotIdentity` owns validation of
the repository, Git, worktree/submodule, configuration, producer, and schema
inputs. The SQLite adapter verifies exact width, rehashes every artifact key,
and refuses prepared facts whose semantics do not match the declared identity.
The production CLI index composition constructs this identity, and the CLI and
MCP retrieval adapters expose its opaque snapshot digest without reconstructing
or weakening it.

## Ownership groups

- `workspaces` stores one repository identity, monotonic source epoch, and
  nullable active-generation pointer.
- `source_snapshots` and `source_manifest_entries` store complete canonical
  snapshot semantics and exact byte paths.
- `analysis_artifacts` and `artifact_facts` store reusable content-addressed
  Rust facts, producer identities, and half-open byte spans.
- `index_generations` and `generation_files` store immutable publication
  lifecycles, coverage, and path-to-artifact mappings.
- `generation_facts` reserves bounded generation-local cross-file facts.
- `generation_search` is a disposable generation-scoped FTS5 projection.

Snapshot and artifact semantic columns are immutable. Their lifecycle metadata
may move only from `staging` to `complete`, allowing at most 256 facts per
transaction without exposing partially written reusable content. Startup
deletes incomplete content staging, marks every incomplete generation failed,
and deletes only its generation-scoped rows within an inclusive 4,096-generation
recovery budget, cancellation flag, and absolute deadline. It selects one
extra generation before mutation to detect overflow; an over-limit or
interrupted recovery rolls back without partial state changes. Complete unused
snapshots and artifacts remain eligible for verified reuse.

Generation lifecycle transitions are enforced by triggers:

```text
discovered -> extracting -> resolving -> validating -> ready -> active
       |            |            |             |          |
       +------------+------------+-------------+----------+-> failed/cancelled

active -> retained
```

Activation compares the expected source epoch, retains the previous active
generation, activates the ready candidate, and updates the workspace pointer
in one immediate transaction. A reader resolves that pointer once inside one
read transaction.

## Retrieval and resource limits

The FTS5 projection uses `unicode61`, keeps diacritics, and treats underscore
as a token character. Untrusted input is limited to 256 UTF-8 bytes, eight
whitespace-delimited terms, and 64 bytes per term. Each term is quoted and
embedded quotes are doubled; raw FTS operators are never accepted.

Search has a hard ceiling of 100 rows and 1 MiB of encoded result data. The
default is 20 rows and 256 KiB. Results are ordered by weighted `bm25()`, exact
repository-path bytes, and fact ordinal. Every hit carries the pinned
generation, exact path, content and artifact digests, declaration category,
symbol names, and source spans. A SQLite progress callback observes the
absolute deadline and cancellation flag.

The writer and each reader own their connections on dedicated OS threads
behind capacity-one queues and bounded one-shot replies. Write batches contain
at most 256 manifest entries, artifact facts, generation mappings, or search
facts. The application `publish_rust_index` use case stages and activates
through a narrow publication port implemented by the owned SQLite writer.
Explicit truncating checkpoints report busy, WAL-frame, and completed frame
counts.

## Backup

Online backup runs on a separately owned thread with page-step, total-step,
retry, deadline, and cancellation bounds. It writes a private sibling file,
validates application/schema identity, migration ledger, `integrity_check`,
foreign keys, and active-pointer agreement, then publishes with a no-clobber
hard link. Cancellation, reply timeout, worker failure, or validation failure
signals cancellation, joins the owner thread, removes the private database and
SQLite sidecars, and never reports a completed destination.

## Validation

Production tests cover fresh creation, idempotent reopen, wrong identity,
future version, migration-checksum mismatch, semantic immutability, bounded
staging, activation, stale epochs, cancellation, restart recovery, active
generation scoping, deterministic retrieval, hostile literal queries,
result-byte limits, checkpointing, online backup, partial cleanup, recovery
overflow rollback, database hard-link/replacement races, and post-open file
identity revalidation.

The opt-in `real_sqlite_index` integration test runs discovery,
capability-contained reads, Rust analysis, persistence, activation, and
retrieval against an externally configured real repository.
