# ADR-0060: Add bounded cross-repository FTI discovery over the local catalog

- Status: Proposed
- Date: 2026-08-12
- Owners: Project maintainers
- Scope: Catalog-mode MCP discovery and multi-repository result aggregation

## Context

Catalog mode currently gives one MCP connection access to independently indexed
repositories, but each normal tool call targets one repository. An agent can
see opaque repository selectors in the tool schemas, yet it cannot ask one
bounded question such as “where is this contract implemented?” across the
catalog. That makes a multi-repository product stack usable only through
manual agent orchestration.

The existing indexes are independent immutable-generation databases with
SQLite FTS5 projections. The
catalog is a bounded startup snapshot, not a shared graph or connected-workspace
view. The current source adapters provide literal search and typed declaration
search, while parser spellings and same-name matches are explicitly not
relationship evidence. A new feature must preserve those boundaries.

MCP tools are model-discoverable and accept structured input; MCP roots can
describe multiple repositories, but roots are access boundaries and do not
establish semantic relationships. Resources are application-driven and are not
a reliable substitute for a model-invoked bounded query.

## Decision

Add one catalog-only read tool named `cross_repository_search`, implemented as
a bounded fan-out over the existing FTI-backed `code_search` path.

### Request

The validated request contains:

- one bounded literal `query`, using the existing `code_search` query profile;
- optional `repository_ids`; omission means every repository in the immutable
  catalog snapshot, while an explicit list selects exact registered IDs;
- `max_results_per_repository`, bounded to the existing search limits;
- a global `max_results` bound; and
- the existing bounded `timeout_ms`.

The tool is unavailable in single-repository mode. It does not accept roots,
database paths, labels, arbitrary filesystem paths, or a caller-supplied
generation.

### Execution

The MCP adapter fans out only to the already admitted repository services,
with the existing operation-concurrency bound and one end-to-end deadline.
Each repository is queried against its own active immutable generation. A
repository failure, cancellation, or timeout does not erase completed results
from other repositories; the response records the categorical omission and
coverage outcome. No synchronous lock or transaction crosses an await.

Results are deterministically ordered by repository identity and then by the
existing repository-local search order. Every result group includes its exact
opaque `repository_id`, snapshot, generation, resolution, coverage, and
repository-relative matches. Host roots, database paths, and source contents
outside the existing bounded match payload are never returned.

The tool description explicitly says that a match in multiple repositories is
FTI candidate discovery, not proof of dependency, ownership, API
compatibility, or a runtime relationship. The response has no confidence score
and never upgrades lexical or same-name evidence into a relationship claim.

### Relationship evidence

Version 1 is intentionally FTI-only. It does not infer cross-repository edges
from sibling paths, package
names, imports, references, calls, same-name declarations, or catalog
membership. Stronger relationship navigation remains a separate future slice:
an explicitly declared connected workspace may compose existing source-slot
contracts, and a reviewed producer such as SCIP may provide attributed
cross-source evidence. Those mechanisms must not be smuggled into lexical
search.

### User experience

The existing one-MCP catalog remains the only required client configuration.
An agent can use the current repository-scoped tools for precise follow-up and
`cross_repository_search` for discovery across all onboarded repositories.
No per-repository MCP entries, daemon, background scan, or index rebuild is
introduced.

## Alternatives considered

### Make the agent call the existing tool once per repository

This requires the agent to know and enumerate opaque IDs, duplicates client
orchestration, and gives no aggregate coverage result. It remains a valid
fallback but is poor default UX.

### Add a shared SQLite database or global graph

This would mix independent generation lifecycles, complicate failure and
retention semantics, and create a new storage abstraction before the bounded
query need is measured.

### Infer product relationships automatically

Names, import spellings, sibling directories, and package metadata are useful
candidates but not trustworthy relationship evidence. False edges would be
worse than an explicit unresolved result.

### Build the full connected-workspace catalog first

The existing proposed ADR-0051 covers atomic multi-source views, but that is a
larger indexing and source-slot feature. Independent FTI fan-out solves the
immediate discovery need without changing publication semantics.

### Add a resource for every repository or search result

MCP resources are application-driven and would add URI, caching, and access
semantics without improving the core query. A bounded model-invoked tool is the
smaller fit for this operation.

## Consequences

### Positive

- One natural agent action can locate a symbol or contract across the product
  catalog.
- Existing per-repository FTI, evidence, and generation guarantees remain
  intact.
- Partial work is visible instead of being presented as complete coverage.
- The implementation reuses the catalog and existing lexical search path.

### Negative and risks

- Searching all 32 entries can cost more than a single-repository query; strict
  repository, result, concurrency, output, and deadline bounds are required.
- Independent generations can differ in time; the response must expose each
  generation rather than pretending the result is one snapshot.
- FTI discovery cannot prove a cross-repository dependency. Agents must
  inspect candidate repositories; semantic relationship evidence remains out
  of scope.
- `tools/list` gains one catalog-only tool, so catalog and single-repository
  tool inventories intentionally differ.

## Validation

- Validate request bounds, duplicate/unknown repository IDs, empty selections,
  cancellation, deadline exhaustion, and output-size limits.
- Route one request through two or three fake services and prove each service
  receives only its own request while one failure produces explicit partial
  coverage.
- Assert deterministic repository ordering and exact per-repository generation
  receipts.
- Assert no host root, database path, or raw catalog contents appear in output,
  errors, or debug formatting.
- Run installed-binary stdio tests against three synthetic repositories and a
  32-entry catalog, measuring single-query latency, all-repository latency,
  truncation, and cancellation.
- Add an agent-facing fixture proving that a query can discover a declaration
  in a non-current repository and then use its repository ID for precise
  follow-up.

## Follow-up

- Completed: the wire DTO, catalog-only tool schema, bounded fan-out, aggregate
  coverage receipt, and installed-binary stdio path are implemented.
- Completed: catalog documentation and MCP contract coverage describe the
  FTI-only behavior.
- Measure real multi-repository prompts before adding anything beyond FTI.
- Revisit ADR-0051 only if independent indexes cannot provide sufficient
  evidence-backed navigation.

## Supersession

None. This complements ADR-0049, ADR-0050, ADR-0051, and ADR-0032 without
changing their authority or publication contracts.

## Research sources

- [MCP tools specification](https://modelcontextprotocol.io/specification/2025-03-26/server/tools)
- [MCP roots specification](https://modelcontextprotocol.io/specification/2025-03-26/client/roots)
- [MCP resources specification](https://modelcontextprotocol.io/specification/2025-03-26/server/resources)
