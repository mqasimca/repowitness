# ADR-0045: Resolve exact declaration receipts to SCIP symbols

- Status: Proposed
- Date: 2026-08-02
- Owners: Project maintainers
- Scope: exact declaration receipts, local SCIP-overlay reads, CLI, and local
  stdio MCP

## Context

`symbol_search` gives an agent a generation-pinned multi-language declaration
receipt, while the accepted SCIP overlay can return provider-declared incoming
and outgoing relationships only when the caller already has an opaque SCIP
symbol. Requiring an agent to guess that opaque identifier defeats discovery;
matching by source name would be unsound.

## Decision

Add read-only `scip_symbol_resolve` as the narrow bridge from one
`symbol_search` candidate's immutable selector and name span to an opaque
provider symbol.

- Its required selector is the receipt's snapshot, generation, path, content
  digest, artifact digest, fact ordinal, and declaration-name byte span.
- It validates the declaration remains in the selected source slot and immutable
  workspace view, then reads exactly one pinned SCIP overlay at the same canonical path,
  content digest, and byte span.
- It returns only `not_produced`, `no_exact_match`, `ambiguous`, or `exact`.
  An exact result is an opaque symbol for the existing bounded `scip_evidence`
  read; it is not itself relationship evidence.
- The bridge never resolves by name, edits source or the overlay, expands an
  open graph query, or claims a relationship when SCIP has no exact occurrence.
  A stale or inconsistent declaration receipt fails closed.

## Alternatives considered

### Ask callers to provide an opaque SCIP symbol

Rejected. It makes the precision overlay impractical for agent discovery and
encourages unverified symbol guessing.

### Infer symbols from names, qualified names, or paths

Rejected. Re-exports, overloads, macros, generated code, and provider naming
rules make these fields insufficient identity evidence.

### Add Cypher or a second general relationship resolver

Rejected. It would bypass the bounded typed-reader, provider, generation, and
resource contracts of ADR-0035 and ADR-0042.

## Consequences

### Positive

- An agent can follow `symbol_search` → `scip_symbol_resolve` →
  `scip_evidence` without guessing provider identifiers.
- Existing SCIP relationship attribution, package scope, output limits, and
  categorical uncertainty remain authoritative.

### Negative and risks

- Only an imported compatible SCIP overlay can produce an exact symbol.
- A source reindex between the declaration receipt and bridge request causes a
  safe failure rather than automatic retargeting.

## Validation

- Application tests reject a mismatched immutable context.
- A SQLite overlay fixture proves a `symbol_search`-derived immutable selector
  resolves only at its matching content digest and name span.
- MCP wire, tool-inventory, and installed stdio tests prove strict input,
  redaction, categorical no-overlay behavior, and deterministic exposure.

## Follow-up

Keep relationship traversal in `scip_evidence`. Any richer cross-provider or
cross-repository relationship model needs independent evidence and an ADR.

## Supersession

None.
