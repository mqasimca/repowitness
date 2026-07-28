# ADR-0019: Compile bounded Phase 0 context from exact source and current memory

- Status: Proposed
- Date: 2026-07-27
- Owners: Project maintainers
- Scope: Application context compilation, local retrieval composition, CLI, and local stdio MCP

## Context

RepoWitness can publish immutable multi-language source generations, retrieve
literal symbol matches, expand an exact occurrence into verified declaration
bytes, and recall the active engineering-memory projection. Phase 0 still lacks
the product loop's final read path: one request that combines those sources
under an explicit budget without hiding stale memory, truncation, unsupported
providers, or a source-generation change.

The longer-term architecture anticipates structural, history, correspondence,
and memory providers. Phase 0 does not yet have complete reference, call-graph,
or history indexes. Treating those providers as if they existed would turn
missing evidence into unsupported confidence. Comparing raw relevance scores
from unrelated providers would also create an unstable contract.

Diagnostics must expose the exact active source and memory state needed to
understand a context result. It must remain read-only, bounded, and safe for
both CLI and local MCP use.

## Decision

Phase 0 adds a versioned `context_build` use case with these invariants:

1. The admitted request contains a bounded literal intent, a positive
   conservative content budget, bounded provider candidate counts, one
   cancellation signal, and one monotonic deadline.
2. The implemented providers are lexical source search and the active memory
   projection. Source candidates are expanded only through the exact
   generation-local selector returned by `code_search`; `symbol_get` verifies
   the current source bytes before a declaration is admitted.
3. Only memory records whose effective state is `current` and whose complete
   record is available may enter the context pack. Every other projected state
   remains visible in coverage and omission counts.
4. Source search, memory recall, exact expansion, and compilation must agree on
   the same repository, source snapshot, and active generation. If activation
   or source content changes during the request, the operation fails closed
   instead of returning a mixed-generation pack.
5. Phase 0 fusion uses versioned reciprocal-rank fusion with `k = 60`.
   Provider-local ranks are preserved. Equal fused scores use the stable order
   memory before source, followed by the provider-local rank and stable item
   identity. Raw provider scores are never compared.
6. Phase 0 labels its budget estimator
   `utf8_bytes_upper_bound_v1`. One admitted UTF-8 content byte consumes one
   budget unit. This is a conservative, deterministic upper bound and is never
   described as an exact model-token count.
7. An item is admitted only when its complete content fits the remaining
   budget. Items are not silently sliced. Budget omissions, provider
   truncation, non-current memory, unavailable memory projection, and
   unsupported structural/history/reference providers are explicit.
8. The returned pack exposes the concrete snapshot and generation, optional
   memory-projection identity and source epoch, versioned fusion and estimator
   identities, component ranks, coverage, omissions, and exact evidence
   selectors or memory identities.
9. CLI and local stdio MCP are thin boundaries over the same local composition
   and application compiler. MCP output retains an independent encoded-output
   ceiling.

Phase 0 also adds a bounded read-only `diagnostics` operation. It reports the
active source snapshot, generation, source epoch, index coverage, active memory
projection and projection coverage when present, implemented provider
capabilities, and explicit Phase 0 limitations. Absence of a memory projection
is a healthy, inspectable state rather than fabricated memory coverage.

This decision does not add vector retrieval, graph traversal, reference
indexing, history search, model-specific tokenizers, background compilation,
remote transport, or write-capable MCP tools.

## Alternatives considered

### Concatenate independent command outputs in the CLI

This is quick, but it duplicates policy across adapters, cannot enforce one
deadline or generation invariant, and has no stable budget or omission
contract.

### Compare SQLite FTS rank directly with memory relevance

Provider scores have unrelated meanings and ranges. Direct comparison would
make ordering depend on adapter implementation details and future schema
changes.

### Require a model-specific tokenizer in Phase 0

This could estimate one model more closely, but adds a production dependency,
versioning surface, model coupling, and avoidable failure modes before context
quality has been measured.

### Fail whenever the memory projection is absent

This preserves a strict two-provider shape but makes source-only repositories
unusable. Reporting memory as unavailable while returning exact source context
is more useful and does not misrepresent coverage.

### Include stale or review-needed memory with a penalty

This increases recall but risks placing invalid engineering guidance directly
into an agent context. Phase 0 instead excludes it and reports why.

## Consequences

### Positive

- The implemented source-change-to-context loop has one deterministic,
  evidence-bearing read path.
- Concurrent activation and changed source fail closed.
- Context size is bounded without claiming model-token precision.
- Missing providers and non-current memory remain observable.
- CLI and MCP behavior share the same ranking and admission rules.

### Negative and risks

- Reciprocal-rank fusion offers little cross-provider deduplication until
  logical correspondence and richer provider overlap exist.
- The byte estimator can under-fill a model's actual token window.
- Exact declaration expansion adds bounded filesystem reads after database
  retrieval.
- Source-only results can differ from later results after a memory projection
  is published; projection identity and omissions therefore remain part of the
  result contract.
- Phase 0 context quality is intentionally limited without references,
  structural expansion, or history retrieval.

## Validation

- Unit tests cover boundary validation, deterministic tie-breaking, exact
  budget admission, non-current-memory exclusion, explicit omissions,
  cancellation, deadline handling, and source/memory context mismatch.
- Local integration tests cover source-only databases, active projections,
  source mutation, concurrent generation activation, and exact declaration
  verification.
- CLI and MCP contract tests cover schema validation, redaction, output bounds,
  read-only annotations, and deterministic structured output.
- Real-repository smoke tests index and build context from mixed Go/Rust,
  Rust-only, and TypeScript/TSX repositories.

## Follow-up

- Completed 2026-07-27: implement the shared application compiler,
  generation-pinned local composition, transactionally pinned diagnostics, CLI
  commands, and read-only local MCP tools with focused unit and contract
  coverage.
- Measure retrieval quality and pack utilization before changing the fusion or
  estimator profiles.
- Add structural, reference, and history providers only with explicit coverage
  and bounded-expansion contracts.
- Revisit a model-aware tokenizer only when a supported integration can pin its
  exact estimator identity and dependency cost.

## Supersession

None.
