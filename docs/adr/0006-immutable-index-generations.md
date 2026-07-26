# ADR-0006: Publish indexes through immutable generations

- Status: Accepted
- Date: 2026-07-22
- Owners: Project maintainers
- Scope: Index writes, reads, cancellation, crash recovery, and incremental correctness

## Context

Indexing changes several related tables: files, occurrences, symbols, edges, evidence, and coverage. Updating active rows in place can expose mixed old/new state to readers, especially when parsing is parallel or cancellation and crashes interrupt the pipeline.

RepoWitness also needs a clear unit for deterministic queries, evidence receipts, cache keys, and clean-versus-incremental equivalence.

## Decision

Use immutable index generations per workspace, backed by reusable content-addressed artifacts.

1. Readers pin one active generation for the duration of a request.
2. Discovery builds a canonical sorted `SourceSnapshot` manifest from exact normalized paths, file types, content digests, Git identity, resolved configuration, and producer versions.
3. A per-file analysis artifact is addressed by a versioned digest over source content plus every semantics-affecting adapter/configuration input. Unchanged inputs reuse the immutable artifact.
4. A staging generation maps repository paths to source/artifact identities and stores generation-scoped cross-file facts, producer metadata, coverage, and validation state.
5. Parsing and resolution persist staging data in bounded batches through one owned transactional writer; publication does not require one unbounded transaction.
6. Activation is one short atomic transaction that changes the workspace's active-generation pointer only after validation succeeds.
7. Cancellation, a newer source epoch, or failure marks/discards the staging generation and leaves the previous active generation readable.
8. Restart recovery either resumes a provably restartable stage or discards incomplete staging data deterministically.
9. Garbage collection marks from active/retained generations, pinned readers/tasks, audit/evidence references, and retention policy, then sweeps unreachable generations, artifacts, and source blobs.

Filesystem watcher events only populate a debounced dirty set. Native watchers, polling, and explicit reconciliation all feed the same manifest builder; correctness never assumes the event stream is complete.

Incremental indexing may reuse unchanged facts internally, but its observable active generation must be logically equivalent to a clean rebuild for the same source snapshot, configuration, and producer versions.

## Alternatives considered

### Update active rows in place

Minimizes duplicated rows but can expose partial state, complicates rollback, and makes request-level provenance unclear.

### Lock the entire graph during updates

Prevents mixed reads but blocks queries for the duration of indexing and creates a large shared-state bottleneck.

### Event sourcing only

Provides detailed history but requires complex replay/materialization for ordinary queries. Generation metadata and memory audits can retain events without making the entire index an event-sourced system.

## Consequences

### Positive

- Readers see a coherent snapshot with one generation ID.
- Failure and cancellation preserve the last known-good index.
- Coverage and producer/configuration metadata attach to a precise snapshot.
- Differential clean/incremental testing has a clear comparison unit.
- Unchanged files reuse artifacts without copying every fact into every generation.
- Watcher loss cannot silently define an incorrect snapshot.

### Negative and risks

- Staging and retained generations consume storage.
- Artifact keys must include every semantics-affecting input or stale analysis may be reused.
- Reuse and mark-and-sweep garbage collection require reachability accounting.
- Long-lived readers can delay cleanup.
- Atomic activation semantics must be preserved in any future backend.

## Validation

- Kill/cancel tests during discovery, parse, resolution, write, validation, and activation.
- Concurrent readers during activation.
- Restart with incomplete staging data.
- File-event storms and duplicate updates.
- Missed watcher events, editor atomic-replace behavior, and polling reconciliation.
- Clean-versus-incremental logical equivalence.
- Generation retention and cleanup with pinned readers/tasks.
- Adapter/configuration version changes invalidate artifact reuse.
- Digest collision handling and corrupted artifact detection.
- SQLite backup/recovery across activation.

## Open questions

- Whether to retain complete source blobs, searchable fragments, or digest-only content for selected file classes.
- Retention defaults and size thresholds for historical generations/artifacts.
- Whether cross-file resolution partitions merit content-addressed reuse after Phase 0.
- Exact activation/retry behavior when reconciliation observes a newer source epoch during a build.

## Implementation status

Implemented for the local Rust index: canonical manifests, complete artifact
keys and payload digests, exact reuse, bounded staging, immutable generations,
atomic activation, pinned readers, stale-epoch rejection, cancellable recovery,
corruption rejection, backup/restore, and clean-versus-incremental
equivalence. Pure reconciliation tests prove watcher hints cannot define
logical output; production watcher ingestion and retention/garbage collection
remain.

## Supersession

None.
