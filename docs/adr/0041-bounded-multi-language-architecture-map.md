# ADR-0041: Bounded multi-language architecture map

- Status: Proposed
- Date: 2026-08-01
- Owners: Project maintainers
- Scope: application retrieval, local SQLite reader, CLI composition, and native local stdio MCP

## Context

RepoWitness indexes exact source facts for Rust, Go, TypeScript, TSX, and Python in one immutable active generation. Its existing native graph is deliberately Rust-only and syntax-derived: it can provide attributed Rust relationships, but it cannot truthfully describe resolved imports, calls, ownership, or cross-language edges for the other indexed languages.

An agent also needs a compact first-step map of the indexed codebase in order to locate relevant paths before it performs focused search or retrieves an exact declaration. Returning storage rows directly would weaken the generation, evidence, ordering, resource, and cancellation guarantees of the application boundary.

## Decision

Add `architecture_map`, version 1, as a read-only bounded source-file inventory over one active repository generation.

- It returns canonical-path-ordered exact file receipts for every supported indexed language, bounded by independent file-count and encoded-output limits.
- Each returned receipt carries the path, language, source-content digest, analysis-artifact digest, producer-manifest digest, and persisted declaration count. It never returns source bytes.
- It returns complete file and declaration totals by language, the exact snapshot and generation, indexed-source coverage, explicit returned-versus-total truncation, and the conservative output-byte count.
- The application validates language/path agreement, strict canonical byte-path order, all totals, language-summary order, output bounds, cancellation, and deadline before exposing a result.
- CLI composition and native MCP call the same application use case through a narrow SQLite read port. The MCP tool is named `architecture_map` and uses a versioned schema.
- The capability explicitly does **not** infer imports, calls, ownership, package resolution, macro expansion, dynamic dispatch, or cross-language relationships. Those claims remain unavailable; `graph_*` remains the separate Rust-only relationship capability.
- The first version reads one active repository generation. Connected-workspace aggregation, pagination, and generic graph queries are out of scope.

## Alternatives considered

### Build one resolved multi-language relationship graph

This could offer richer navigation, but reliable resolution requires language-specific package/module semantics and explicit handling of macros, dynamic dispatch, generated code, and inter-language boundaries. Shipping partial edges would risk claiming certainty unsupported by evidence.

### Expose a generic SQLite or graph query endpoint

This would bypass bounded resource policy, generation receipts, and typed evidence contracts, while exposing unstable persistence details as public behavior.

### Rely on `code_search` alone

Literal search remains the focused discovery tool, but it cannot provide an unprompted, deterministic inventory of source paths and language coverage that constrains a repository-level investigation.

### Build a second map index

The active generation already persists the required source and artifact receipts. A separate index would add duplication and activation-consistency risk without supplying stronger evidence.

## Consequences

### Positive

- Agents can discover the indexed source surface and relevant canonical paths before issuing focused retrieval calls.
- Every map response is pinned to one active immutable generation and gives exact receipts suitable for follow-up evidence retrieval.
- The capability covers all already supported languages without implying relationship support that does not exist.

### Negative and risks

- A file inventory is not an architecture relationship graph; clients must not use adjacency or absence of an edge as a design conclusion.
- Large repositories can produce truncated receipts. Complete language totals and explicit truncation make that limitation visible, but the first version has no cursor pagination.
- The public MCP schema adds a maintained compatibility surface even while the broader stable API remains deferred.

## Validation

- Application tests validate limits, receipt order, language/path agreement, total accounting, cancellation, deadline, and invalid port output.
- SQLite reader tests and installed mixed-language fixtures must prove deterministic active-generation results, all five languages, digest/producer receipt integrity, output truncation, and stale-generation isolation.
- MCP schema, tool-list, cancellation, output-budget, and stdio round-trip tests must cover the new native tool.
- Documentation must state that the tool inventories files only and does not infer relationships.

## Follow-up

- Evaluate generation-bound cursor pagination if the bounded 1,000-file response cannot cover expected repositories.
- Treat resolved multi-language relationships as a separate research/ADR decision with language-specific evidence requirements.

## Supersession

None.
