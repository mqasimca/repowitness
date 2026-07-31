# ADR-0036: Compile Phase 2 context through named evidence-ranking profiles

- Status: Accepted
- Date: 2026-07-31
- Owners: Project maintainers
- Scope: Context-provider contracts, ranking, token allocation, evidence,
  application use cases, local persistence, CLI, MCP, and evaluation

## Context

ADR-0019 accepted a small, deterministic Phase 0 context compiler. It combines
exact lexical declarations and current engineering-memory records under a
conservative byte budget. Its profile is intentionally narrow: structural,
reference, history, and compiler-derived providers are explicit omission slots,
not active sources; its ordering is provider-local rank followed by a stable
tie-break.

Phase 2 adds candidate evidence with different precision, coverage, cost, and
staleness characteristics. ADR-0035 proposes a SCIP precision overlay, while
the existing syntax graph can provide bounded structural and reference
expansion, and trusted Git evidence can provide historical context. Selecting
the next useful item by an undocumented provider order would make results hard
to reproduce, obscure missing coverage, and risk spending the whole budget on
one attractive-but-partial evidence source.

The context contract must preserve Phase 0 behavior, disclose every source and
omission, and make future profile revisions measurable rather than silently
changing an agent's input.

## Decision

### Profiles are named, versioned, and immutable inputs

The accepted Phase 0 `context-build` profile remains
`phase0-source-memory-v1`; its selection, budget estimator, output schema, and
observable ordering do not change.

Each Phase 2 compiler invocation names an immutable profile ID and version in
the request, result, diagnostics, cache/receipt identity, benchmark record,
and MCP/CLI output. The initial profile is
`phase2-evidence-balanced-v1`. A profile is selected only through resolved,
monotonic local policy; a remote client cannot choose an unreviewed profile.
Changing provider eligibility, scoring stages, tie-breaks, allocation rules,
estimator, or output schema creates a new profile version.

Profiles express ordered categories and fixed versioned rules, not caller-
supplied weights, learned ranking, vectors, or opaque model scores.

### Candidates must first be independently admissible

Every candidate carries its concrete repository, source view, snapshot,
generation, source slot/package scope, producer/evidence class, coverage, and
bounded content-cost estimate. Candidates whose identities do not exactly match
the pinned context request are rejected before ranking. A precise overlay may
contribute only when it is the selected current immutable overlay for that
generation; incomplete, stale, ambiguous, truncated, or unavailable precise
evidence remains an explicit coverage outcome and never becomes a ranking
bonus.

The compiler deduplicates only evidence items that prove the same exact source
span/content and logical request role. It retains a bounded list of contributing
providers and never collapses conflicting evidence into a fabricated certainty.
Syntax, parser, memory, history, and overlay coverage remain visible even when
their content is not admitted to the final pack. The initial implementation
reports each active provider categorically as `available` or `unavailable`,
with its pre-allocation candidate count; whole-item budget omissions remain a
separate result field.

### Ranking is deterministic and staged

`phase2-evidence-balanced-v1` applies these stages in order:

1. Validate identity, source state, bounds, cancellation, and deadline; reject
   invalid candidates without partial publication.
2. Group exact duplicates and retain all provider/evidence attribution.
3. Order candidate groups by an explicit evidence tier: exact request/source
   anchor, validated unambiguous precision edge, validated lexical/syntax
   edge, current memory evidence, trusted history evidence, then explicitly
   unresolved supporting context.
4. Within a tier, order by provider-local relevance rank, then smaller complete
   content-cost estimate, then stable typed identity fields. No filesystem,
   hash-map, arrival, or database-row order may affect output.
5. Assign a stable final ordinal after the preceding stages; results report the
   tier, provider-local ranks, duplicate attribution, and final ordinal rather
   than an opaque numerical score.

The profile does not infer dynamic dispatch, cross-language edges, package
topology, or historical correspondence. It may prefer applicable validated
SCIP evidence for an exact navigation role only under ADR-0035's evidence
precedence rules; this does not remove syntax candidates or coverage.

### Token allocation is bounded and coverage-preserving

