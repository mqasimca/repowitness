# Roadmap

- Status: Proposed
- Last reviewed: 2026-07-28

## Sequencing rule

RepoWitness must prove its differentiating evidence-and-memory loop before becoming a broad indexing platform. Additional languages, server deployment, plugins, runtime ingestion, UI, and alternative search infrastructure are demand-gated.

Every milestone has a versioned benchmark manifest naming its corpora and commits, task set, hardware/OS, resolved configuration, metrics, and numeric pass/fail budgets. Thresholds are agreed before optimizing the feature they gate.

## Phase 0 — supported-language evidence-and-memory alpha

### Goal

Prove that RepoWitness can connect a source change to memory revalidation and a better context pack using the smallest credible implementation.

### Deliver

- Rust, Go, TypeScript, TSX, and Python in one atomic source generation,
  following the
  named design-partner evidence recorded in
  [ADR-0015](adr/0015-phase0-go-and-rust-indexing.md) and
  [ADR-0016](adr/0016-phase0-typescript-and-tsx-indexing.md), extended by
  [ADR-0020](adr/0020-phase0-python-indexing.md).
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

Languages beyond Rust, Go, TypeScript, TSX, and Python, SCIP, PostgreSQL,
remote MCP, persisted tasks, automatic memory extraction, runtime telemetry,
UI, extension execution, raw ranking weights, vectors, and general
`query_graph` compatibility.

The output is a design-partner alpha, not a general public beta.

### Progress through 2026-07-28

| Phase 0 area | State | Verified result |
|---|---|---|
| Rust workspace and engineering baseline | Implemented | Six packages, enforced dependency policy, pinned Rust/MSRV and dependencies, formatting, Clippy, docs, lockfile, license/advisory/source checks, Make targets, and required Ubuntu PR CI |
| Repository and source identity | Implemented | Sanitized bounded Git discovery, canonical Git/worktree receipts, exact byte paths, capability-contained no-follow reads, final stability fence, and fail-closed sparse/gitlink scope |
| Rust, Go, TypeScript, TSX, and Python analysis and incremental reuse | Implemented | Bounded language-specific Tree-sitter facts, one canonical mixed snapshot, five independent artifact identities and payload digests, exact per-language/dialect reuse validation, and clean-versus-incremental equivalence |
| SQLite publication and recovery | Implemented | One clean baseline-version-1 migration and exact ledger row, explicit non-mutating rejection of retired development versions 1–8, persisted exact artifact language, Rust occurrence fingerprints, reviewed correspondence, owned connections, immutable source and memory generations, atomic activation, FTS5 projection switching, startup recovery, checkpoints, online backup, mutation lease, and database file-identity guards |
| Evidence retrieval | Implemented | Bounded literal `code_search`, exact digest-verified `symbol_get`, persisted language, language-specific producer attribution, explicit limitations, match counts, and coverage |
| CLI and local stdio MCP | Implemented | CLI commands cover indexing, exact retrieval, canonical memory write/approval/history/review, memory revalidation/recall, context compilation, diagnostics, and path inspection; MCP exposes five read-only tools by default and adds fixed-actor `memory_manage` only under explicit startup authorization |
| Engineering-memory format | Implemented | Accepted version-1 pure domain values, strict hostile-YAML parser, bounded canonicalizer and deterministic writer, exact golden vectors, independent mutation/property oracle, release resource probes, and a coverage-guided fuzz target |
| Engineering-memory import and persistence | Implemented | Capability-contained worktree admission and canonical writes, scope-checked import, observation-only bounded Git history, separately trusted approvals, immutable SQLite journal rows, and rebuildable current projections pass rollback, reopen, corruption, idempotency, and online-backup tests |
| Correspondence and memory revalidation | Implemented | Versioned Rust fingerprints, exact/same-path-rename/exact-Git-move correspondence, explicit ambiguity and staleness, Git-DAG/worktree validity, head conflicts, idempotent approve/reject/manual-link audit events, deterministic conflict aggregation, and atomic projection activation are implemented |
| Context compiler and read tools | Implemented | Deterministic reciprocal-rank fusion, conservative byte-budget admission, exact source expansion, current-memory exclusion rules, `context_build`, `memory_recall`, and transactionally pinned diagnostics are shared by CLI and MCP with explicit coverage, omissions, limits, cancellation, and source-only fallback |
| Memory management | Implemented | CLI `memory-manage` writes canonical records, approves exact current revisions, records exact manual review, and imports reachable Git history as observations only; opt-in MCP shares the use case without accepting host paths, actor, repository identity, or resource policy |
| Phase 0 evaluation and release gate | Partial | The public pinned product-loop benchmark passes all proposed correctness and numeric ceilings in a dirty development worktree; crash/recovery, adversarial, and four requested neighboring-repository runs pass, while explicit ADR/budget ratification, residual release-matrix cases, clean-revision attestation, and a comparative design-partner outcome remain |

## Phase 1 — trustworthy local core

- Complete blocking ADRs and the Rust engineering/CI baseline.
- Support multi-repository workspaces, packages, branches, revisions, and worktrees.
- Harden watching, recovery, path/case behavior, and cancellation on supported operating systems.
- Expand the first-language graph to references, imports, calls, tests, architecture, trace, and impact.
- Add versioned configuration, monotonic policy merging, `config explain`, and `doctor`.
- Add bounded, schema-tested incumbent compatibility aliases.
- Add another language only for a named user need.

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
- Expand correspondence review to multi-parent and archival workflows; add
  historical “as known at” queries, task checkpoints, verification, and MCP
  Tasks.
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

1. Review the implemented contracts and evidence for proposed
   [ADR-0017](adr/0017-phase0-memory-journal.md),
   [ADR-0018](adr/0018-phase0-memory-revalidation.md),
   [ADR-0019](adr/0019-phase0-context-compilation-and-diagnostics.md), and
   [ADR-0021](adr/0021-phase0-memory-management-and-review.md). Accept, revise,
   or reject them explicitly. The current database contract is the single
   baseline-version-1 migration accepted by
   [ADR-0022](adr/0022-squash-pre-release-sqlite-schema.md).
2. Finish the residual adversarial matrix for rewritten or missing Git
   history, obsolete review snapshots, competing reviewed targets,
   split/merge abstention, and fault injection at every canonical-file and
   SQLite publication stage.
3. Rerun the complete pinned
   [product-loop benchmark](research/phase0-product-benchmark-2026-07-28.md)
   from a clean exact RepoWitness revision, then explicitly ratify or revise
   correctness, latency, RSS, database/WAL, and result-size budgets.
4. Compare RepoWitness with the declared lexical/source-only and
   naive-memory-text baselines on one real design-partner task, and record
   whether evidence or recalled failure changes a useful engineering decision.
5. Keep production reconciliation separate from watcher hints, finish the
   active-`gix` cancellation/performance and Windows path/containment spikes,
   and retain fail-closed sparse and recursive-submodule behavior until their
   coverage contracts are accepted.

See the dated [architecture research](research/architecture-2026-07-22.md) for the spike definitions and [`plan.md`](../plan.md) for the broader product research record.
