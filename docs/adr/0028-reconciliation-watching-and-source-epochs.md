# ADR-0028: Make reconciliation authoritative for watching and source epochs

- Status: Accepted
- Date: 2026-07-29
- Owners: Project maintainers
- Scope: Local watching, reconciliation, source epochs, cancellation, and restart

## Context

Phase 0 proves that a complete canonical manifest can reconcile arbitrary
dirty-path hint sequences, but it has no production watcher service. Its
original one-shot CLI used source epoch zero and created a cancellation flag
that is not connected to an operating-system shutdown signal.

Filesystem notifications are not a correctness boundary. Editors and hosts
may drop, duplicate, reorder, coalesce, or overflow events. Network and
virtualized filesystems may emit no usable event. Treating an event stream as a
transaction log would eventually publish a generation that omits a real
source change.

Watching also turns occasional one-shot work into a long-lived supervised
service. Admission, debounce, retries, cancellation, restart, retained
generations, and shutdown therefore need explicit bounds.

## Decision

### Complete reconciliation is authoritative

Every watcher backend produces only dirty hints. A hint can reduce the work
considered by a reconciliation attempt, but activation always depends on the
same complete Git/worktree receipt, canonical source manifest, contained
source reads, and final stability fence used by one-shot indexing.

The service performs:

1. one mandatory complete reconciliation at startup;
2. a bounded debounce after hints;
3. a complete reconciliation after hint-buffer overflow or an unsupported
   event;
4. a periodic complete reconciliation even when no hint arrives; and
5. a complete final check before any generation activation.

Dropping, duplicating, reordering, or coalescing hints cannot change the
logical result. An overflow sets one `full_reconciliation_required` bit rather
than growing an unbounded queue.

The first production backend is bounded polling over the existing sanitized
Git and contained-read boundary. Its request accepts an optional already
resolved ADR-0025 configuration; configuration discovery remains outside this
core. The effective source-poll interval comes from
`watcher_poll_interval_ms`, capped by the stricter mandatory complete
reconciliation interval. Compiled debounce, retry, overflow, and periodic
safety bounds remain authoritative. Native notification backends may reduce
latency later, but they feed the same supervisor and do not change the
correctness contract.

### One supervisor per source slot

One owned supervisor coordinates a configured source slot. It owns:

- one bounded dirty set and overflow bit;
- debounce and maximum-reconciliation intervals;
- one active reconciliation/index task;
- a bounded retry counter and backoff;
- the cancellation state; and
- non-sensitive counters and categorical outcomes.

The supervisor never starts a second index task for the same source slot.
Hints received during work mark the slot dirty for a later reconciliation.
Backpressure and coalescing are observable outcomes; the service does not
spawn unbounded work or queue every event.

### Durable monotonic source epochs

Source epochs are fixed-width monotonic values stored with the source slot.
One-shot and watched indexing use the same compare-and-set publication
contract.

A mere hint does not advance the epoch. A watched reconciliation atomically
reserves the next epoch only after complete source capture proves that the
canonical source state differs from the active state. An explicit one-shot
index invocation preserves its fresh-generation and stale-selector contract,
so each successfully prepared invocation reserves the next epoch even when
its immutable analysis artifacts are reusable. Overflow, cancellation, a
stale compare-and-set, or an exhausted epoch fails closed.

Staging carries the reserved epoch. After staging, a final complete source
fence must confirm the expected canonical snapshot before an immutable
completion receipt can bind that `(source slot, epoch)` to the generation.
Workspace-view publication requires the member epoch to remain current and
the exact receipt to exist. A newer proven source state advances the slot
again and prevents an older candidate from publishing. Cancellation, parser
failure, database failure, or a final source-fence mismatch leaves the
previous active generation and workspace view readable.

The hint queue is not durable. After process restart, bounded startup recovery
removes incomplete staging state and the supervisor performs a complete
reconciliation. A ready generation with an immutable completion receipt for
the source slot's current epoch survives recovery so a crash between receipt
commit and view publication can resume without invalidating that receipt.
Older receipts do not pin superseded ready generations. Correctness never
depends on replaying pre-crash events.

### Cancellation and shutdown

The CLI connects `SIGINT`, `SIGTERM`, or the platform console-cancellation
equivalent to one cooperative cancellation token before long-running work is
admitted. A second termination request may use normal operating-system forced
termination; RepoWitness does not install a handler that hides it.

