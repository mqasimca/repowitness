# SQLite generation durability spike

- Status: Completed research record
- Research date: 2026-07-23
- Last updated: 2026-07-26
- Scope: Phase 0 bounded staging, generation activation, crash recovery,
  checkpointing, and online backup

## Question

Can RepoWitness use bounded direct SQLite staging writes while preserving one
readable active generation through activation, cancellation, failure, and
process termination, and does a bundled SQLite satisfy the WAL-reset safety
floor?

The architecture required this spike before the persistence boundary
stabilized. The fixture remains an integration and measurement harness rather
than the production schema; its results were promoted through accepted
[ADR-0012](../adr/0012-phase0-sqlite-schema-and-ownership.md), the versioned
[SQLite schemas](../schemas/README.md), and owned production adapters.

## Reviewed dependency and promotion

The workspace pins `rusqlite` 0.40.1 as a production dependency with default
features disabled and only `bundled`, `backup`, and `hooks` enabled.

- `rusqlite` and `libsqlite3-sys` are MIT-licensed. The bundled SQLite source
  is public domain.
- The selected features provide a controlled SQLite build and the online
  backup API without enabling loadable extensions, serialization helpers,
  time libraries, or an async/pooling layer.
- This candidate builds native SQLite C through `libsqlite3-sys`; it does not
  add first-party unsafe code.
- The runtime reports SQLite 3.53.2 with FTS5 and serialized thread safety.
  The test rejects versions older than 3.51.3.
- `rusqlite` follows the latest stable Rust release as its stated MSRV, which
  currently matches RepoWitness's pinned Rust 1.97.1 toolchain.

