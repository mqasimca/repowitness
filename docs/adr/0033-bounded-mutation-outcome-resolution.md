# ADR-0033: Resolve mutation outcomes without denying committed work

- Status: Proposed
- Date: 2026-07-29
- Owners: Project maintainers
- Scope: Local mutation deadlines, receipts, path identity, file publication,
  online backup, SQLite writer commands, and post-commit maintenance

## Context

RepoWitness mutates several independently durable resources: canonical memory
files, online-backup destinations, and SQLite state owned by a writer thread.
Each operation has an absolute deadline and cancellation signal. Those controls
bound authority to start or continue work, but they do not make a commit and
its reply one atomic action.

A process can successfully rename, link, or commit and then miss the receipt
because verification, cleanup, checkpointing, directory synchronization,
thread scheduling, or reply delivery crosses the caller's deadline. Returning
an ordinary cancellation, deadline, or publication error in that state falsely
claims that retrying is safe. For non-idempotent mutations, an automatic retry
can duplicate audit history or overwrite a later state.

Path identity has the same boundary. A stable opened handle can authorize the
resource used for a commit, while the lexical path is replaced immediately
afterward. The commit remains real even when the final path fence fails.

[ADR-0012](0012-phase0-sqlite-schema-and-ownership.md) already requires owned
connections, bounded one-shot replies, and online backup.
[ADR-0021](0021-phase0-memory-management-and-review.md) establishes atomic
canonical memory publication with directory synchronization. Neither decision
defines how callers distinguish a pre-commit failure from a committed result
whose maintenance or receipt was not confirmed.

## Decision

### Three mutation outcomes

Every local mutation reports exactly one of these categories:

1. **Definitely not committed.** Validation, authorization, cancellation,
   deadline, resource, identity, or storage failure occurred before the
   operation's commit point. The operation returns a stable redacted error.
2. **Committed.** A concrete receipt identifies the committed result.
   Post-commit identity and maintenance facts are categorical fields on that
   receipt. Warnings never replace or erase the receipt.
3. **Outcome unknown.** A mutating worker accepted work but no definitive
   receipt or pre-commit error arrived within bounded outcome-resolution time.
   The stable error is `MutationOutcomeUnknown`.

`MutationOutcomeUnknown` means only that no receipt arrived. It does not imply
success, rollback, corruption, or permission to retry. Callers must reconcile
authoritative state before retrying a non-idempotent operation.

### Operation deadline and resolution grace

The operation deadline remains the authority boundary used by discovery,
SQLite progress handlers, backup page steps, and pre-commit checks. Once the
deadline elapses, the caller signals cancellation and waits only for a short,
fixed outcome-resolution grace. The local online-backup profile uses at most
250 milliseconds.

Grace observes a result; it does not extend mutation authority. A definitive
receipt or pre-commit error arriving during grace is returned unchanged. If no
receipt arrives by the end of grace, the caller returns
`MutationOutcomeUnknown` without an unbounded thread join. Read-only commands
retain ordinary reply-timeout semantics.

The same separation applies to owner-thread SQLite mutations. Successful queue
admission followed by loss of the one-shot reply is outcome-unknown.
Queue-full or deadline failure before admission is definitely not committed.

### Commit points

The commit point is explicit for every mutation family:

- creating a canonical memory record commits when the no-clobber hard link to
  the canonical target succeeds;
- updating a canonical memory record commits when the same-directory atomic
  rename succeeds;
- an online backup commits when the no-clobber hard link creates the
  destination;
- a SQLite mutation commits when its transaction commit succeeds; and
- generation or projection publication commits when its atomic active pointer
  transaction commits.

Every conversion, count check, validation required to construct the receipt,
and precondition that can still reject publication runs before the commit
point. After the commit point, cleanup, verification, synchronization,
checkpointing, shutdown, and reply delivery cannot turn the outcome into an
ordinary rollback-shaped error.

### Identity fences and categorical warnings

Filesystem mutations retain an opened no-follow authority for the source,
temporary file, destination directory, or database as applicable. The
implementation verifies stable file identity, regular-file type, link policy,
and current lexical-path correspondence before committing.

A failed pre-commit identity fence prevents publication. A failed fence after
commit produces a receipt with `ChangedAfterCommit`. Identity confirmation is
truth at the named final fence, not a promise that another process cannot
replace the path later.

Post-commit maintenance is reported as `Complete` or `Deferred`. Steps that do
not apply may be `NotRequired`. Current receipts expose:

- canonical-memory target identity, records-directory identity, temporary
  cleanup, and directory synchronization;
- backup source identity, destination identity, private temporary cleanup, and
  destination-directory synchronization; and
- terminal SQLite checkpoint, shutdown, and database-path confirmation when
  those mutation facades adopt this contract.

Errors and warnings remain path-, source-, query-, credential-, and
content-redacted. Raw operating-system or SQLite error text does not cross the
public boundary.

### Retry and recovery

RepoWitness does not automatically retry a non-idempotent unknown mutation.
Recovery first reads an authoritative receipt, idempotency key, immutable row,
canonical file digest, active pointer, or destination identity. A later remote
or multiprocess profile may require durable operation IDs and a receipt
journal; this proposal does not invent that protocol for the local profile.

## Alternatives considered

### Treat the operation deadline as proof of rollback

This is simple for callers but false once a commit syscall or SQLite
transaction succeeds before reply delivery.

### Return an ordinary error after cleanup or synchronization fails

This preserves the old `Result` shape but encourages unsafe retry and hides the
already-visible destination. A committed receipt with warnings states both
facts.

### Wait indefinitely for the worker

Joining proves more outcomes but violates the bounded local-operation contract
when a filesystem, SQLite, or worker thread stalls.

