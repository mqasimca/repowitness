# Product

- Status: active development
- Last reviewed: 2026-08-12

RepoWitness gives coding agents bounded, evidence-backed knowledge about one
local repository. Source facts are pinned to immutable index generations;
memory is useful only when revalidation proves it still applies.

## Supported loop

```text
source change
  -> complete manifest and atomic source generation
  -> memory revalidation
  -> bounded context pack with evidence and coverage
```

The Phase 0 source slice supports Rust, Go, TypeScript, TSX, and Python. It
provides syntax-derived declarations, literal search, exact symbol retrieval,
raw syntax observations, a native Rust graph, diagnostics, memory recall and
management, context compilation, and revision-pinned change review.

## User experience contract

- `index` is the normal explicit entry point.
- `onboard` is the private local-state shortcut: it completes the source index
  first, then imports Go SCIP relationships when the root has `go.mod` and
  `scip-go` is available. Use `onboard --full` for graph evidence, `--no-scip`
  to skip enrichment, or `--scip-go <path>` to select the producer.
- Normal `index`, `watch`, and MCP startup remain producer-free.
- `watch` is a foreground reconciler, not a daemon.
- `mcp-serve` accepts one explicit repository or one private catalog of
  onboarded repositories and is read-only by default.
- The catalog reloads its bounded onboarding control file at MCP request
  boundaries; malformed later updates preserve the last valid snapshot.
- Catalog MCP provides bounded FTI search across registered repositories;
  matching results do not claim semantic relationships.
- `repowitness --help` lists only commands that can be invoked.
- Missing, skipped, stale, ambiguous, and truncated work remains categorical;
  it is never presented as confidence.

The project is under development, so indexes may be recreated. The database
is an implementation detail, not a compatibility promise.

## Explicit non-goals

Do not add catalog discovery beyond the explicit onboarded catalog, daemon
coordination, connected-workspace manifests, semantic
cross-repository relationship inference, personal memory,
durable task workflows, remote MCP, vectors, PostgreSQL, plugins, telemetry,
or a UI without a concrete user need and a new decision record.

The native Rust graph and syntax projection are intentionally different:
syntax facts are always available for supported source; graph edges are only
reported when the native bounded graph projection produced them.

## Trust boundaries

Repository source, Git/configuration data, memory files, MCP input, and paths
are hostile. Inputs are bounded and validated before domain construction. Paths
are canonical byte-preserving values; logs and errors do not expose source,
secrets, queries, or raw host paths by default.
