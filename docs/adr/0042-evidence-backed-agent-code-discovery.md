# ADR-0042: Evidence-backed agent code discovery and syntax-site navigation

- Status: Proposed
- Date: 2026-08-01
- Owners: Project maintainers
- Scope: supported-language analysis, immutable generation publication, SQLite
  reads, CLI, local stdio MCP, and agent discovery contracts

## Context

RepoWitness has strong temporal and evidence guarantees, but an agent's first
questions are navigational: find a typed declaration, locate a relevant path,
inspect an exact declaration, trace supported relationships, and form a compact
overview of the active codebase. The existing `code_search` and
`architecture_map` operations cover literal declaration discovery and a
multi-language file inventory. The accepted Rust graph adds exact-name search,
syntax sites, trace, and impact, but only for Rust. The opt-in aliases named
`search_graph`, `trace_path`, and `get_architecture` currently preserve only
name compatibility with an incumbent surface; they must not imply broader
request, response, or behavior compatibility.

The five supported language adapters already persist deterministic declaration
facts with exact spans, language, artifact, producer, source digest, snapshot,
and active-generation receipts. Reusing those facts can provide all-language
typed discovery without reindexing or weakening the source-generation
invariants. In contrast, a syntax occurrence is not proof of a resolved
relationship: package/module resolution, macros, dynamic dispatch, re-exports,
generated code, and cross-language behavior require language-specific evidence
or the separately scoped SCIP provider defined by ADR-0035.

## Decision

Add a versioned, bounded agent code-discovery family. It consists of typed
declaration search, source-only architecture overview, raw syntax-site
navigation, and a closed query-operation algebra. Native names remain primary;
any incumbent aliases map only to the same validated application use cases.

### Typed declaration search

`symbol_search` reads the existing immutable active-generation declaration
facts for Rust, Go, TypeScript, TSX, and Python. It accepts only bounded exact
or prefix text plus allow-listed language, symbol-kind, and canonical-path
filters. It returns deterministic declaration candidates with exact selector,
path, language, kind, qualified name, spans, source/artifact/producer receipts,
snapshot, generation, coverage, and explicit result/output truncation.

It is a distinct use case from `code_search`: literal full-text retrieval stays
available for recall and context, while `symbol_search` is the agent's typed
discovery operation. Neither operation treats a partial result as proof of
absence.

### Source-only architecture overview

`architecture_overview` derives a deterministic, bounded overview from one
active generation: exact indexed source roots, language and declaration-kind
totals, declared entry-point candidates, and per-file declaration counts. A
candidate is explicitly syntax-derived; it is not a runtime entry point,
package boundary, ownership relation, hotspot, or dependency proof.

Documentation, configuration, workflow, and general tracked-path topology are
outside this decision. They require their own content, privacy, source-state,
and artifact-reuse boundary under ADR-0043.

### Multi-language raw syntax sites

Add a new language-neutral, artifact-local syntax-site projection beside the
accepted Rust graph, not inside it. Each supported language has a reviewed,
versioned Tree-sitter extractor for the allowed raw site kinds: `import`,
`reference`, `call`, and `test_marker` where that grammar can identify the
construct. Every site records language, kind, direct-syntax or bounded-heuristic
evidence, exact occurrence and target spans, bounded target text, optional
enclosing declaration selector, parser/site coverage, and artifact identity.

Raw sites and resolved graph edges stay separate. A raw import specifier or
identifier does not establish a target declaration. The initial all-language
navigation operation returns outbound raw sites for one exact declaration,
including categorical unresolved/ambiguous status and candidate receipts when
they are safely available. Existing Rust trace and impact behavior remains
unchanged. Resolved package-aware, macro-aware, dynamic-dispatch, or
cross-language edges require a separate language-specific resolution profile or
SCIP evidence; same-name matching must never silently create an edge.

### Closed query-operation algebra

