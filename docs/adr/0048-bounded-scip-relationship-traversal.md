# ADR-0048: Traverse validated SCIP relationships through a bounded precision profile

- Status: Proposed
- Date: 2026-08-02
- Owners: Project maintainers
- Scope: existing immutable SCIP overlay reads, SQLite query indexes, CLI,
  local stdio MCP, and agent relationship-navigation contracts

## Context

RepoWitness can resolve one exact indexed declaration receipt to an opaque
SCIP symbol and read that symbol's directly declared incoming and outgoing
overlay relationships. An agent must currently issue one read per hop and
construct its own traversal. That loses one coherent deadline, makes global
bounds difficult to preserve, and provides no one-result account of the
unexpanded frontier.

The accepted SCIP overlay is the only current multi-language producer of
validated cross-file relationship evidence. The all-language raw syntax-site
projection intentionally records observations without resolving their target
spellings; equal source text must not become a relationship. The Rust syntax
graph remains a separate, Rust-only syntax-derived evidence profile.

## Decision

Add `scip_relationship_trace`, a versioned read over one active or explicitly
pinned immutable SCIP overlay. It accepts one exact opaque SCIP symbol,
caller-provided package scope, explicit `outgoing` or `incoming` traversal
direction, and a bounded maximum depth from one through four. It starts at
the supplied symbol and performs deterministic breadth-first traversal only
over validated persisted SCIP relationship rows in the selected source slot.

Every returned edge preserves its immutable document and relationship ordinals,
the source document path and content digest, the exact producer source and
target symbols, the declared relationship kind bits, and its depth. Expansion
is breadth-first; siblings are ordered by those persisted ordinals, not by an
unbounded path sort. The result carries the selected connected workspace,
immutable view, source slot, overlay receipt, package-scope digest, requested
direction and depth, conservative JSON-encoded edge accounting, and
categorical coverage. `not_produced`, `no_relationships`, a known unexpanded
frontier of symbols that could not be completely expanded, and independent
depth, edge, node, or output-byte truncation remain
distinct. The known frontier is a lower bound when an edge or output ceiling
stops relationship-row discovery. A bounded result never claims a complete
transitive closure when a frontier or resource limit remains.

The profile has fixed independent ceilings for returned edges, distinct
visited symbols, JSON-encoded edge output, deadline, and four traversal hops.
SQLite progress callbacks enforce cancellation and deadline checks even while
a package-scoped query is filtering persisted rows. New immutable indexes
support both source- and target-symbol expansion in persisted ordinal order.
The operation reads no source files,
executes no producer or package-manager command, and does not mutate an
overlay, generation, or active pointer.

This is a dedicated SCIP precision tool rather than an addition to the
repository-only `code_graph_query` algebra. That algebra does not select a
connected-workspace source slot or SCIP overlay, and extending it without
those pins would weaken ADR-0035 and ADR-0037. It remains a closed operation
set and does not become Cypher, SQL, or a general graph-query surface.

## Alternatives considered

### Ask clients to repeatedly call `scip_evidence`

Rejected. It can expose direct evidence, but cannot enforce a global
deadline, stable traversal order, shared edge/node/output bounds, or one
truthful unexpanded-frontier receipt.

### Traverse raw syntax targets or equal declaration names

Rejected. Raw parser observations and equal text are not package resolution.
Doing so would create false cross-file and cross-language relationships in
contradiction of ADR-0035 and ADR-0042.

### Merge the traversal into the Rust syntax graph

Rejected. It would blur syntax-derived Rust evidence with producer-declared
precision evidence and incorrectly imply equivalent coverage for other
languages.

### Expose arbitrary graph queries

Rejected. General query execution bypasses fixed resource planning,
source-slot/view selection, provider attribution, and output coverage. The
finite traversal has a small auditable contract instead.

## Consequences

### Positive

- Agents can follow provider-declared cross-file relationships with one
  coherent, pinned, bounded receipt.
- Existing SCIP imports gain high-confidence relationship navigation without
  changing source indexing or syntax evidence.
- Incoming and outgoing traversal use explicit indexes and deterministic
  breadth-first ordering.

### Negative and risks

- SCIP overlays are optional, producer-specific, and may be partial; an edge
  is not a claim of repository-wide or runtime completeness.
- Opaque producer symbols and source locations can enlarge outputs quickly,
  requiring conservative limits and explicit truncation.
- A new MCP/CLI schema and SQLite migration are a permanent compatibility and
  maintenance obligation.

## Validation

- Application tests for selection pinning, direction/depth admission,
  cancellation, deadlines, and hostile adapter context mismatches.
- Synthetic overlay fixtures for deterministic multi-hop inbound/outbound
  traversal, cycles, branches, package scope, no-overlay/no-relationship
  categories, and independent depth/node/edge/output bounds.
- SQLite migration tests proving both relationship expansion indexes exist and
  version-ten databases upgrade without changing persisted overlay facts.
- CLI and MCP contracts covering strict schemas, tool inventory, all
  categorical outcomes, output ceilings, and empty child-process stderr.
- `scripts/test-sibling-repositories` smoke coverage for every sibling
  worktree's explicit `not_produced` outcome only, with no repository paths,
  symbols, source, or per-repository details emitted.

## Follow-up

- Evaluate language-specific package-aware resolution profiles separately.
- Consider cross-language producer evidence only when a validated producer
  receipt supplies it; do not infer it from source syntax.

## Supersession

None. This composes with accepted ADR-0035 and ADR-0037 and keeps the raw
syntax limitations in proposed ADR-0042 intact.
