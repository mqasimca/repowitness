# ADR-0029: Bound generation retention and garbage collection

- Status: Accepted
- Date: 2026-07-28
- Owners: Project maintainers
- Scope: Local SQLite source generations, reusable analysis artifacts, source
  snapshots, search projections, reader pins, diagnostics, and cleanup

## Context

[ADR-0006](0006-immutable-index-generations.md) requires immutable staging and
active generations. Activation changes the previous active generation to
`retained`, but the current local store never removes retained generations.
Every successful reindex therefore grows generation mappings and the
disposable FTS projection even when artifacts are reused.

Cleanup is not a normal row-deletion problem. A retained generation can remain
necessary because it is:

- selected by an immutable connected-workspace view;
- pinned by an in-flight reader or supervised task;
- referenced by a memory projection, correspondence review, evidence record,
  or append-only audit;
- retained by policy for diagnosis, rollback, or reproducibility; or
- the only reachable owner of a source snapshot or analysis artifact.

Current schema triggers reject deletion of complete source snapshots, complete
artifacts, and their facts. Those triggers correctly prevent accidental
mutation, but a dedicated and testable lifecycle is needed for authorized
garbage collection. Disk pressure must never silently become permission to
delete a root or weaken configured retention.

## Decision

Implement deterministic mark-and-sweep garbage collection through the owned
SQLite writer. Garbage collection is an explicit maintenance operation in
Phase 1. Ordinary indexing does not silently delete retained data.

### Policy and limits

The resolved versioned configuration supplies a retention policy with:

- a minimum number of newest retained generations per source slot;
- explicit generation and task pins;
- one shared maximum for logical row work across root enumeration, candidate
  discovery, marking, dependent deletion, and audit publication;
- maximum generation candidates and estimated deleted bytes per transaction;
- an absolute operation deadline and cooperative cancellation; and
- bounded aggregate diagnostics for roots and unresolved candidates.

Security, administrator, user, workspace, and repository policy merge through
the monotonic rules in
[ADR-0025](0025-versioned-local-configuration-and-policy.md). A less-trusted
layer may increase a retention floor or reduce a work limit, but may not lower
a retention floor, remove a pin, increase a work limit, or enable automatic
deletion. A storage-size target is a diagnostic objective, not authority to
cross a retention floor.

The initial policy keeps the active generation and at least two newest retained
generations for every source slot. It imposes no age-based deletion by default.
Configuration version 1 has no retained-age field and age is not an eligibility
condition; adding one requires a new reviewed decision and versioned schema.
Maintainers must ratify changed defaults with storage-growth, rebuild, and
recovery evidence.

### Mark roots

Each collection pass computes roots from one consistent database snapshot.
Roots include:

1. every active generation and every generation selected by an active immutable
   workspace view;
2. bounded in-flight reader and supervised-task pins supplied by the
   application supervisor;
3. explicit policy pins and the per-source-slot count floor;
4. source generations or facts referenced by current or retained memory
   projections, correspondence reviews, evidence, or append-only audit state;
5. staging, ready, failed, or cancelled generations still owned by a live or
   recoverable operation; and
6. any object conservatively retained because its reachability could not be
   proven within the configured bound or deadline.

Marking then follows generation files and generation-scoped graph facts to
source snapshots, manifest entries, analysis artifacts, declaration facts,
correspondence fingerprints, and other typed artifact payloads. Shared
snapshots and artifacts remain live while any root reaches them.

Missing, malformed, over-limit, or cyclic metadata fails closed. It produces a
bounded diagnostic and retains the affected objects.

### Sweep lifecycle

Migration 3 adds explicit typed `garbage` mark relations and guarded
transitions:

```text
retained generation -> garbage
complete unreferenced snapshot -> garbage
complete unreferenced artifact -> garbage
```

The accepted migration-1 and migration-2 lifecycle `CHECK` constraints remain
byte-identical. Migration 3 therefore represents each transition in a typed
mark relation instead of rewriting an accepted table definition. Only the
owned writer's collection transaction may create and consume these marks.
The transaction first rechecks all roots and foreign-key references, marks a
bounded deterministic batch, removes dependent rows in dependency order, and
then removes the marked parent rows. The existing active-pointer, workspace
view, memory, and evidence foreign keys remain final backstops.

A sweep orders candidates by source slot, activation order, generation ID, and
byte identity. It does not depend on wall-clock ties, hash-map order, row
storage order, or filesystem enumeration.

Each batch is one immediate transaction. Cancellation, timeout, constraint
failure, or process termination rolls back the whole batch. Startup recovery
may resume deletion of objects already marked `garbage` only after recomputing
roots and proving that they remain unreachable. It never treats a stale mark as
deletion authority.

A new sweep requires every transient mark relation to be empty before it writes
its own plan-scoped marks. A foreign-plan mark fails closed without deletion or
audit publication; startup recovery revokes such marks without consuming their
targets.

### Reader and task pins

Queries pin an immutable workspace view and concrete generations before
reading. The application supervisor owns a bounded pin registry and stops
admitting a collection pass when the registry cannot represent every live pin.
New queries may pin only active or explicitly retained generations.

SQLite read transactions already preserve their own WAL snapshot. The
application pin additionally protects multi-step requests and future
historical selectors. A collection pass revalidates the bounded pin snapshot
immediately before its write transaction. Future multi-process historical
readers require a durable lease design before they can participate in
collection.

