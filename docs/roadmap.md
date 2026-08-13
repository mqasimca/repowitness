# Roadmap

- Status: development baseline
- Last reviewed: 2026-08-12

## Current baseline

The project has one local workflow:

1. index one explicit repository into SQLite;
2. publish one complete immutable generation;
3. revalidate current engineering memory;
4. retrieve evidence or compile bounded context through CLI or local stdio MCP.

The supported source languages are Rust, Go, TypeScript, TSX, and Python.
The native Rust graph is optional per index profile, while syntax-derived
source facts remain the primary retrieval surface.

An explicit connected-workspace lifecycle is also available for local product
stacks: `repowitness codex workspace create/list/remove` maintains one private
multi-source index behind the same catalog MCP connection. Membership is
operator-supplied; it is never inferred from sibling repositories.

## Development priorities

### 1. Fast and predictable startup

- keep indexing work out of MCP startup where possible;
- use source-only onboarding for the common development path and explicit full
  indexing when graph evidence is needed;
- reuse artifacts only when every semantics-affecting input matches;
- recreate disposable development indexes after schema changes;
- keep `--help` short, accurate, and command-driven.

### 2. Correctness before breadth

- preserve atomic generation publication and previous-generation readability;
- keep cancellation, deadlines, bounds, and coverage explicit;
- test clean versus incremental indexing and crash/recovery behavior;
- keep memory correspondence precision-first and abstaining when ambiguous.

### 3. Evidence-backed retrieval

- maintain literal search, exact symbol retrieval, syntax observations,
  diagnostics, graph reads, memory recall, context compilation, and verify;
- improve output clarity and latency with measurements from synthetic fixtures;
- do not add ranking infrastructure until deterministic retrieval is shown
  insufficient.

## Deferred until demanded

Daemons, personal memory, durable tasks,
remote MCP, PostgreSQL, vectors, plugins, telemetry, and UI are deliberately
out of the current product. The local catalog remains explicitly
onboarding-backed; its membership and control file are read-only to MCP, while
the optional fixed-actor memory capability remains explicit. It reloads its
bounded control file at MCP request boundaries but does not scan repositories
or infer semantic cross-repository relationships. Catalog-wide bounded FTI
search is implemented without a shared graph. Any future addition needs a
narrow design, a benchmark, and a superseding ADR when it changes an accepted
contract.

## Verification gate

```text
cargo fmt --all -- --check
cargo check --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
./scripts/check-docs
git diff --check
```
