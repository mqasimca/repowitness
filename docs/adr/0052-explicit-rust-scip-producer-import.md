# ADR-0052: Produce and import one Rust SCIP overlay through explicit local execution

- Status: Proposed
- Date: 2026-08-02
- Owners: Project maintainers
- Scope: direct CLI, local Rust SCIP production, and existing SCIP-overlay import

## Context

The exact declaration-to-SCIP bridge and relationship reads are useful only
when an overlay was previously imported. A normal source index deliberately
does not execute a compiler, package manager, or external producer, so it
truthfully returns `not_produced` when no overlay exists.

The upstream [scip-rust](https://github.com/scip-code/scip-rust) documentation,
checked on 2026-08-02, identifies `rust-analyzer scip` as the Rust producer and
supports an output-path argument. Requiring an agent to create a persistent
artifact and then issue a separate import makes the precision path unnecessarily
hard to use, while integrating producer execution into ordinary indexing would
weaken the hostile-workspace and predictable-indexing boundaries.

## Decision

Add the direct CLI-only `scip-rust-import` command.

- The user explicitly supplies the database and repository root. They select
  either an ordinary indexed repository identity (which deterministically maps
  to its compatible single-repository workspace and source slot) or an explicit
  connected workspace and source slot, and may select an exact workspace view.
- It invokes only `rust-analyzer scip . --output <private-temporary-file>` with
  the supplied root as the working directory. The executable defaults to
  `rust-analyzer` and can be selected explicitly.
- Producer and import deadlines are independently bounded. Producer output and
  diagnostics are not forwarded, and the temporary artifact is removed on a
  best-effort basis after the command returns.
- The generated file is admitted only through the existing no-follow,
  source-fenced, immutable-view `scip-import` path. Failed production, a bad
  artifact, changed source, cancellation, or failed import leaves the prior
  overlay readable.
- Normal `index`, `onboard`, and `watch` never run a producer. The read-only
  MCP server never exposes this operation. No general producer registry,
  package-manager execution, downloaded tool, background process, or second
  language adapter is introduced.

## Alternatives considered

### Automatically run rust-analyzer during every index

Rejected. It changes the cost and trust boundary of source indexing and would
allow repository-controlled build configuration to affect ordinary indexing.

### Require separate manual production and import commands

Rejected. Both operations must select the same current source slot and view;
one explicit command can compose the existing safe import path without
retaining an artifact in the worktree.

### Introduce a generic compiler or SCIP-producer plugin framework

Rejected. It would broaden the execution surface, policy model, maintenance
burden, and language scope before a named need exists.

## Consequences

### Positive

- Rust users can turn an explicit producer invocation into exact SCIP evidence
  without leaving a persistent index artifact in their repository.
- `not_produced` remains a truthful categorical result when the command was not
  explicitly run or its output could not be admitted.
- Existing overlay identity, source fences, and relationship attribution remain
  the only basis for semantic relationship claims.

### Negative and risks

- The command executes a locally installed external tool against a user-chosen
  workspace and can take substantially longer than lexical indexing.
- A producer may be unavailable, fail, or generate partial SCIP. RepoWitness
  preserves those boundaries rather than inferring missing relationships.
- The command adds a direct CLI contract and an upstream producer compatibility
  dependency that must be retested when Rust toolchains change.

## Validation

- A black-box synthetic producer emits one valid SCIP overlay. The CLI imports
  it, then `scip-evidence` returns `found` with the persisted relationship.
- Existing import fixtures cover no-follow reads, stale source, invalid
  artifacts, atomic overlay publication, and prior-overlay preservation.
- Direct sibling-worktree smoke testing exercises ordinary no-overlay SCIP
  evidence and trace categories without executing producer tooling.

## Follow-up

- Measure real Rust workspaces before proposing another producer or automatic
  producer policy.
- Keep unresolved/unsupported native Rust graph coverage distinct from
  compiler-produced SCIP evidence.

## Supersession

None. This composes with ADR-0035, ADR-0037, ADR-0045, and ADR-0048.