On the first request the supervisor:

1. stops new work admission;
2. marks the active Git, read, parse, resolve, and SQLite operation cancelled;
3. waits only for a configured bounded shutdown interval;
4. performs no new checkpoint or activation after cancellation; and
5. exits with an explicit cancelled outcome.

SQLite transactions roll back through their owner. No synchronous lock or
transaction is held while awaiting timers or shutdown.

### Configuration and diagnostics

ADR-0025 supplies bounded debounce, polling, retry, and shutdown values. The
compiled hard ceilings remain authoritative. Repository configuration may
request slower polling or tighter work limits, but it cannot disable periodic
reconciliation, final source fencing, cancellation, or overflow recovery.

Diagnostics report:

- backend kind and profile version;
- last complete reconciliation outcome;
- active and observed source epochs;
- dirty, overflow, retry, cancellation, and backpressure counters;
- configured bounds; and
- unsupported host or path behavior.

They do not report source paths, event payloads, symbol names, configuration
file paths, or raw Git output.

## Alternatives considered

### Apply native events directly to the active index

This is low latency, but dropped or reordered events can make a partial view
look complete. It violates the canonical-manifest invariant.

### Persist and replay every event

Filesystem events are hints with host-specific semantics, not a durable source
log. Persistence adds an unbounded recovery surface without proving source
truth.

### Advance the epoch on every hint

This makes stale activation unlikely, but an event storm can starve all useful
work and exhaust the epoch without any semantic source change.

### Require a native watcher dependency immediately

Native notifications can improve latency, but polling already provides a
portable correctness backend. A new dependency should be justified by
measured latency or CPU behavior, not by correctness.

### Abandon worker threads on cancellation

Returning while a worker can still access SQLite or repository capabilities
creates hidden concurrent mutation. Work remains supervised and cooperative;
forced process termination is tested separately.

## Consequences

### Positive

- Watcher loss and event storms cannot make a partial generation current.
- Polling gives Linux, macOS, and Windows one shared correctness path.
- Durable epochs reject stale activation across retries and restarts.
- Cancellation reaches the existing Git, read, parse, and SQLite boundaries.
- Restart does not require a durable event queue.

### Negative and risks

- Complete periodic reconciliation consumes work even when events are quiet.
- Polling latency is bounded by an interval rather than immediate.
- A non-cooperative platform operation can delay bounded shutdown until the
  process is forcibly terminated.
- Epoch reservation and retained generation cleanup need coordinated storage
  limits.
- Native watcher adapters still require separate host-specific testing if
  added.

## Validation

- Deterministic unit fixtures that drop, duplicate, reorder, coalesce, and
  overflow hints yet converge to the clean manifest.
- Property tests proving dirty-set order and duplicate count cannot change the
  activated snapshot.
- Startup, idle-periodic, debounce, retry, overflow, and backpressure tests
  with injected clocks and events.
- Source changes before capture, during reads, during analysis, after staging,
  and immediately before activation.
- Cancellation at every Git, contained-read, parse, resolution, writer,
  commit, checkpoint, and wait boundary.
- First-signal graceful cancellation and process-kill tests on Linux, macOS,
  and Windows.
- Crash/restart tests proving incomplete work is removed and the first full
  reconciliation converges without an event replay.
- Epoch overflow, stale compare-and-set, concurrent supervisor, and mutation
  lease contention tests.
- Long quiet periods, rename storms, large dirty sets, deleted/recreated files,
  case-only renames, and unsupported path collisions.
- Clean versus watched incremental equality and ratified CPU, latency, memory,
  database, WAL, and retained-generation budgets.

## Follow-up

- Implemented under this accepted contract: the bounded polling
  supervisor, mandatory complete reconciliation schedule, durable monotonic
  source-slot epochs and completion receipts, final source fencing, and the
  CLI watch command with cooperative Unix and Windows shutdown signaling.
- Deterministic retention planning, exact apply, root revalidation, and
  aggregate audit are implemented under the accepted
  [ADR-0029](0029-bounded-generation-retention-and-garbage-collection.md).
- Maintainers accepted the configuration, connected-workspace, watcher,
  retention, migration, and resource-budget contracts after the Phase 1
  evidence gates passed.
- Add native watcher adapters only after measured latency or CPU evidence
  justifies them; they must continue to feed this reconciliation contract.

## Supersession

None.