The existing conservative UTF-8-byte upper-bound estimator remains the initial
token estimator until a separately evaluated tokenizer profile is accepted.
Each provider has independent candidate, content-byte, and allocation-work
bounds. An item is admitted whole or omitted categorically; no truncation may
misrepresent a declaration, memory record, relationship, or history excerpt.

The profile admits the request anchor first when one fits. It then makes one
deterministic pass through evidence tiers, allocating an available item only
when its full estimated cost fits the remaining budget. After each tier has had
one bounded opportunity, it repeats in the same order until no remaining item
fits. This prevents a large early provider from masking all other evidence
while preserving a deterministic budget decision. The result reports used and
unused units plus included and omitted counts/reasons for every provider and
tier.

Cancellation, deadline expiry, source/view change, allocation overflow, or a
provider error returns no newly visible context result. Readers continue to
observe their previously pinned immutable generation and context receipt.

### Interfaces and evaluation are evidence-first

The domain holds validated profile/tier/coverage/provenance values only. The
analysis crate ranks immutable provider-neutral inputs without filesystem,
SQLite, Git, or network I/O. The application coordinates profile policy and
ports. The local crate owns bounded provider reads, persistence, and measured
evaluation harnesses; CLI and MCP are thin adapters over the same read use
case. Local stdio MCP remains read-only by default.

Every Phase 2 profile change requires a public synthetic or explicitly public,
pinned corpus evaluation against lexical-only, graph-only, and supported
incumbent baselines. It must report relevant source lines per budget unit,
navigation/task success, precise/syntax coverage, omission/truncation, latency,
and downstream-agent stale-answer rate. Confidential design-partner inputs stay
out of default logs and repository evidence.

## Alternatives considered

### Keep the Phase 0 provider order

It is simple and already deterministic, but cannot express exact precision
evidence, graph expansion, history, or fair allocation without relying on
implicit implementation order.

### Use caller-supplied numeric weights or learned scores

This could tune individual tasks but makes receipts difficult to compare,
enables unreviewed policy changes, and cannot explain why evidence was omitted.

### Merge all provider output and truncate bytes

Byte truncation can split source and memory evidence into misleading fragments;
it also hides which provider consumed the budget.

### Prefer SCIP globally whenever it exists

An overlay can be partial, stale, package-limited, or ambiguous. Global
preference would hide useful syntax coverage and violate ADR-0035.

## Consequences

### Positive

- Context choices, omissions, and profile changes are reproducible and auditable.
- Precision evidence can improve navigation without replacing broad syntax coverage.
- Allocation gives multiple evidence classes a visible bounded opportunity.
- Evaluation has fixed, comparable baselines and explicit stale-answer safety gates.

### Negative and risks

- Provider-neutral inputs and provenance schemas add implementation work.
- Ordered tiers can be less flexible than a task-trained scorer.
- Whole-item admission can leave unused budget when remaining items are too large.
- Comparative downstream-agent evaluation is expensive and must avoid data leakage.

## Validation

- Deterministic/permutation, duplicate-attribution, tie-break, overflow,
  cancellation, deadline, and no-partial-result fixtures for every stage.
- Boundary fixtures for all allocation capacities, whole-item admission, empty
  providers, and each categorical omission reason.
- Generation/view/overlay mismatch, stale, ambiguous, incomplete, and
  conflicting-evidence fixtures proving syntax coverage remains exposed.
- Public pinned-corpus measurements against lexical, graph-only, and supported
  incumbent baselines, including relevant-lines-per-unit and stale-answer rate.
- CLI/MCP contract, persistence/receipt, crash/recovery, and policy-selection
  tests once a profile becomes an active implementation contract.

## Follow-up

- Accept or revise this ADR before changing the active Phase 0 compiler.
- Define domain and application profile/evidence inputs, then implement bounded
  structural, reference, history, and optional precision-overlay providers.
- Version the context receipt and MCP/CLI schemas with per-provider/tier
  coverage and allocation outcomes.
- Run the required comparative evaluation before any profile is made default.

## Supersession

None. This preserves ADR-0019's Phase 0 profile and adds a separately versioned
Phase 2 context path.
