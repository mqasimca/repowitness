# ADR-0005: Model Git-DAG project validity and recorded time separately

- Status: Accepted
- Date: 2026-07-22
- Last reviewed: 2026-07-28
- Owners: Project maintainers
- Scope: Memory validity, historical queries, branches, rebases, and audit history

## Context

RepoWitness promises to distinguish when engineering knowledge applied in a project from when the system learned it. A single timestamp cannot answer both questions. A linear `valid_from`/`valid_to` commit interval is also incorrect because Git history branches and merges.

Branch names move, worktrees may be dirty, clones may be shallow, and commits may be rewritten or unavailable. The system must not label knowledge current when ancestry cannot be established.

## Decision

Use two explicit time axes.

### Project-valid time

A record version contains sets of `introduced_by` and `invalidated_by` Git commits plus repository, worktree, path, symbol, and policy scope. A commit ID stores the repository object format and raw object ID; the model does not assume SHA-1.

For query revision `R`, the record is eligible when:

1. at least one applicable introduction commit is an ancestor of `R`;
2. no applicable invalidation commit is an ancestor of `R`;
3. all other scope predicates match;
4. the record lifecycle permits retrieval.

Branch names are selectors resolved to a concrete commit before evaluation. Dirty worktree evidence is tied to a content-digested snapshot and is eligible only for that exact snapshot. It does not imply validity for later commits until a reviewed/imported version establishes a commit-scoped introduction event.

If required objects or ancestry are unavailable, validity is `indeterminate`. The caller may request more history or review the record; RepoWitness does not convert missing evidence into validity.

### System-recorded time

Edits create immutable record versions in a digest-linked version DAG. Each version has:

- record ID, canonical content digest, and zero or more parent revision digests;
- a display revision/sequence that is not used as the concurrency or identity primitive;
- `recorded_at` and a derived or stored `recorded_until`;
- actor, origin, operation, and evidence in an append-only audit event.

An “as known at” query selects the record version visible at the requested recorded time, then evaluates its project validity at the requested repository revision.

Contradiction and supersession are relationships between immutable versions; they do not rewrite historical results.

## Alternatives considered

### One wall-clock interval

Easy to implement but cannot represent repository applicability or branch divergence.

### One linear commit interval

Works for a simplified mainline but misrepresents parallel branches, merges, and invalidation that exists on only one descendant path.

### Branch-name scoping alone

Branch names are mutable pointers and may be deleted or force-updated. They remain useful selectors and policy labels, not temporal identity.

### Destructive record updates

Simple current-state queries but prevents reliable historical “as known at” answers and weakens auditability.

## Consequences

### Positive

- Historical answers can explain both project applicability and knowledge availability.
- Branch-specific invalidation follows commit ancestry naturally.
- Missing history produces an honest indeterminate result.
- Immutable versions support audit, conflict preservation, and projection rebuild.

### Negative and risks

- Ancestry queries and caching add cost.
- Force pushes and pruned history complicate durable references.
- Multiple introduction/invalidation events require careful user explanations.
- Recorded time from Git commits and local uncommitted changes needs a deterministic ordering policy.

## Validation

Fixtures must cover:

- linear introduction and invalidation;
- two branches with invalidation on only one;
- merge before and after invalidation;
- cherry-pick and equivalent content with different commit identity;
- rebase/force-push and missing original commits;
- shallow clone with unavailable ancestry;
- dirty worktree digest changes;
- dirty-worktree memory committed and rebound to a concrete introduction commit;
- SHA-1 and SHA-256 repository object IDs;
- conflicting, superseding, and tombstoned record versions;
- “as known at” before and after an imported Git commit.

## Open questions

- Whether equivalent cherry-picked evidence creates a new introduction event or an explicit equivalence link.
- Ordering and display rules for uncommitted local versions relative to Git commit time.
- Portable archival/export format and default retention period for unreachable historical commits and their imported memory versions; ADR-0007 requires local retention plus explicit rebuild coverage in the interim.
- Whether `gix` should replace or supplement the production sanitized Git CLI
  for history traversal after cancellation and performance spikes.

## Implementation status

Canonical SHA-1/SHA-256 Git and dirty-worktree source receipts are implemented
for indexing, including explicit shallow state and concurrent-mutation
fencing. Immutable memory versions, introduction/invalidation ancestry,
branch-aware validity, shallow-history coverage, approved-head conflicts, and
indeterminate effective states are implemented. Historical “as known at”
queries and portable retention/export for pruned objects remain unimplemented.

## Supersession

None.