The [rusqlite 0.40.1 documentation](https://docs.rs/crate/rusqlite/0.40.1)
documents its feature surface and bundled-build recommendation. SQLite records
that the WAL-reset corruption defect is fixed in 3.51.3 and later in the
[WAL documentation](https://sqlite.org/wal.html#the_wal_reset_bug).

The production dependency review is recorded in
[Phase 0 production dependency review](phase0-dependency-review-2026-07-25.md).
The spike itself still does not define the migration checksum, production
schema, or public persistence contract.

## Executable fixture

[`sqlite_generation_spike.rs`](../../crates/repowitness-local/tests/sqlite_generation_spike.rs)
defines an intentionally disposable schema and validates:

- fixed runtime version, FTS5, serialized thread safety, WAL mode, foreign
  keys, disabled trusted schema, a fixed busy timeout, and disabled automatic
  checkpoints;
- bounded fact batches with one writer;
- one short activation transaction that retains the previous active
  generation and changes the workspace pointer;
- a reader transaction that continues to observe its pinned old generation
  while a new reader observes the newly activated generation;
- cancelled, stale-epoch, failed, and rolled-back staging that never changes
  the active pointer;
- real child-process termination in `discovered`, `extracting`, `resolving`,
  `validating`, and `ready`, followed by deterministic cleanup of incomplete
  facts and preservation of the old active generation;
- online backup while committed frames remain in the source WAL, followed by
  integrity checking and logical restore verification;
- deterministic interleaving of online-backup steps with a separate writer
  committing and activating a new generation plus an incomplete successor,
  followed by restored-state recovery that preserves the active generation;
- cancellation of an online backup held open across four successive
  generation publications, with fixed step, deadline, and WAL bounds,
  followed by a clean backup that restores the final active generation;
- a reader-pinned snapshot that prevents a truncating checkpoint while the
  writer continues publishing bounded generations, followed by successful WAL
  truncation only after the reader releases its snapshot;
- a capacity-one command queue with separately owned writer and reader
  connections, bounded replies and read lifetime, pre-mutation request-limit
  rejection, observed checkpoint latency/WAL size, explicit reader
  cancellation, and clean worker shutdown;
- atomic activation, pinned reads, and stale-epoch rejection under both
  `synchronous=FULL` and `synchronous=NORMAL`;
- an explicit truncating checkpoint; and
- logical equivalence between bounded direct staging and a private in-memory
  staging database materialized through the online backup API.

The fixture originally used `synchronous=FULL` and manual checkpointing as
conservative spike settings. ADR-0012 subsequently accepted those choices for
Phase 0 production, together with at most 256 fact rows per transaction and
explicit checkpoint outcomes.

## Initial measurements

These numbers are a synthetic microbenchmark, not the Phase 0 corpus result or
a release gate.

### Environment

| Field | Value |
|---|---|
| Rust | 1.97.1 (`8bab26f4f`, LLVM 22.1.6) |
| Host | x86_64 Linux 7.1.4-1-cachyos |
| CPU | AMD Ryzen 9 9950X3D, 32 logical CPUs |
| Memory | 63,423,724 KiB reported by Linux |
| Filesystem | Btrfs |
| Profile | Cargo `release` |
| SQLite | bundled 3.53.2 |
| Input | 10,000 deterministic facts of 256 bytes |
| Batch size | 256 facts |

Five warm runs produced:

| Strategy | Median | Range | Database bytes |
|---|---:|---:|---:|
| Bounded direct WAL staging | 6.48 ms | 5.46–7.21 ms | 2,949,120 |
| Private RAM-first plus online materialization | 52.97 ms | 51.29–53.07 ms | 2,949,120 |

The median RAM-first path was approximately 8.2 times slower for this
workload. Separate Linux process samples reported:

| Strategy | Sample elapsed | Peak RSS |
|---|---:|---:|
| Bounded direct WAL staging | 5.47 ms | 9,472 KiB |
| Private RAM-first plus online materialization | 51.93 ms | 12,784 KiB |

The RSS values include the Rust test harness and the input vector. They are
useful only as an initial within-host comparison.

### Owned-connection contention sample

The test-only owned topology fixes the writer queue capacity at one, limits a
generation request to 512 facts, gives every reply and reader lifetime a
five-second bound, gives each checkpoint a two-second assertion deadline, and
caps this fixture's WAL at 4 MiB. An oversized request is rejected before a
generation row is created.

A representative debug-profile run on the environment above, followed by 25
consecutive clean repetitions, reported:

| Observation | Value |
|---|---:|
| First reader-blocked checkpoint | 250 ms |
| Second reader-blocked checkpoint | 250 ms |
| Maximum observed WAL | 675,712 bytes |
| Reader cancellation acknowledgement | 8 ms |
| Final checkpoint after reader release | 0 ms |

These are contention-fixture observations, not accepted production budgets.
They show that the candidate ownership shape can surface starvation, continue
bounded generation publication, cancel the pin, and recover checkpoint
progress without mixing generations.

### Sustained-write backup cancellation sample

The cancellation fixture holds an online backup open while four generations
of 512 deterministic facts are published through the capacity-one writer. It
limits the backup to 4,096 single-page steps and five seconds, requires
cancellation acknowledgement within two seconds, and caps the source WAL at
16 MiB. A cancelled destination remains explicitly partial; only a subsequent
completed online backup is used for restore verification.

A representative debug-profile run on the environment above, followed by 25
consecutive clean repetitions, reported:

| Observation | Value |
|---|---:|
| Backup steps before cancellation | 2 |
| Backup lifetime before cancellation | 10 ms |
| Cancellation acknowledgement | 5 ms |
| Maximum observed WAL | 2,130,072 bytes |

These values are non-gating fixture observations. The durable result is that
publication remains available during the open backup, cancellation releases
the source snapshot within the declared bound, a truncating checkpoint then
completes, and a fresh online backup restores the last active generation
without publishing the partial destination.

### Batch and synchronization sample

Five release-profile samples for each candidate used 10,000 deterministic
256-byte facts. Every profile separately verifies the active generation and
all stored facts; a non-benchmark regression test also proves atomic
activation, reader pinning, and stale-epoch rejection under both synchronization
settings.

| Synchronization | Batch | Median | Range | Maximum WAL |
|---|---:|---:|---:|---:|
| `FULL` | 16 | 9.08 ms | 8.90–9.87 ms | 17,514,152 bytes |
| `FULL` | 64 | 5.79 ms | 5.79–6.44 ms | 7,547,872 bytes |
| `FULL` | 256 | 4.88 ms | 4.76–5.64 ms | 4,338,392 bytes |
| `FULL` | 512 | 4.67 ms | 4.56–5.73 ms | 3,695,672 bytes |
| `NORMAL` | 16 | 9.00 ms | 8.83–9.12 ms | 17,514,152 bytes |
| `NORMAL` | 64 | 5.77 ms | 5.66–6.32 ms | 7,547,872 bytes |
| `NORMAL` | 256 | 4.80 ms | 4.77–4.84 ms | 4,338,392 bytes |
| `NORMAL` | 512 | 4.62 ms | 4.59–4.64 ms | 3,695,672 bytes |

This host showed no material reason to weaken the conservative Phase 0
durability setting. `FULL` with a maximum 256-row transaction stayed close to
the 512-row timing while providing twice as many cancellation boundaries and
was subsequently accepted for Phase 0 by ADR-0012. These are synthetic
measurements, not ratified corpus budgets.

## Outcome

Bounded direct staging with `synchronous=FULL` and at most 256 rows per write
transaction is the implemented Phase 0 profile. In this fixture it is faster,
uses less peak resident memory, produces the same database size and logical
rows, bounds cancellation intervals, and naturally leaves crash-visible
generation state for recovery.

The test schema encodes only enough behavior to challenge the architecture
invariants and is not a migration or public persistence format. The production
schema and migration checksums live in the
[versioned schema documents](../schemas/README.md).

## Promotion and remaining measurement

Completed production promotion includes exact migration identity, immutable
artifacts and generations, atomic activation, active-reader pinning, bounded
and cancellable startup recovery, double-buffered FTS5 publication,
checkpointing, online backup/restore, and exact persisted-artifact reuse.
Crash tests terminate writers in every pre-activation state. Additional
regressions cover a 4,096-row inclusive recovery ceiling, rollback at 4,097,
database hard-link and replacement races, post-open identity revalidation, and
cleanup limited to a verified newly created database.

All four manual release probes passed again on 2026-07-26. The two 10,000-fact
strategies produced the same 2,949,120-byte logical database; direct staging
remained faster and used less peak resident memory on that host. These
environment-specific samples are diagnostic, not release budgets.

The remaining gate is a repeated pinned-corpus run spanning cold creation,
warm exact reuse, one-file incremental reanalysis, retrieval, projection
rebuild, database/WAL growth, queue high-water marks, and peak RSS. Those
measurements belong to the Phase 0 benchmark decision, not to the already
completed architecture spike.

SQLite's [online backup documentation](https://sqlite.org/backup.html) and
[WAL checkpoint behavior](https://sqlite.org/wal.html) remain the controlling
primary references for the next experiments.
