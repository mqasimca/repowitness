# ADR-0047: Bounded raw syntax target search

- Status: Proposed
- Date: 2026-08-02
- Owners: Project maintainers
- Scope: existing all-language raw syntax-site projection, immutable SQLite
  reads, CLI, local stdio MCP, and agent discovery contracts

## Context

`outbound_sites` lets an agent inspect parser-attributed import, reference,
call, and test-marker observations physically contained in one exact
declaration. It cannot answer the complementary navigational question: where
does an exact raw target spelling occur in the current supported-language
source slice?

The stored raw-site projection already records the immutable source path,
content digest, language, artifact identity, exact spans, bounded raw target,
and parser evidence for every observation. Matching target text is useful
navigation, but it is not name resolution: the same spelling can designate
different declarations, modules, fields, or runtime values.

## Decision

Add `syntax_site_search`, a versioned, bounded search over the immutable
all-language raw syntax-site projection. It accepts one exact UTF-8 raw target
spelling and returns only observations whose stored target text has exactly the
same bytes. It returns parser-attributed import, reference, call, and
test-marker observations in canonical path then source-span order.

The operation carries the active snapshot and generation, source-index
coverage, raw-site-projection availability, exact pre-limit match count,
conservative output-byte accounting, per-observation path/content/artifact
receipts, language, source spans, kind, and extraction evidence. A SHA-256
digest identifies the query without adding query text to diagnostic output.
An empty response never establishes that a declaration has no callers or
references: unsupported constructs, parser coverage, unavailable projections,
and bounded results remain explicit.

The SQLite schema gains only an immutable `raw_target` index for this exact
predicate. No raw source read, target-to-declaration association, package
resolution, call graph edge, ranking model, vector index, or general query
language is introduced. `code_graph_query` may select this one finite
operation, but it remains a closed algebra rather than Cypher or SQL.

## Alternatives considered

### Reuse the Rust graph's inbound trace for all languages

Rejected. The Rust graph has its own bounded syntax-resolution contract. Its
results cannot truthfully represent Go, TypeScript, TSX, or Python without a
language-specific resolution profile or validated SCIP evidence.

### Match raw target text to declarations automatically

Rejected. Text equality is not semantic identity. Imports, re-exports,
methods, fields, macros, aliases, dynamic dispatch, and generated code make
same-name associations unsafe.

### Add a full-text or vector index over raw source

Rejected for this slice. It widens content handling and ranking behavior. The
existing immutable parser projection is enough to answer the exact-observation
question with auditable coverage.

## Consequences

### Positive

- Agents can navigate exact all-language raw observations before escalating to
  a language-specific graph or SCIP evidence.
- Lookup uses an indexed predicate without reindexing source or mutating an
  active generation.
- The contract distinguishes a useful lexical observation search from a
  resolved inbound-reference claim.

### Negative and risks

- Identical raw spellings can produce many unrelated observations; returned
  evidence must not be presented as a relationship.
- Exact matching deliberately does not bridge spelling variants, aliases,
  casing, or natural-language intent.
- The additional MCP schema and compatibility surface require permanent
  cancellation, bounds, privacy, and protocol coverage.

## Validation

- Application tests for target admission, redacted debug output, immutable
  context validation, exact matching, ordering, bounds, output accounting,
  projection availability, cancellation, and no-resolution limitation.
- SQLite migration, indexed query, stale-context, cancellation, and
  source-order tests using synthetic mixed-language fixtures.
- Installed CLI and MCP contracts, including the finite `code_graph_query`
  operation schema and rejection of cross-operation fields.
- `scripts/test-sibling-repositories` invokes the direct and finite-envelope
  no-match operation for every direct sibling Git worktree and emits only
  aggregate results.

## Supersession

None.
