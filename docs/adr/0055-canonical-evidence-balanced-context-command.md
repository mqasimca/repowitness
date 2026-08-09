# ADR-0055: Use one canonical evidence-balanced context builder

- Status: Proposed
- Date: 2026-08-09
- Owners: Project maintainers
- Scope: Context CLI and MCP contracts, evaluation, and documentation

## Context

The evidence-balanced context compiler has passed its exit gate, but the CLI
and MCP previously exposed it under a second context name. Users had to choose
between two commands before they could obtain the complete evidence pack. That
separation made ordinary usage harder and duplicated the read-only MCP tool
surface.

ADR-0036 supplied the evaluation rules for the evidence-ranked compiler.
Stable public API commitments remain deferred, but the completed gate provides
the evidence required to make it the sole context interface.

## Decision

`context-build` is the one canonical user-facing CLI command and MCP tool for
bounded context compilation. It runs the immutable `evidence-balanced-v1`
profile and returns its profile ID, version,
scope, tier, provider attribution, coverage, and whole-item omissions.

The CLI command accepts the evidence-balanced profile's single-repository or
explicit connected-workspace source scope and optional exact SCIP symbol. The
MCP `context_build` tool accepts the evidence-balanced request and result
schemas, and remains read-only. Neither interface exposes caller-selected
numeric ranking weights or an unreviewed profile.

The former second command and MCP tool, along with the compatibility-profile
switch and comparison runner, are removed. The MCP catalog consequently
contains one fewer read-only tool.

This decision does not change the evidence eligibility, ranking, allocation,
source fencing, cancellation, or reporting invariants of
`evidence-balanced-v1`. Changing any of those still creates a new
profile version and requires the evaluation specified in ADR-0036.

## Alternatives considered

### Keep separate context commands or a compatibility switch

This preserves old receipts, but makes the complete evidence path look
provisional and leaves users to select an implementation before they can obtain
context.

## Consequences

### Positive

- Normal CLI and MCP clients use one clear context operation.
- Evidence-balanced context becomes the ordinary supported path without
  obscuring its exact profile and provider coverage.
- The MCP surface removes one duplicative tool while preserving bounded,
  read-only behavior.

### Negative and risks

- Existing callers of retired context names or schemas must migrate to
  `context-build` and its evidence-balanced input/output contract.

## Validation

- CLI contract fixtures cover the default single-repository and
  connected-workspace evidence-balanced paths, including optional SCIP input.
- MCP contract fixtures verify one `context_build` tool with the
  evidence-balanced schema.
- Full CLI/MCP contract, formatting, lint, test, documentation, and benchmark
  checks remain required before acceptance.

## Follow-up

- Review this ADR before accepting the exported contract change.
- Publish a migration note before declaring a stable public API.

## Supersession

This supersedes ADR-0036 only for its exported second context command and tool
path. ADR-0036's named-profile, evidence, ranking, allocation, and evaluation
invariants remain in effect.