### Search projection and file size

The FTS projection is disposable. After a successful sweep, the writer removes
or rebuilds projection rows so they contain only generations selected by active
workspace views. Projection repair failure does not invalidate source
generations; it leaves search unavailable with an explicit diagnostic until a
bounded rebuild succeeds.

Row deletion does not promise immediate database-file shrinkage. WAL
checkpointing follows the accepted bounded policy. Automatic `VACUUM` is not
part of collection because it is a separate, potentially long-running rewrite.
`doctor` reports logical reclaim, page freelist, WAL state, and whether an
explicit offline compaction would help.

### Command and diagnostics

The local composition provides:

- a read-only plan mode that reports bounded counts, estimated bytes, roots,
  candidates, unresolved work, truncation, and total logical row work without
  opening a writer, taking the mutation lease, migrating, or recovering;
- `gc plan --database <path>` and an explicit
  `gc apply --database <path> --plan-digest <64-lowercase-hex>` mode bound to
  the same resolved policy and pins;
- stable outcomes for completed, no-op, cancelled, timed-out, blocked,
  stale-plan, and repair-required operations; and
- aggregate audit fields only: policy/plan digests, opaque workspace and source
  slot IDs, counts, estimated bytes, duration, and outcome.

The default CLI output and logs never include source text, memory content,
symbol names, repository paths, neighboring repository identities, or
credentials. A plan is stale if active views, source epochs, pins, policy, or
the candidate set changed; apply recomputes and rejects rather than following
the stale plan. Ordinary `index` and `watch` operations never invoke retention.

## Alternatives considered

### Keep every generation forever

This is simple and maximizes local history, but creates unbounded database and
projection growth. It also leaves no way to enforce explicit retention policy.

### Delete the previous generation during activation

This minimizes growth but couples publication to destructive work, lengthens
the activation transaction, removes diagnostic rollback history, and cannot
respect readers, tasks, workspace views, or memory evidence.

### Reference counts

Reference counts make individual deletions cheap, but every new reference type
must update counts perfectly across crashes and migrations. Mark-and-sweep is
easier to audit against authoritative relations. Materialized counts may be
added later only as verified hints.

### Delete on a disk-size threshold

A threshold alone cannot distinguish disposable derived state from evidence or
audit roots. It may report pressure and stop indexing, but it may not authorize
unsafe deletion.

### Drop immutability triggers during maintenance

Temporarily removing schema protections broadens the failure surface and makes
crash recovery difficult to prove. Explicit lifecycle transitions preserve the
guardrails throughout collection.

## Consequences

### Positive

- Repeated indexing has a bounded, explainable cleanup path.
- Active, pinned, evidence-referenced, and policy-retained state fails closed.
- Shared immutable artifacts remain reusable until no root reaches them.
- Collection is cancellable, deterministic, auditable, and independently
  repairable from the FTS projection.
- Publication remains short and non-destructive.

### Negative and risks

- Reachability queries and sweep ordering add schema and test complexity.
- Conservative failure can retain more data than the configured target.
- Append-only memory evidence may intentionally keep old artifacts alive.
- Logical deletion does not immediately reduce the SQLite file size.
- Multi-process historical readers remain unsupported until durable pins exist.

## Validation

- Fresh-version-3, populated-version-2 upgrade, exact ledger, and interrupted
  migration tests.
- Root fixtures for active views, current and retained memory projections,
  correspondence reviews, evidence, audits, explicit pins, task pins, and
  shared artifacts.
- Boundary tests for zero candidates, the exact count/row/byte limits,
  one-over-limit input, a shared logical-row budget spanning plan and sweep,
  deadline expiry, cancellation at every batch boundary, and candidate
  ordering ties.
- Concurrent activation, indexing, search, context, memory revalidation,
  backup, checkpoint, and collection tests.
- Process termination before mark, after mark, during dependent deletion, and
  before commit; restart must preserve roots and converge deterministically.
- Repeated no-op collection and equivalent databases with different insertion
  orders produce the same plan digest and logical result.
- FTS repair failure and restart rebuild leave source generations readable and
  never expose deleted generations as active.
- Long-lived-reader and supervised-task pin tests, including pin-registry
  overflow that blocks collection.
- Foreign-key, trigger, integrity, backup/restore, and freelist diagnostics
  after every fault injection point.
- Privacy tests prove normal output, errors, audit rows, and logs omit source,
  memory, symbol, repository-path, environment, and credential text.
- Storage benchmarks cover unchanged and single-file reindex loops, shared
  artifacts, evidence-retained artifacts, bounded sweep batches, WAL growth,
  checkpoint latency, and logical versus physical reclaimed bytes.

## Implementation status

Implemented as an accepted Phase 1 contract.
Migration 3 contains typed generation, snapshot, artifact, workspace-view, and
source-slot-receipt garbage marks plus append-only aggregate collection audit.
The owned writer implements deterministic read-only planning, stale-safe
immediate-transaction apply, exact replay idempotency, bounded root and
candidate evaluation, cancellation/deadline handling, and fail-safe startup
revocation of stale marks. The local facade and CLI expose explicit `gc plan`
and exact digest-bound `gc apply`; plans include aggregate root, unresolved,
truncation, and shared logical-row-work metrics. Acceptance still requires
maintainer ratification of the default floor and budgets plus the remaining
release evidence named above.

## Supersession

None.