### Always return outcome unknown at the operation deadline

This discards definitive receipts that are already in flight. A short bounded
grace resolves ordinary scheduling races without authorizing more mutation
work.

### Add durable operation IDs immediately

They would improve reconciliation, but require a new persisted and wire
contract across every mutation family. The local alpha can first expose honest
outcomes through existing concrete receipts.

## Consequences

### Positive

- Callers never receive a rollback-shaped error for known committed work.
- Retry decisions can distinguish definite failure, committed work with
  warnings, and uncertainty.
- Deadline handling remains bounded without discarding a late definitive
  receipt.
- File and database replacement races become explicit identity evidence.
- Cleanup and durability limitations remain visible without hiding the primary
  result.

### Negative and risks

- Public receipts gain categorical status fields that adapters must render.
- Callers must reconcile `MutationOutcomeUnknown` instead of blindly retrying.
- A detached worker may finish after the caller returns outcome-unknown.
- Final-fence confirmation cannot prevent a replacement after the fence.
- Conservative warnings may remain even if best-effort cleanup succeeds after
  the recorded fence.
- The 250-millisecond local grace needs release-platform measurement.

## Validation

- Inject failure before every memory-file commit point and prove no canonical
  target is created or replaced.
- Inject cleanup, target-verification, and directory-sync failure after memory
  publication and prove a receipt plus categorical warnings is returned.
- Replace or alias published memory targets and prove committed bytes are not
  described as rolled back.
- Replace or hard-link the backup source before publication and prove the
  destination is not created.
- Replace or alias backup paths after publication and prove the receipt reports
  changed identity while preserving the known commit.
- Delay backup reply delivery until inside and beyond resolution grace; assert
  exact receipt versus `MutationOutcomeUnknown` and a wall-clock upper bound.
- Inject temporary cleanup and directory-sync failures after backup
  publication and validate the destination independently.
- For owner-thread SQLite mutations, pause after transaction commit but before
  reply delivery; cover receipt inside grace, no receipt beyond grace,
  disconnect, queue-full before admission, read-only reply timeout, and
  immediate client drop after outcome-unknown.
- Run the publication, backup/restore, cancellation, crash/recovery, Unix
  hard-link/symlink, Windows reparse/link, and redacted-diagnostic matrices.

## Follow-up

- Keep CLI and MCP receipt schemas, committed warnings, and operation-specific
  reconciliation guidance aligned with every local mutation facade.
- Preserve the shared owner fence that rejects later and already-queued
  mutations after an unresolved receipt until the store is reopened and
  authoritative state is reconciled.
- Measure the resolution grace on supported release platforms before
  accepting this ADR.
- Revisit durable operation IDs before remote mutation or multiple concurrent
  process owners are supported.

## Implementation status

The canonical memory-file and online-backup commit-point slices implement this
proposal, including separate target, records-directory, source, and destination
identity categories; no-follow identity fences; post-commit maintenance
warnings; bounded backup outcome resolution; and adversarial fault hooks.
Canonical-memory status is rendered in both CLI and MCP write receipts.
Owner-thread SQLite startup and mutating commands now use the same bounded
receipt-resolution transport: known replies during the 250-millisecond grace
are preserved, while a missing or disconnected reply after queue admission is
`MutationOutcomeUnknown`. Read-only commands retain ordinary reply-timeout
semantics. A test-only owner-thread seam pauses successful mutations after the
transaction returns but before reply delivery. Real SQLite integration tests
cover a receipt released inside grace, a durable commit whose receipt is
withheld beyond grace, queue-full rejection before admission, and an admitted
read-only command that retains reply-timeout behavior. Bounded transport tests
separately cover reply-channel disconnect.

The local indexing, connected-workspace, memory-management, memory-revalidation,
and retention facades preserve an explicit outcome-unknown category. They
expose operation-specific, path- and content-redacted guidance for reconciling
authoritative state before retry. Automatic local-index polling does not retry
an unknown mutation.

The explicitly authorized MCP `memory_manage` request remains version 1, while
its receipt schema is version 2. Approval, correspondence-review, and
history-import receipts carry categorical checkpoint and shutdown maintenance
state plus final database-path identity evidence: `ConfirmedAtFinalFence`,
`ChangedAfterCommit`, or `Unconfirmed`. The aggregate `complete` field is true
only when both maintenance steps and the identity fence are confirmed; changed
or unconfirmed identity remains a warning on the known durable receipt. The
alpha boundary does not retain constructors that silently label unknown
maintenance as complete.

The SQLite writer client and owner share a sticky unresolved-mutation state
after outcome-unknown or failed mutation-reply delivery. Later mutations fail
before queue admission, and a mutation already queued behind the unresolved
operation is rejected before execution. Read-only reconciliation and orderly
shutdown remain available; reopening after authoritative reconciliation starts
a new owner-local outcome state. Drop does not treat successful
shutdown-command admission as proof that an unresolved owner reached shutdown:
it joins only an already-finished owner and otherwise detaches. A real SQLite
regression pauses after checkpoint completion, returns outcome-unknown, invokes
consuming shutdown with an expired deadline, and proves the resulting drop is
bounded while the owner remains paused. The concurrency regression queues a
second mutation before the first receipt becomes unknown and proves that only
the first transaction can be durable. Reopen then permits a reconciled retry.
The unknown diagnostic remains generic and path-redacted.

Acceptance still requires measuring the 250-millisecond resolution grace on
each supported release platform and completing the full release validation
matrix above.

## Supersession

None. This proposal clarifies the outcome semantics of accepted ADR-0012 and
ADR-0021 without changing their storage, trust, or ownership choices.
