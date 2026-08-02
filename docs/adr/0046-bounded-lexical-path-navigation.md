# ADR-0046: Bounded lexical path navigation from immutable declaration evidence

- Status: Proposed
- Date: 2026-08-02
- Owners: Project maintainers
- Scope: application discovery projection, local adapter, CLI, local stdio MCP,
  tests, and sibling-repository validation

## Context

RepoWitness can inventory the indexed source, search supported-language
declarations, retrieve exact declarations, and return narrowly scoped syntax or
SCIP evidence. That is trustworthy but unnecessarily laborious for a first
agent step: it must manually group a lexical search's declaration receipts to
find the source paths with the most direct matches.

The project must not turn a lexical co-occurrence into a dependency, call path,
semantic similarity result, ownership inference, or general graph query. Such
claims require a separately scoped resolution or ranking profile. A second
storage query would also risk losing the single snapshot/generation receipt
already provided by `code_search`.

## Decision

Add `locate_relevant_paths`, a bounded presentation of exactly one completed
`code_search` receipt. It receives the same bounded literal symbol terms and
uses the existing active-generation search path once. It groups only the
returned syntax declaration evidence by canonical repository path, rejects
conflicting content identities, and orders paths by descending returned-match
count followed by canonical path.

The result carries the unchanged lexical search receipt: snapshot, generation,
query digest, evidence records, producer and artifact identities, resolution,
coverage, and declaration-candidate truncation. It separately returns bounded
path summaries with their exact content digest, matching-declaration count, and
first fact ordinal, plus an exact pre-limit count of paths represented by the
returned matches and categorical truncation of that path surface. Neither field
counts paths that might occur only in omitted candidates. A limited path list
therefore never appears to account for every returned declaration match.
The operation makes no semantic, relationship, runtime, ownership, or absence
claim. Its documentation and wire limitation state that “relevant” means only
direct matches within the returned bounded lexical candidate surface.

No schema, indexing profile, raw source read, vector store, general graph query,
or new language adapter is introduced. The CLI and MCP compose the same local
application projection; MCP remains read-only and path-free on input.

## Alternatives considered

### Add a semantic/vector task-to-code ranker

Rejected for this slice. It needs an evaluated corpus, model/version identity,
privacy boundary, ranking calibration, and an explicit confidence contract. It
would not be honest to present its output as direct source evidence.

### Add all-language call/import graph resolution

Rejected. Raw syntax sites are not resolved edges. Package resolution,
re-exports, macros, dynamic dispatch, and language-specific build semantics
require independent per-language evidence profiles or the scoped SCIP overlay.

### Make clients group `code_search` responses themselves

Rejected. Repeating this logic encourages inconsistent ranking, accidental
loss of coverage/truncation information, and inconvenient agent navigation.

## Consequences

### Positive

- Agents get a one-call, generation-pinned path starting point for literal
  discovery without weakening evidence attribution.
- Existing immutable lexical evidence, storage, and language adapters are
  reused without a migration or duplicated query.
- Canonical grouping and ordering are testable and identical for CLI and MCP.

### Negative and risks

- Natural-language intent, comments, configuration, documentation, and
  unsupported-language source remain outside the operation.
- A path with more returned declarations is not necessarily more important;
  candidate truncation can affect the presentation and remains explicit.
- The MCP surface gains another schema and compatibility obligation.

## Validation

- Application fixtures for aggregation, ordering, path limit, no-match, and
  conflicting-content rejection while preserving the original coverage receipt.
- Local and installed MCP contracts for valid input, invalid bounds, output
  schema, deterministic ordering, and no relationship claim.
- `scripts/test-sibling-repositories` invokes the bounded operation for every
  direct sibling worktree and reports aggregate-only outcomes.
- Full workspace format, check, Clippy, test, documentation, dependency, and
  benchmark checks.

## Follow-up

1. Evaluate a separately versioned semantic ranking profile only with a public
   benchmark, privacy policy, ranking evidence, and maintainer decision.
2. Continue resolved cross-file relationships only through language-specific
   evidence or the scoped SCIP profile.

## Supersession

None.
