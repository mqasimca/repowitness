# ADR-0050: Opt-in Codex catalog onboarding

- Status: Proposed
- Date: 2026-08-02
- Owners: Project maintainers
- Scope: Local Codex integration, private catalog state, and MCP startup

## Context

The explicit `onboard` command and the fixed multi-repository MCP registry are
safe but ask an operator to repeat setup work for every repository. That is
unnecessarily different from a coding agent's normal workflow: start Codex in
a worktree and immediately use code-discovery tools. A catalog mode must not
turn a model-controlled request into authority to walk the home directory,
discover siblings, mutate source trees, or select arbitrary databases.

Codebase-memory-mcp demonstrates the desired interaction shape: one installed
MCP connection has persistent local project state, and an opt-in automatic
indexing setting admits the project of a new session. As observed on 2026-08-02,
its public [README](https://github.com/DeusData/codebase-memory-mcp) describes
that setting plus a shared daemon and watcher. Those broader mechanisms—an
open-ended project store, background watcher, and cross-repository graph—are
not Phase 0 RepoWitness requirements.

## Decision

Add an opt-in `mcp-serve --catalog` startup mode intended for one global Codex
entry. At process startup it resolves only the process current directory to its
containing Git worktree. It never walks siblings, parents beyond that one
worktree-root resolution, configured roots, or arbitrary caller input. It
indexes or incrementally refreshes that one worktree before MCP starts, then
records it in a private local catalog only after the complete index activates.

The catalog is a bounded versioned no-follow control file under a private
user-state root. Direct `mcp-serve --catalog` uses the normal onboarding state
selection; the Codex installer fixes a private `repowitness-state` child of the
Codex home as its explicit state root, avoiding ambient user-state layout
assumptions. It contains at most 32 isolated entries, each with an opaque
repository ID, canonical absolute worktree root, and its private SQLite
database. Catalog data is never returned to MCP callers. A failed
current-worktree admission leaves the prior catalog and every existing
generation readable. Each MCP process loads one immutable catalog snapshot;
the next Codex session sees later successful admissions.

Catalog MCP retains the canonical read-only surface. For the worktree captured
at startup, `repository_id` is optional and defaults only to that exact
process-fixed catalog entry. Cross-catalog access requires one exact opaque
ID. Callers cannot pass roots, database paths, source slots, actors, mutation
capabilities, tasks, aliases, or repository configuration. The ordinary
`--registry` mode remains stricter: it has no default selector and never
mutates any catalog.

A bundled Codex SessionStart hook is non-mutating and non-blocking. It reminds
the agent that catalog startup admitted the current worktree and that the MCP
tools should be preferred for discovery. Installation is a separate explicit
user action that adds this one MCP configuration and hook; it must be
idempotent, own only its marked configuration records, and support removal.

Version 1 deliberately has no persistent daemon, background watcher,
home-directory scan, root glob, automatic catalog synchronization, remote
catalog, general cross-repository query, or MCP indexing mutation. A new Codex
session performs the bounded incremental refresh of its own current worktree.
The separately scoped ADR-0051 may recognize a current worktree only when an
operator has already explicitly registered it as one member of a private
connected workspace; it retains these exclusions and uses the accepted
source-slot publication contract rather than ambient catalog discovery.

## Alternatives considered

### Keep only manually maintained static registries

This preserves the current boundary but makes every first-use repository a
configuration task and defeats the goal of a one-entry Codex experience.

### Let an MCP tool accept a root or database path and index it

This makes untrusted model input local filesystem authority and contradicts
the read-only MCP contract. Catalog admission belongs to explicit process
startup, not a tool call.

### Scan a parent, sibling, or home directory for repositories

This has surprising write scope and leaks host layout. The session's current
directory is the only automatic admission input.

### Copy a shared daemon and watcher architecture

It introduces process coordination, retention, upgrade, cancellation, and
cross-session ownership concerns before evidence that a foreground
incremental startup is insufficient.

## Consequences

### Positive

- A user installs one Codex MCP entry and works normally from any supported
  Git worktree.
- First use creates an isolated private index; subsequent session starts
  refresh it incrementally.
- The default selector removes opaque-ID friction for the current repository
  without granting ambient or caller-selected filesystem authority.

### Negative and risks

- A first index can make MCP startup slower; catalog mode must use the existing
  bounded indexing policy and report a stable failure rather than partially
  starting.
- A catalog snapshot does not see a repository another session admits until a
  new MCP process starts.
- Automatic catalog admission is initially unavailable on platforms where
  private onboarding state fails closed.
- The catalog stores local roots, so it is private control state rather than a
  repository artifact and must never be logged or returned by default.

## Validation

- Unit-test catalog absence, strict schema, bounds, duplicate roots/IDs,
  no-follow admission, atomic replacement, and failure preservation.
- Test current-directory worktree resolution rejects non-worktrees and never
  attempts sibling or home-directory discovery.
- Test first admission, repeat incremental admission, and an index failure,
  asserting catalog mutation only after a complete index report.
- Verify catalog MCP defaults only to the startup-selected repository, requires
  an exact ID for every other entry, and preserves static-registry semantics.
- Test the generated Codex MCP and SessionStart-hook configuration for
  idempotency, removal, path-free output, and non-blocking failure.
- Run installed-binary first-session and repeat-session MCP round trips against
  synthetic Git worktrees.

## Follow-up

- Add an explicit catalog status/remove command so a user can inspect opaque
  IDs and forget a repository without editing control files.
- Measure startup latency before considering a supervised shared watcher or
  daemon.
- Revisit Windows only with a private-state ACL implementation equivalent to
  the current Unix capability boundary.

## Supersession

This supersedes ADR-0044's no-global-root-registry restriction only for this
private, versioned, process-startup catalog. It complements ADR-0049; static
registry mode retains its existing no-default, read-only contract.
