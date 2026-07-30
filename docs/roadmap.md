# Roadmap

- Status: Proposed
- Last reviewed: 2026-07-30

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
- Lexical/source-only and naive-memory baselines plus a privacy-reviewed real
  design-partner task evaluation; the gate requires a material decision change.

### Exit criteria

- The entire change, revalidation, and context-building loop runs reproducibly.
- Strongly supported renames relink; ambiguous changes return `needs_review`; semantic changes make affected memory stale or indeterminate.
- Every material result includes revision, generation, evidence, precision, digest, coverage, and unresolved work.
- Cancellation and failure preserve the previous active generation.
- Clean and incremental output is equivalent on the fixture.
- Synthetic missed, duplicated, reordered, and coalesced watcher hints converge
  through complete-manifest reconciliation; production watcher ingestion is a
  Phase 1 deliverable and is never the source of correctness.
- The bundled/verified SQLite contains the WAL-reset fix and passes crash, checkpoint, online-backup, and restore tests.
- Ratified correctness, resource, latency, and retrieval budgets pass.
- At least one real task shows that evidence or recalled failure changes an engineering decision.

### Explicitly deferred

Languages beyond Rust, Go, TypeScript, TSX, and Python, SCIP, PostgreSQL,
remote MCP, persisted tasks, automatic memory extraction, runtime telemetry,
UI, extension execution, raw ranking weights, vectors, and general
`query_graph` compatibility.

The output is a design-partner alpha, not a general public beta.

### Progress through 2026-07-30

| Phase 0 area | State | Verified result |
|---|---|---|
| Rust workspace and engineering baseline | Implemented | Six packages, enforced dependency policy, pinned Rust/MSRV and dependencies, formatting, Clippy, docs, lockfile, license/advisory/source checks, Make targets, and required Ubuntu PR CI |
| Repository and source identity | Implemented | Sanitized bounded Git discovery, canonical Git/worktree receipts, exact byte paths, capability-contained no-follow reads, final stability fence, and fail-closed sparse/gitlink scope |
| Rust, Go, TypeScript, TSX, and Python analysis and incremental reuse | Implemented | Bounded language-specific Tree-sitter facts, one canonical mixed snapshot, five independent artifact identities and payload digests, a checksum-pinned reviewed TypeScript/TSX grammar fix, exact per-language/dialect reuse validation, and clean-versus-incremental equivalence |
| SQLite publication and recovery | Implemented foundation | Immutable baseline-version-1, compatible accepted parser-diagnostic-version-2, and provisional connected-workspace-version-3 migrations with exact ledger rows and populated upgrade coverage; version 3 adds globally unique bounded source slots, immutable published workspace views, atomic active-view switching, pinned recovery, generation-scoped Rust graph publication, and explicit deterministic bounded retention plan/apply with root revalidation and aggregate audit; retention defaults, budgets, and the migration remain proposed pending ratification |
| Evidence retrieval | Implemented | Bounded literal `code_search`, exact digest-verified `symbol_get`, persisted language, language-specific producer attribution, and native generation-pinned Rust graph status, search, site evidence, architecture, trace, and conservative impact with explicit coverage and limits |
| CLI and local stdio MCP | Implemented | CLI commands cover indexing, exact retrieval, six native Rust graph reads, canonical memory write/approval/history/review, memory revalidation/recall, context compilation, diagnostics, and path inspection; MCP exposes eleven deterministically ordered read-only tools by default and adds fixed-actor `memory_manage` only under explicit startup authorization |
| Engineering-memory format | Implemented | Accepted version-1 pure domain values, strict hostile-YAML parser, bounded canonicalizer and deterministic writer, exact golden vectors, independent mutation/property oracle, release resource probes, and a coverage-guided fuzz target |
| Engineering-memory import and persistence | Implemented | Capability-contained worktree admission and canonical writes, scope-checked import, observation-only bounded Git history, separately trusted approvals, immutable SQLite journal rows, and rebuildable current projections pass rollback, reopen, corruption, idempotency, and online-backup tests |
| Correspondence and memory revalidation | Implemented | Versioned Rust fingerprints, exact/same-path-rename/exact-Git-move correspondence, explicit ambiguity and staleness, Git-DAG/worktree validity, head conflicts, idempotent approve/reject/manual-link audit events, deterministic conflict aggregation, and atomic projection activation are implemented |
| Context compiler and read tools | Implemented | Deterministic reciprocal-rank fusion, conservative byte-budget admission, exact source expansion, current-memory exclusion rules, `context_build`, `memory_recall`, and transactionally pinned raw/recognized parser diagnostics are shared by CLI and MCP with explicit coverage, omissions, limits, cancellation, and source-only fallback |
| Memory management | Implemented | CLI `memory-manage` writes canonical records, approves exact current revisions, records exact manual review, and imports reachable Git history as observations only; opt-in MCP shares the use case without accepting host paths, actor, repository identity, or resource policy |
| Configuration and readiness diagnostics | Implemented under proposed ADR-0025 | Strict bounded schema-version-1 user/workspace/repository layers, deterministic monotonic resolution, path-free `config explain`, read-only `doctor`, runtime request enforcement, diagnostics identity, and pre-runtime MCP profile/write authorization |
| Phase 0 evaluation and release gate | Completed | A checksummed clean Ubuntu 24.04 product-loop attestation passes the ratified correctness and resource budgets; crash/recovery, the complete adversarial release matrix, a controlled lexical/naive-memory comparison, and three repeated isolated Codex utility runs pass. The first [privacy-reviewed real design-partner outcome](research/phase0-design-partner-evaluation-2026-07-30.md) was correct and useful but did not change the decision. The [second outcome](research/phase0-design-partner-evaluation-2026-07-30-task-02.md) materially changed the useful decision relative to both baselines and passed the gate. ADR-0017, ADR-0018, ADR-0019, ADR-0021, and ADR-0023 are accepted. |

