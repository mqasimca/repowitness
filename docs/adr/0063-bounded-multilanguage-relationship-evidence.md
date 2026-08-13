# ADR-0063: Add bounded multi-language relationship evidence

- Status: Proposed
- Date: 2026-08-12
- Owners: Project maintainers
- Scope: SCIP overlay import, relationship reads, change-review evidence, and repository inventory

## Context

Go source declarations alone cannot answer caller, callee, interface-dispatch,
or impact questions. The existing SCIP import already carries exact symbol
occurrences and producer-declared implementation relationships, but the old
reader exposed only the latter. The review surface also needs a clear boundary
between exact relationship evidence and lexical/configuration observations.

## Decision

- Keep `scip-go-import` explicit. Its default producer run retains implementation
  and test relationships; skip flags are opt-outs.
- Persist derived reference edges separately from producer-declared SCIP rows.
  A derived row is admitted only when an exact SCIP reference occurrence is
  enclosed by an exact indexed function/method fact and its exact definition
  occurrence. The row is labelled `enclosed_reference`.
- Reuse the existing bounded incoming/outgoing SCIP traversal for Go callers,
  callees, interface implementations, and conservative impact paths. Missing
  producer or source evidence remains `not_produced`, `no_relationships`, or
  truncated; it is never upgraded to certainty.
- Keep the finite `code_graph_query` algebra. Do not add Cypher, SQL, runtime
  tracing, or heuristic dynamic dispatch.
- Treat imports, test markers, operational files, data-flow-like names, and
  service-boundary references as bounded observations unless exact SCIP or
  source-fact evidence proves an edge. Repository topology remains the
  bounded inventory path for non-code operational files; it does not pretend
  to parse deployment semantics.
- Receipts retain canonical byte-preserving paths and add readable display
  paths. The canonical value remains the lookup identity.

## Alternatives rejected

- Reusing raw spelling matches as callers or interface implementations would
  create false edges in overloaded, aliased, and interface-heavy code.
- Replacing the finite operation algebra with a general graph language would
  expose an unbounded storage contract and make resource guarantees harder to
  review.
- Running Go tooling during ordinary indexing would make startup and indexing
  depend on hostile build and module state.

## Consequences

Ordinary indexing, watch mode, and MCP startup stay source-only and
predictable. Go relationship and impact evidence remains bounded, and explicit
`scip-go-import` or onboarding performs the producer step. The derived
projection is intentionally conservative: calls whose enclosing declaration or
SCIP target cannot be proven remain raw observations rather than edges.

## Validation

The SQLite migration, atomic publication, evidence mapping, CLI producer flags,
workspace fixtures, and sibling-repository smoke suite must pass. A synthetic
fixture must cover one enclosed reference, one implementation relationship, an
incoming caller trace, and a no-evidence result.

## Supersession

This supersedes the relationship-coverage limitation in Proposed ADR-0048 and
the Go precision wording in Proposed ADR-0053 where they conflict. It does not
change accepted source identity, generation publication, or dependency-direction
contracts.
