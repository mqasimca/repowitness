# ADR-0031: Resolve source-slot selectors in caller-provided worktrees

- Status: Accepted
- Date: 2026-07-29
- Owners: Project maintainers
- Scope: Source-slot configuration, Git selection, package scope, final fencing, and local indexing

## Context

[ADR-0026](0026-connected-workspace-source-slots-and-views.md) defines opaque
source slots and requires moving Git selectors to resolve before persistence.
It does not define accepted selector spellings, package selection, or how a
selected revision becomes readable source.

RepoWitness already has a capability-contained worktree reader and a complete
source-state fence. Adding direct Git-object-tree reads would create a second
source adapter with separate path, symlink, submodule, sparse-checkout,
cancellation, and resource-limit behavior. Creating or changing worktrees
would also mutate caller state.

The primary-source review on 2026-07-29 confirmed these relevant Git
contracts:

- `git rev-parse --verify` with the `^{commit}` peel verifies that a selector
  resolves to one commit, and `--end-of-options` prevents an untrusted
  selector from becoming an option
  ([git-rev-parse](https://git-scm.com/docs/git-rev-parse));
- linked worktrees have distinct `HEAD` and index state while most repository
  state is shared, and Git recommends commands instead of direct
  administrative-path assumptions
  ([git-worktree](https://git-scm.com/docs/git-worktree));
- porcelain status is the stable script boundary for dirty worktree state
  ([git-status](https://git-scm.com/docs/git-status)); and
- ref names have a dedicated validation command
  ([git-check-ref-format](https://git-scm.com/docs/git-check-ref-format)).

## Decision

### Versioned selectors

Define an internal version-1 source selector with three categories:

1. `worktree-head`: select the caller-provided worktree's concrete `HEAD`
   commit;
2. `exact-revision`: select one full object ID whose width matches the
   repository's reported object format; and
3. `full-ref`: select one validated, fully qualified name under
   `refs/heads/`, `refs/tags/`, or `refs/remotes/`.

Configuration text is UTF-8, contains no NUL or control characters, and is at
most 1,024 bytes. Exact revisions do not accept abbreviated IDs. Full refs
must use one of the allow-listed namespaces and pass both RepoWitness's
length/character admission and `git check-ref-format`; short-name
disambiguation and other ref namespaces are not supported.

Resolution uses the existing sanitized, no-shell Git subprocess boundary with
fixed arguments, disabled prompts, hooks, pagers, external diff, optional
locks, and fsmonitor, plus bounded stdout, stderr, time, and cancellation.
`git rev-parse --verify --quiet --end-of-options <selector>^{commit}` must
return exactly one full object ID. The object format and raw object ID remain
typed together until the source-state receipt is constructed.

### Caller-provided worktrees only

Every selector is evaluated in an explicit caller-authorized worktree.
RepoWitness does not run checkout, switch, reset, worktree add/remove, fetch,
or any other command that changes Git or filesystem state.

The worktree's concrete `HEAD` must equal the resolved selector commit before
source discovery. To index another branch or revision, the caller supplies a
worktree already checked out at that commit. Multiple supplied linked
worktrees may map to the same logical repository through distinct source
slots.

A dirty worktree is allowed. Its indexed evidence is bound to the exact
content manifest and worktree-state digest. It never gains commit-wide or
descendant validity merely because its `HEAD` matches the selector.

### Moving-selector fence

The resolved commit, selector category, and a digest of any moving full-ref
selector stay in process memory for one reconciliation. Raw selector text is
not persisted, logged, returned by diagnostics, or included in public errors.

The final source fence repeats selector resolution. A moving ref that no
longer resolves to the same commit rejects the candidate before completion or
view publication. Exact revisions still repeat the complete worktree,
manifest, database-alias, cancellation, and deadline fence.

### Explicit package scopes

A source slot selects either the whole worktree or a bounded set of explicit
repository-relative package roots. Version 1:

- accepts at most 64 roots;
- validates each root with the accepted repository-path rules;
- sorts by exact path bytes;
- rejects duplicates and ancestor/descendant overlaps; and
- treats the repository root as the separate whole-worktree category rather
  than an empty path.

Discovery and the final fence apply the same scope. Supported-language paths
outside the scope are explicit policy omissions. The canonical package-scope
encoding contributes to the resolved configuration and source-snapshot
digests, so a scope change cannot reuse or publish a mismatched snapshot.

“Package” in this decision means a caller-named source root. RepoWitness does
not infer Cargo workspaces, Go modules, Python projects, JavaScript packages,
or dependency graphs in Phase 1. It does not claim package-aware symbol
resolution. Those are separate evidence producers and remain outside the
Rust syntax-graph contract.

### Connected-workspace input

One internal connected-workspace indexing request contains:

- one explicit connected-workspace ID;
- one to the compiled maximum number of source-slot requests;
- for each slot, an explicit source-slot ID, logical repository identity,
  authorized worktree root, selector, package scope, and resolved
  configuration; and
- one shared cancellation token plus explicit per-slot and whole-operation
  deadlines.

Host roots and selector text are adapter inputs only. Persistence receives
opaque IDs, concrete source-state receipts, generations, epochs, completion
receipts, and immutable view membership.

The coordinator may prepare slots concurrently only within an owned bounded
pool. It publishes a new immutable workspace view only after every requested
slot has a complete eligible generation for its current epoch. Any failure,
stale selector, cancellation, or deadline leaves the prior view readable.

## Alternatives considered

### Read arbitrary commits directly from the Git object database

This avoids caller-created worktrees and can be efficient for clean commits.
It requires a second hostile-source adapter and cannot represent a dirty
worktree without an overlay model. Revisit it only after an exact object-tree
manifest and containment contract has independent tests and benchmarks.

### Create temporary worktrees automatically

This gives convenient revision access but mutates shared Git administrative
state and adds cleanup, locking, hook, path, credential, and crash-recovery
risks. The local core remains read-only with respect to caller repositories.

### Accept short branch names and abbreviated object IDs

They are convenient but can be ambiguous or change meaning as refs and
objects change. Version 1 accepts only explicit full refs and full object IDs.

### Infer package managers and dependency graphs

Inference would improve ergonomics, but five supported languages have
different manifests, workspaces, build constraints, and generated-source
rules. Explicit bounded roots provide honest package scoping without claiming
package semantics.

## Consequences

### Positive

- Branches, tags, revisions, and linked worktrees reduce to one exact source
  adapter and one final-fence contract.
- RepoWitness does not mutate caller Git state.
- Moving refs cannot silently retarget a staged candidate.
- Dirty evidence remains snapshot-scoped.
- Explicit package roots are deterministic and language-neutral.
- No host root or raw selector enters persistence or default diagnostics.

### Negative and risks

- Callers must create or select a worktree before indexing another revision.
- Remote refs are only as current as the caller's local repository.
- UTF-8 configuration cannot name every byte sequence that Git may permit in
  a ref.
- Package roots do not provide dependency or package-manager semantics.
- Multi-slot preparation costs complete source reconciliation per affected
  worktree.

## Validation

- SHA-1 and SHA-256 repositories with worktree-head, exact-revision, and
  full-ref selectors.
- Symbolic and detached `HEAD`, unborn `HEAD`, annotated tags, missing objects,
  ambiguous short names, malformed refs, option-like text, NUL/control
  characters, exact length limits, and one-over-limit values.
- A moving ref before capture, during analysis, after graph staging, and
  immediately before completion.
- Clean and dirty main/linked worktrees at equal and different commits,
  including the same logical repository in two source slots.
- Whole-worktree and package-root scopes with empty, duplicate, overlapping,
  case-distinct, non-UTF-8, deleted, symlink, sparse, gitlink, and over-limit
  paths.
- Cancellation, deadline, Git-output limit, subprocess failure, mutation-lease
  contention, stale epoch, and crash/restart at every coordinator phase.
- Atomic two-repository view publication and prior-view readability after
  every injected failure.
- Persistence, error, debug, diagnostics, and privacy-canary checks proving
  that roots and selector text are absent.

## Follow-up

- Implemented under this accepted contract: validated selectors and
  package scopes, bounded sanitized Git resolution and final revalidation,
  explicit source-slot requests, and atomic internal multi-source
  coordination.
- The versioned CLI surface is implemented through the explicit manifest
  contract in accepted
  [ADR-0032](0032-explicit-connected-workspace-manifest.md).
- Maintainers accepted ADR-0026, this ADR, ADR-0032, migration 3, and the
  associated cross-platform and resource budgets after the Phase 1 evidence
  gates passed.
- Revisit direct Git-object-tree reads only through a separate measured spike
  with its own containment, identity, and dirty-worktree model.

## Supersession

None. This refines the accepted ADR-0026 selector requirement without
changing accepted repository, source-state, path, or temporal-validity
decisions.
