# Roadmap

- Status: Proposed
- Last reviewed: 2026-07-26

## Sequencing rule

RepoWitness must prove its differentiating evidence-and-memory loop before becoming a broad indexing platform. Additional languages, server deployment, plugins, runtime ingestion, UI, and alternative search infrastructure are demand-gated.

Every milestone has a versioned benchmark manifest naming its corpora and commits, task set, hardware/OS, resolved configuration, metrics, and numeric pass/fail budgets. Thresholds are agreed before optimizing the feature they gate.

## Phase 0 — one-language evidence-and-memory alpha

### Goal

Prove that RepoWitness can connect a source change to memory revalidation and a better context pack using the smallest credible implementation.

### Deliver

- Rust as the first indexed language unless design-partner evidence changes the choice.
- One repository and active worktree backed by SQLite.
- Minimal tree-sitter indexing, FTS5 search, exact source manifests, content-addressed analysis artifacts, immutable generations, and content-digested evidence.
- CLI and stdio MCP access to `code_search`, `symbol_get`, minimal `context_build`, `memory_recall`, `memory_manage`, and `diagnostics`.
- Manual decision and failure records in `.code-memory/`.
- Immutable memory versions, Git-ancestry validity, tombstones, idempotent SQLite projection, and audit events.
- Precision-first rename/move correspondence, explicit ambiguity, and meaning-change staleness.
- One public source-change-to-revalidated-context fixture.
- Lexical/source-only and naive-memory baselines plus one real design-partner task.

### Exit criteria

- The entire change, revalidation, and context-building loop runs reproducibly.
- Strongly supported renames relink; ambiguous changes return `needs_review`; semantic changes make affected memory stale or indeterminate.
- Every material result includes revision, generation, evidence, precision, digest, coverage, and unresolved work.
- Cancellation and failure preserve the previous active generation.
- Clean and incremental output is equivalent on the fixture.
- Missed/duplicated watcher events converge through reconciliation; watcher output is not required for correctness.
- The bundled/verified SQLite contains the WAL-reset fix and passes crash, checkpoint, online-backup, and restore tests.
- Ratified correctness, resource, latency, and retrieval budgets pass.
- At least one real task shows that evidence or recalled failure changes an engineering decision.

### Explicitly deferred

Additional languages, SCIP, PostgreSQL, remote MCP, persisted tasks, automatic memory extraction, runtime telemetry, UI, extension execution, raw ranking weights, vectors, and general `query_graph` compatibility.

The output is a design-partner alpha, not a general public beta.

### Progress through 2026-07-26

| Phase 0 area | State | Verified result |
|---|---|---|
| Rust workspace and engineering baseline | Implemented | Six packages, enforced dependency policy, pinned Rust/MSRV and dependencies, formatting, Clippy, docs, lockfile, license/advisory/source checks, and Make targets |
| Repository and source identity | Implemented | Sanitized bounded Git discovery, canonical Git/worktree receipts, exact byte paths, capability-contained no-follow reads, final stability fence, and fail-closed sparse/gitlink scope |
| Rust analysis and incremental reuse | Implemented | Bounded Tree-sitter facts, canonical manifests/snapshots/artifact keys, independent payload digests, exact reuse validation, and clean-versus-incremental equivalence |
| SQLite publication and recovery | Implemented | Versioned migrations, owned connections, immutable generations, atomic activation, FTS5 projection switching, startup recovery, checkpoints, online backup, mutation lease, and database file-identity guards |
| Evidence retrieval | Implemented | Bounded literal `code_search`, exact digest-verified `symbol_get`, explicit evidence, limitations, match counts, and coverage |
| CLI and local stdio MCP | Implemented | `index`, `search`, `symbol-get`, `mcp-serve`, and `inspect-paths`; MCP exposes the same two read-only application use cases |
| Engineering-memory format | Spike only | Strict hostile-YAML and canonical-digest tests pass; ADR-0014 remains proposed and no production memory parser or store exists |
| Correspondence and memory revalidation | Not implemented | Logical relinking, ambiguity review, Git-DAG validity, tombstones, audit projection, and staleness remain |
| Context compiler and remaining tools | Not implemented | Rank fusion, token allocation, `context_build`, memory tools, and diagnostics remain |
| Phase 0 evaluation and release gate | Partial | Pinned preparation measurements, crash/recovery probes, and two neighboring real-repository end-to-end runs pass; ratified full-corpus retrieval/context/memory budgets and a design-partner outcome remain |

## Phase 1 — trustworthy local core

