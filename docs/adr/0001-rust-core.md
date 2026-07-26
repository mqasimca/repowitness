# ADR-0001: Use Rust for the core engine

- Status: Accepted
- Date: 2026-07-22
- Owners: Project maintainers
- Scope: Indexing engine, storage/retrieval core, CLI, and MCP server

## Context

RepoWitness is intended to be a long-running local process that parses untrusted repositories, performs CPU-heavy incremental analysis, coordinates cancellation, maintains transactional state, and serves bounded protocol requests. It should distribute without requiring a language runtime and remain predictable in background resource use.

The main alternatives are extending the incumbent C implementation, using Go for operational simplicity, using TypeScript for rapid protocol work, or keeping a C core behind a higher-level server.

## Decision

Use Rust 2024 Edition for the Phase 0 engine, SQLite/retrieval core, CLI, and MCP server.

- Tokio handles transport, I/O orchestration, cancellation, and shutdown.
- Bounded Rayon workers or explicit bounded pools handle parsing and CPU-heavy work.
- A dedicated blocking thread owns SQLite writes.
- Existing tree-sitter parsers remain behind their maintained Rust binding.
- Compiler-grade integrations are consumed through SCIP rather than rewritten in Rust.
- TypeScript is reserved for a future MCP App UI if needed.

Continuing the full product in Rust is conditional on the Phase 0 vertical slice meeting agreed correctness, resource, packaging, and maintainability budgets.

## Alternatives considered

### Extend C

This has the lowest rewrite cost and can preserve incumbent algorithms. It also keeps manual memory/concurrency safety at the center of a more stateful, network-capable product. It remains a behavioral and performance baseline and a source of selectively ported tests or algorithms with attribution.

### Go

Go offers fast development, simple deployment, and a mature server ecosystem. It is the fallback if Rust team throughput or native packaging is unacceptable. Its garbage-collected runtime and C parser integration are a less direct fit for the local resource-control objective.

### TypeScript

TypeScript offers fast MCP iteration and is a strong UI choice. CPU-parallel indexing, native grammar packaging, and background memory use make it less suitable for the core engine.

### Hybrid C core

A hybrid can reduce initial porting work but creates two ownership models, duplicated domain types, FFI failure modes, and more complicated releases. It is acceptable only as a temporary measured migration bridge.

## Consequences

### Positive

- Ownership and type invariants help contain concurrency, temporal-state, and scope errors.
- Native binaries can provide predictable startup and background resource behavior.
- The ecosystem provides maintained MCP, SQLite, tree-sitter, serialization, tracing, and parallelism libraries.
- Safe abstractions can minimize the surface requiring native/unsafe review.

### Negative and risks

- Rust learning and review capacity may limit team throughput.
- Compile time, binary size, and native grammar packaging still require engineering.
- A rewrite creates compatibility and semantic-regression risk.
- Rust does not solve faulty graph algorithms, retrieval design, memory poisoning, or query denial of service.

## Validation

Phase 0 must implement one end-to-end Rust slice and record:

- clean and incremental indexing time;
- P50/P95 query latency;
- peak and steady-state RSS;
- database, binary, and startup size/time;
- extraction parity and unresolved-edge counts;
- clean-versus-incremental equivalence;
- crashes and fuzz findings;
- first-party unsafe inventory;
- cross-platform packaging results;
- engineering effort and maintainer review confidence.

Stop or reconsider Rust if no maintainer can review the result confidently, packaging is less reliable than the baseline, or agreed budgets fail without a credible correction.

## Implementation status

The Rust 2024 six-package workspace, local CLI, stdio MCP server, analysis
pipeline, and SQLite persistence/retrieval path are implemented and pass the
locked local verification matrix. Clean-versus-incremental equivalence,
first-party unsafe prohibition, crash/recovery behavior, and initial resource
probes pass. The final Rust go/no-go decision remains open until the complete
memory/context loop, ratified product budgets, fuzzing, and supported-platform
packaging gates pass.

## Supersession

None.