## Phase 1 — trustworthy local core

- Complete blocking ADRs and the Rust engineering/CI baseline.
- Support multi-repository workspaces plus explicit package scopes, branches,
  revisions, and caller-provided worktrees through the source-slot contract in
  [ADR-0026](adr/0026-connected-workspace-source-slots-and-views.md) and the
  selector refinement in
  [ADR-0031](adr/0031-source-slot-selectors-and-package-scopes.md). Phase 1
  package scope does not imply package-manager inference or package-aware
  graph resolution.
- Harden watching, recovery, path/case behavior, and cancellation on supported operating systems.
- Ratify and stabilize the implemented Rust-only syntax-derived graph
  publication, evidence, architecture, trace, and impact contract; retain
  explicit abstention for package-aware resolution, macro expansion, SCIP,
  dynamic dispatch, and cross-language edges.
- Ratify and stabilize the implemented versioned configuration, monotonic
  policy, `config explain`, and `doctor` contract.
- Add bounded, schema-tested incumbent aliases with independently measured
  compatibility levels; the implemented subset currently claims names only.
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

1. Completed 2026-07-29: accept
   [ADR-0017](adr/0017-phase0-memory-journal.md),
   [ADR-0019](adr/0019-phase0-context-compilation-and-diagnostics.md), and
   [ADR-0023](adr/0023-vendor-typescript-grammar-fix.md). Completed 2026-07-30:
   the passing [second real design-partner outcome](research/phase0-design-partner-evaluation-2026-07-30-task-02.md)
   supports accepting [ADR-0018](adr/0018-phase0-memory-revalidation.md), then
   [ADR-0021](adr/0021-phase0-memory-management-and-review.md). The current
   database contract remains the immutable baseline accepted by
   [ADR-0022](adr/0022-squash-pre-release-sqlite-schema.md) plus the compatible
   version-2 migration accepted by
   [ADR-0024](adr/0024-persist-parser-diagnostics-migration.md).
2. Completed 2026-07-28: the adversarial release matrix covers rewritten,
   pruned, and missing-object Git history; obsolete review snapshots;
   competing reviewed targets; explicit split/merge abstention; and
   deterministic failure at every canonical-file and SQLite publication stage,
   including transaction commit.
3. Completed 2026-07-29: the
   [clean Ubuntu 24.04 attestation](research/phase0-clean-benchmark-attestation-2026-07-29.md)
   passes from an exact clean RepoWitness revision, and the unchanged
   correctness, latency, RSS, database/WAL, and result-size budgets are
   ratified.
4. Completed 2026-07-28 for the controlled public fixture: RepoWitness,
   lexical/source-only, and naive-memory-text now run against one pinned
   before/after oracle. Three repeated isolated Codex paired runs make both
   correct decisions, use current memory, ignore stale memory, and rate the
   packet useful. Completed 2026-07-30: the first
   [privacy-reviewed real design-partner outcome](research/phase0-design-partner-evaluation-2026-07-30.md)
   was correct and useful but did not change the decision relative to either
   baseline, so it did not pass the gate. The [second outcome](research/phase0-design-partner-evaluation-2026-07-30-task-02.md)
   then passed under the
   [design-partner evaluation protocol](research/phase0-design-partner-evaluation-protocol.md)
   by changing a useful engineering decision relative to both baselines.
5. Completed 2026-07-28 for Phase 0 Git discovery: a synthetic 50,000-path
   comparison found `gix` faster, but its index-open path does not provide the
   caller-owned active cancellation/deadline boundary. Retain sanitized Git in
   production and exact-pinned `gix` as a development oracle. Keep production
   reconciliation separate from watcher hints, finish the Windows
   path/containment spikes, and retain fail-closed sparse and
   recursive-submodule behavior until their coverage contracts are accepted.
   Actual nested-submodule and concurrent sparse/gitlink-mode regressions now
   enforce that boundary.

See the dated [architecture research](research/architecture-2026-07-22.md) for the spike definitions and [`plan.md`](../plan.md) for the broader product research record.