- Complete blocking ADRs and the Rust engineering/CI baseline.
- Support multi-repository workspaces, packages, branches, revisions, and worktrees.
- Harden watching, recovery, path/case behavior, and cancellation on supported operating systems.
- Expand the first-language graph to references, imports, calls, tests, architecture, trace, and impact.
- Add versioned configuration, monotonic policy merging, `config explain`, and `doctor`.
- Add bounded, schema-tested incumbent compatibility aliases.
- Add a second language only for a named user need.

Exit when crash consistency, identity precision/ambiguity, cross-platform behavior, query/resource budgets, and explicit compatibility fixtures pass.

## Phase 2 — precision and full context compiler

- Import SCIP and define evidence precedence.
- Add package-aware cross-file resolution.
- Implement deterministic multi-stage ranking, named profiles, and token allocation.
- Add tests, ownership, and Git-history relationships.
- Evaluate context packs against lexical, graph-only, and supported incumbent baselines.
- Add second and third languages only after each meets identity, coverage, and retrieval gates.

Exit when precise overlays improve navigation without hiding syntax coverage, context packs improve relevant lines per token, and downstream-agent tests do not increase stale-answer rate.

## Phase 3 — durable engineering memory beta

- Complete team-memory synchronization and local personal memory.
- Add remaining memory kinds and lifecycle policies.
- Add manual correspondence review, historical “as known at” queries, task checkpoints, verification, and MCP Tasks.
- Test poisoning, secrets, concurrent Git edits, rewritten history, conflict preservation, and projection rebuilds.

Exit when longitudinal tests show fewer repeated failures and less stale-memory use than source-only and naive text-memory baselines, with no cross-scope leakage. This is the first recommended public beta.

## Phase 4 — demand-gated team server

Begin only when users need centralized concurrency, permissions, or operations that local SQLite plus Git cannot provide.

- Prototype PostgreSQL before extracting a shared storage behavior contract.
- Add remote MCP authorization, team/user/tenant isolation, retention, backup/restore, and operational diagnostics.
- Keep backend-specific search and concurrency implementations.
- Provide supported migration/import paths between local and server profiles.

Exit when demand justifies the operational cost and both backends agree on documented domain invariants without pretending their performance behavior is identical.

## Phase 5 — optional observed behavior, UI, and ecosystem

Prioritize these independently using measured demand:

- privacy-preserving OTLP/profile import;
- static-versus-observed path analysis;
- MCP App evidence review UI with text fallback;
- more grammar packs and SCIP producers;
- supervised extension SDK and conformance kit;
- WASI components if their sandboxing value justifies runtime cost;
- structural query/refactor packs;
- dependency/SARIF overlays and cross-service links;
- offline embeddings and optional vector indexes;
- architecture rules and drift alerts;
- signed registry distribution.

## Immediate backlog

1. Review and either accept, revise, or reject proposed
   [ADR-0014](adr/0014-phase0-engineering-memory-record.md). Do not promote the
   test-only YAML stack before that decision and its hostile-input, fuzz,
   dependency, and resource gates pass.
2. Implement the accepted bounded version-1 team-memory record, canonical
   writer, Git import, immutable SQLite projection, tombstones, audit history,
   and deterministic rebuild without weakening ADR-0005 or ADR-0007.
3. Implement precision-first occurrence correspondence, explicit ambiguity,
   manual review, Git-DAG validity, and memory staleness with durable
   regression fixtures for rename, move, semantic edit, split/merge, shallow
   history, and rewritten history.
4. Implement deterministic retrieval fusion and the minimal token-budgeted
   context compiler, then expose the remaining Phase 0 memory/context tools
   through the shared application boundary, CLI, and local stdio MCP.
5. Extend the pinned mini-redis runner through persistence, exact reuse,
   retrieval, MCP, source-change revalidation, and context compilation.
   Ratify correctness, latency, RSS, database/WAL, and result-size budgets
   before claiming the Phase 0 exit gate.
6. Keep production reconciliation separate from watcher hints, finish the
   active-`gix` cancellation/performance and Windows path/containment spikes,
   and retain fail-closed sparse and recursive-submodule behavior until their
   coverage contracts are accepted.
7. Run the complete Phase 0 identity, crash/recovery, retrieval, MCP,
   memory-revalidation, context-quality, and Rust go/no-go gates on the pinned
   corpus and at least one real design-partner task before expanding scope.

See the dated [architecture research](research/architecture-2026-07-22.md) for the spike definitions and [`plan.md`](../plan.md) for the broader product research record.
