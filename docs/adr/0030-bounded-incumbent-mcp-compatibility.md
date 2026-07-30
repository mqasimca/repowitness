# ADR-0030: Offer bounded incumbent-compatible MCP aliases

- Status: Proposed
- Date: 2026-07-28
- Owners: Project maintainers
- Scope: Local stdio MCP tool names, request/response schemas, capability
  discovery, compatibility claims, and clean-room contract testing

## Context

Coding agents already learn tool names and call patterns from established local
code-graph servers. Requiring a completely different vocabulary adds discovery
cost even when RepoWitness provides an equivalent bounded operation.

The public
[`codebase-memory-mcp` v0.9.0 release](https://github.com/DeusData/codebase-memory-mcp/releases/tag/v0.9.0),
observed on 2026-07-29 at exact tag revision
`b637e3330c96cfe452da623db068c241aaa3ec01`, exposes familiar code-discovery
names. The release changes independently, so a shared tool name does not by
itself prove compatible input, response, behavior, coverage, or failure
semantics.

RepoWitness must preserve proof-carrying answers, immutable generation
selection, explicit coverage, bounded traversal, default read-only MCP, and the
clean-room policy in [ADR-0009](0009-mit-license-and-clean-room-contributions.md).
It must not hide unsupported work merely to resemble another server.

## Decision

Add an opt-in versioned MCP compatibility profile. The profile exposes a small
set of independently implemented aliases over the same application use cases
as native RepoWitness tools.

Compatibility is measured separately at four levels:

1. **name compatible**: an alias with the same public tool name exists;
2. **request compatible**: the documented shared request subset validates and
   maps without reinterpretation;
3. **response compatible**: the documented shared response subset has stable
   field names and types;
4. **behavior compatible**: equivalent fixtures produce equivalent ordered
   logical results, pagination, limits, and error categories.

Capability discovery reports each level for every alias. Documentation and
responses must not use “drop-in compatible” unless the full supported matrix is
green against a pinned public incumbent release and all known differences are
listed.

### Initial candidate alias set

The first profile may expose only these aliases:

| Alias | RepoWitness use case | Required behavior |
|---|---|---|
| `search_code` | `code_search` | Bounded literal search over one pinned immutable workspace view, with deterministic pagination and explicit skipped/truncated coverage |
| `get_code_snippet` | `symbol_get` | Exact digest-verified source for a fully selected occurrence; no fuzzy retargeting |
| `search_graph` | typed symbol/reference query | Allow-listed labels and filters only; definitions and graph sites retain evidence and resolution status |
| `trace_path` | bounded graph traversal | Direction and depth from the shared subset; exact limits, cycle handling, resolution status, and truncation |
| `get_graph_schema` | graph capability receipt | Versioned supported node/site/edge kinds, limits, producer versions, generation, and unavailable capabilities |
| `get_architecture` | bounded architecture summary | Deterministic packages, entry points, test relations, and hotspots only when supported by indexed evidence |
| `detect_changes` | revision/worktree impact | Exact changed paths plus bounded graph reachability; unresolved or uncovered paths remain explicit |
| `list_projects` | connected-workspace view listing | Opaque project/source-slot identities, active snapshot/generation, and aggregate coverage without host paths |
| `index_status` | diagnostics | Current immutable view, freshness/source epoch, coverage, and bounded task state |

An alias is not listed until its application use case and contract fixtures are
implemented. A listed alias may return a stable `unsupported_capability`
outcome for an optional parameter, but it may not silently ignore that
parameter or return an empty success.

### Deliberate exclusions

The initial profile does not expose:

- `query_graph`, because accepting an open query language would bypass typed
  traversal limits and create a separate parser, planner, authorization, and
  denial-of-service boundary;
- `delete_project`, because destructive project removal needs an independent
  explicit maintenance contract;
- `manage_adr`, because architecture decisions remain normal repository files
  and RepoWitness memory mutation uses its own reviewed trust workflow;
- `ingest_traces`, because runtime telemetry is demand-gated beyond Phase 1; or
- `index_repository` over default MCP, because indexing mutates local state and
  the accepted transport is read-only unless startup policy explicitly grants
  a named write capability.

These exclusions are returned by capability discovery. They are not represented
by no-op tools.

### Profile negotiation

Native RepoWitness tools remain the default. Startup configuration selects one
of:

```text
canonical             -> native-v1
minimal               -> minimal-native-v1
incumbent-compatible  -> native-v1-plus-incumbent-subset-v1
```

These are the exact version-1 configuration spellings defined by
[ADR-0025](0025-versioned-local-configuration-and-policy.md). Version 1 does
not offer an alias-only surface: an agent that selects familiar aliases still
retains the canonical evidence and context tools needed to interpret their
results. `minimal` remains unavailable until its smaller native surface has an
independent contract. `incumbent-compatible` remains unavailable until every
advertised alias in its selected subset is implemented and authorized. A
repository layer can restrict either profile but cannot make an unavailable
profile available.

The resolved profile and its concrete surface identifier are part of the
configuration digest. MCP initialization and `tools/list` expose both values.
Tool lists remain stable for a process lifetime. A configuration change
requires a new process and cannot alter an established session.

Aliases and native tools map into validated application requests before any
filesystem or database access. They share authorization, request context,
deadlines, cancellation, generation pinning, policy, and resource budgets.
Adapters may rename fields but may not remove evidence, coverage, producer,
snapshot, generation, resolution, limitations, or truncation from a material
result.

### Request and response rules

- Reject unknown request fields, wrong types, invalid enums, embedded NUL,
  over-limit text, invalid pagination, depth outside `1..=5`, and ambiguous
  project or symbol selectors.
- Use stable opaque continuation tokens bound to the profile, normalized
  request, workspace view, generation, policy digest, and result ordering.
- Reject a token if any bound input changes. Never restart silently from a
  newer generation.
- Sort by the documented logical key and use exact byte identities as final
  tie-breakers.
- Report pre-limit match counts only when they were fully computed within
  bounds; otherwise report the count as unknown with explicit truncation.
- Return generic protocol errors. Diagnostic details may contain bounded
  categories and counts, never source text, query text, raw host paths,
  credentials, environment values, or internal stack traces.
- Treat an empty result as unresolved unless complete coverage for the stated
  scope proves absence.

RepoWitness extensions live in a namespaced receipt object. Fields required to
preserve evidence and coverage are not optional merely because an incumbent
schema omits them.

### Clean-room process

Compatibility work may use public documentation, observed public protocol
behavior, and independently authored fixtures. It must not copy or port
upstream source, tests, fixtures, generated code, schemas, prompts, or
substantial documentation.

Every compatibility record includes:

- observation date, public source URL, and pinned release or commit;
- exact tool and compatibility levels tested;
- independently written vectors for every claimed request, response, or
  behavior level;
- known semantic and limit differences; and
- provenance and license review.

Incumbent behavior is an oracle only where the public contract is unambiguous.
Undocumented behavior does not become a RepoWitness contract accidentally.

## Alternatives considered

### Use only native names

This keeps the surface smallest but forces agents to relearn common discovery
operations and makes controlled incumbent comparison less representative.

### Copy the incumbent schemas and implement every tool

This creates provenance risk, inherits unrelated product choices, and would
force destructive mutation, open query execution, telemetry, and broad
language claims before RepoWitness can support them truthfully.

### Expose aliases that return native payloads

Matching only names is easy but misleading. Agents commonly depend on request
fields, pagination, ordering, and response structure. Capability discovery must
state the actual compatibility level.

### Make the compatibility profile the default

This would freeze an externally controlled vocabulary before RepoWitness's
evidence-backed memory and context model stabilizes. Native names remain the
primary contract.

### Translate openCypher into typed queries

Partial translation risks silently changing query meaning. Explicit typed
search and traversal are easier to bound, authorize, explain, and test.

## Consequences

### Positive

- Agents can reuse familiar high-value discovery calls.
- Compatibility claims become versioned, narrow, and falsifiable.
- Native and alias tools share one trusted application implementation.
- Evidence and coverage remain visible instead of being lost in translation.
- Independent fixtures preserve the clean-room boundary.

### Negative and risks

- Each alias adds schema, pagination, and differential-test maintenance.
- The incumbent can change independently and invalidate a compatibility level.
- RepoWitness extensions mean full wire equivalence may remain intentionally
  unavailable.
- An opt-in profile adds startup and documentation complexity.

## Validation

- Golden JSON Schema and JSON-RPC vectors for every listed alias, including
  exact `tools/list` annotations and stable ordering.
- Reject unknown, duplicate, missing, wrong-type, over-limit, invalid-enum,
  invalid-depth, invalid-pagination, invalid-token, stale-generation, and
  unauthorized requests.
- Differential clean-room fixtures against the pinned public incumbent version
  for every claimed request, response, and behavior level.
- Native-versus-alias property tests prove identical application requests and
  logical result receipts after boundary mapping.
- Empty, exact-limit, one-over-limit, ambiguous, cyclic, partially indexed,
  cancelled, timed-out, stale-view, and corrupt-store cases.
- Pagination fixtures prove no duplicates or omissions and reject tokens after
  generation, profile, policy, or query changes.
- MCP initialization, concurrent request, backpressure, output-bound,
  stdout-purity, clean-shutdown, and client compatibility fixtures.
- Privacy fixtures inject source, query, path, environment, credential, and
  control-character canaries and prove they do not appear in errors, logs, or
  capability receipts.
- Provenance review proves all production code and fixtures are independently
  authored.

The implemented subset currently claims only name compatibility. Its
acceptance evidence is therefore deliberately narrower than the validation
needed to claim request, response, or behavior compatibility:

- a pinned, independently authored public `tools/list` observation proves the
  seven shared names and records the incompatible minimum request shapes;
- an exact local name-only `tools/list` contract golden freezes the seven-alias
  order, descriptions, strict input-field inventory, schema presence, and
  read-only annotations without copying an incumbent schema;
- table-driven boundary tests prove invalid inputs for all seven aliases fail
  before service access and do not disclose canaries; and
- all seven successful local responses preserve the canonical payload and a
  canary-free receipt that says `name=compatible`, `request=incompatible`,
  `response=not_assessed`, and `behavior=not_assessed`.

Differential request, response, and behavior fixtures become mandatory before
any of those three levels can be upgraded from the current conservative
assessment.

## Implementation status

Proposed. The opt-in version-1 `incumbent-compatible` profile is implemented
as `native-v1-plus-incumbent-subset-v1`. It advertises seven independently
authored bounded read-only aliases: `search_code`, `get_code_snippet`,
`search_graph`, `trace_path`, `get_graph_schema`, `get_architecture`, and
`index_status`. Every alias claims name compatibility only. Request shapes are
explicitly incompatible with the pinned release, and response and behavior
compatibility are not assessed.

The independently authored
[public observation fixture](../../crates/repowitness-mcp/src/wire/compatibility/fixtures/codebase-memory-mcp-v0.9.0.json)
and
[local `tools/list` golden](../../crates/repowitness-mcp/src/server/tests/fixtures/incumbent-subset-v1-tools-list.json)
bound those claims. The default remains the canonical native surface. The
other initial candidates remain excluded until their own use case, strict
boundary, and contract fixtures are implemented. This ADR remains proposed
until maintainers review the name-only claims and release evidence.

## Supersession

None.