Do not add raw SQLite, Cypher, or arbitrary `query_graph` execution. Instead,
the versioned `code_graph_query` envelope selects one finite operation:
`symbols`, `outbound_sites`, `architecture`, `files`, `test_markers`, or
`relevant_paths`.
Each operation has a strict schema, independently bounded planner, result
count, output bytes, deadline, cancellation path, deterministic order, pinned
generation, evidence, coverage, and truncation. Unknown operations and fields
are rejected before storage access.

### Publication and retention

Syntax-site artifacts and topology receipts have their own
semantics-affecting profile/grammar/configuration identity, immutable staging,
complete-generation validation, atomic activation, recovery, retention,
backup, and reader validation. They may reuse an unchanged site artifact only
when every corresponding input matches. A failed or cancelled preparation
leaves the current active source generation and all active discovery receipts
readable.

## Alternatives considered

### Rebrand the current Rust graph as a universal graph

Rejected. It would make a Rust-specific syntax/resolution profile appear to
cover other languages and weaken the accepted ADR-0027 contract.

### Match declaration names to create all-language call edges

Rejected. Same-name matching is not resolution. It creates false inbound
relationships and is especially unsafe across imports, methods, re-exports,
and multi-package repositories.

### Expose open Cypher, SQL, or `query_graph`

Rejected. An open query language adds an authorization, resource-planning,
generation-pinning, and persistence-compatibility boundary. It would bypass
the bounded typed application use cases required by ADR-0008 and ADR-0030.

### Read arbitrary documentation and configuration files during code indexing

Rejected. It would broaden content handling, source snapshot semantics, and
secret exposure without proof that the extra content supports an agent result.
Path-only topology is sufficient for initial orientation.

## Consequences

### Positive

- Codex can locate exact multi-language declarations before source expansion.
- Every returned code-discovery result remains attributable to one immutable
  generation with evidence, coverage, and explicit limits.
- Raw syntax evidence offers useful navigation without falsely claiming a
  package- or compiler-resolved relationship graph.
- The closed query algebra provides a practical structural-query workflow
  without opening a general query-execution surface.

### Negative and risks

- New site extractors, persistence, and schemas add migration and maintenance
  cost for every supported language.
- Results can remain unresolved or truncated; clients must honor coverage and
  never interpret absence as proof.
- The broader MCP surface requires versioned compatibility, privacy, output,
  cancellation, and backpressure tests.

## Validation

- Pure analyzer tests for every site kind, syntax error, limit, cancellation,
  exact span, deterministic ordering, and unsupported construct in all five
  languages.
- Migration, staging, activation, recovery, retention, backup, corruption, and
  clean-versus-incremental tests for topology and site artifacts.
- Application and SQLite-reader tests for typed filters, source/artifact
  receipts, canonical ordering, candidate cardinality, no-result coverage,
  output/result bounds, stale-generation isolation, cancellation, and deadline.
- Installed CLI/MCP mixed-language contracts for discovery, exact source,
  overview, direct and finite-envelope raw-target and lexical-path navigation,
  outbound sites, closed query operations, tool schemas, stdout purity, and
  protocol limits.
- Regression fixtures proving no same-name edge is created without the
  applicable resolution profile, and that Rust graph semantics remain unchanged.
- `scripts/test-sibling-repositories` builds the CLI, indexes each direct
  sibling Git worktree, verifies architecture-map, architecture-overview,
  repository-scoped test-marker, direct and finite-envelope exact raw-target
  and lexical-path navigation, typed-symbol-discovery, outbound-site, and
  no-overlay SCIP-resolution smoke operations, and emits aggregate-only results
  without repository identifiers, paths, source, or per-repository measurements.

## Follow-up

1. `symbol_search` and `architecture_overview` use existing active-generation
   facts and path receipts; retain their no-inference boundary while extending
   the discovery family.
2. Add the versioned all-language raw syntax-site projection and outbound-site
   navigation.
3. Add the closed `code_graph_query` algebra over those use cases.
4. Add resolved relationships only through language-specific research/ADRs or
   validated SCIP receipts; do not broaden syntax-site claims implicitly.

## Supersession

None.
