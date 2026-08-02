# Roadmap

- Status: Proposed
- Last reviewed: 2026-08-02

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

Languages beyond Rust, Go, TypeScript, TSX, and Python, additional SCIP producers, PostgreSQL,
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
| SQLite publication and recovery | Implemented | Immutable baseline-version-1, compatible accepted parser-diagnostic-version-2, connected-workspace/version-3, SCIP-overlay/version-4, raw-syntax-site/version-7, repository-topology/version-9, exact raw-target-index/version-10, and directional SCIP-relationship-index/version-11 migrations with exact ledger rows and populated upgrade coverage; version 3 adds globally unique bounded source slots, immutable published workspace views, atomic active-view switching, pinned recovery, generation-scoped Rust graph publication, and explicit deterministic bounded retention plan/apply with root revalidation and aggregate audit; version 4 adds source-slot/view-scoped immutable SCIP receipts, atomic active-overlay switching, retention-safe overlay collection, and pinned package-scope symbol/relationship reads; version 7 adds independent all-language raw-site artifacts, complete-generation receipts, integrity-checked reuse, and retention-safe collection; version 9 adds complete path-only topology publication as an activation guard while retaining rejection of retired development schema version 8; version 10 adds an exact raw-target lookup index without altering generation contents; version 11 adds inbound and outbound trace indexes without changing overlay facts |
| Evidence retrieval | Implemented | Bounded multi-language `architecture_map` file inventory, source-only `architecture_overview` with structural path buckets and syntax-only `function main` candidates, separately digested path-only `repository_topology`, literal `code_search`, lexical evidence-only `locate_relevant_paths` grouping returned declaration matches by canonical path, typed exact/prefix `symbol_search`, exact digest-verified `symbol_get`, declaration-contained `outbound_sites`, repository-scoped `test_markers`, and exact raw-target `syntax_site_search` observations without target resolution, a finite typed `code_graph_query` envelope, exact declaration-receipt-to-opaque-SCIP navigation, bounded incoming/outgoing traversal of persisted producer-declared SCIP rows, persisted language, language-specific producer attribution, and native generation-pinned Rust graph status, search, site evidence, architecture, trace, and conservative impact with explicit coverage and limits |
| CLI and local stdio MCP | Implemented | CLI commands cover explicit private onboarding, indexing, topology/architecture-map/overview retrieval, lexical path navigation, typed declaration discovery, exact retrieval, exact raw outbound-site, raw-target, and test-marker observations, six native Rust graph reads, contained source-slot-scoped SCIP import, exact declaration-receipt SCIP symbol navigation, package-scoped SCIP evidence and producer-declared relationship-trace reads, canonical memory write/approval/history/review, memory revalidation/recall, Phase 0 and separate Phase 2 context compilation, diagnostics, and path inspection; MCP exposes twenty-four deterministically ordered read-only tools by default, including finite `code_graph_query`, and adds fixed-actor `memory_manage` only under explicit startup authorization |
| Engineering-memory format | Implemented | Accepted version-1 pure domain values, strict hostile-YAML parser, bounded canonicalizer and deterministic writer, exact golden vectors, independent mutation/property oracle, release resource probes, and a coverage-guided fuzz target |
| Engineering-memory import and persistence | Implemented | Capability-contained worktree admission and canonical writes, scope-checked import, observation-only bounded Git history, separately trusted approvals, immutable SQLite journal rows, and rebuildable current projections pass rollback, reopen, corruption, idempotency, and online-backup tests |
| Correspondence and memory revalidation | Implemented | Versioned Rust fingerprints, exact/same-path-rename/exact-Git-move correspondence, explicit ambiguity and staleness, Git-DAG/worktree validity, head conflicts, idempotent approve/reject/manual-link audit events, deterministic conflict aggregation, and atomic projection activation are implemented |
| Context compiler and read tools | Implemented | Deterministic reciprocal-rank fusion, conservative byte-budget admission, exact source expansion, current-memory exclusion rules, `context_build`, `memory_recall`, and transactionally pinned raw/recognized parser diagnostics are shared by CLI and MCP with explicit coverage, omissions, limits, cancellation, and source-only fallback |
| Memory management | Implemented | CLI `memory-manage` writes canonical records, approves exact current revisions, records exact manual review, and imports reachable Git history as observations only; opt-in MCP shares the use case without accepting host paths, actor, repository identity, or resource policy |
| Configuration and readiness diagnostics | Implemented | Strict bounded schema-version-1 user/workspace/repository layers, deterministic monotonic resolution, path-free `config explain`, read-only `doctor`, runtime request enforcement, diagnostics identity, and pre-runtime MCP profile/write authorization |
| Phase 0 evaluation and release gate | Completed | A checksummed clean Ubuntu 24.04 product-loop attestation passes the ratified correctness and resource budgets; crash/recovery, the complete adversarial release matrix, a controlled lexical/naive-memory comparison, and three repeated isolated Codex utility runs pass. The first [privacy-reviewed real design-partner outcome](research/phase0-design-partner-evaluation-2026-07-30.md) was correct and useful but did not change the decision. The [second outcome](research/phase0-design-partner-evaluation-2026-07-30-task-02.md) materially changed the useful decision relative to both baselines and passed the gate. ADR-0017, ADR-0018, ADR-0019, ADR-0021, and ADR-0023 are accepted. |

## Phase 1 — trustworthy local core (completed 2026-07-31)

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
- Add bounded, schema-tested incumbent aliases. The accepted Phase 1 scope is
  deliberately name-only; request, response, and behavior compatibility remain
  deferred until independently measured.
- Add another language only for a named user need.

Exit conditions passed: crash consistency, identity precision/ambiguity,
cross-platform behavior, query/resource budgets, and explicit name-only
compatibility fixtures. Broader incumbent compatibility remains deferred.

## Phase 2 — precision and full context compiler

Phase 2 starts with accepted
[ADR-0035](adr/0035-phase2-scip-precision-overlay.md), which keeps SCIP as an
optional immutable precision overlay and preserves the accepted syntax graph,
and [ADR-0036](adr/0036-phase2-context-ranking-profiles.md), which preserves
the accepted Phase 0 context profile while defining a separately versioned
evidence-ranking path. Accepted [ADR-0037](adr/0037-phase2-scip-overlay-source-slot-scope.md)
defines the unambiguous source-slot scope required before persistence and
activation.

- Import SCIP and define evidence precedence.
- Add package-aware cross-file resolution.
- Implement deterministic multi-stage ranking, named profiles, and token allocation.
- Continue the staged discovery family proposed by
  [ADR-0042](adr/0042-evidence-backed-agent-code-discovery.md): implemented
  all-language typed declaration search, lexical evidence-only path navigation, source-only architecture overview,
  bounded raw syntax sites, exact raw-target navigation, and the finite `code_graph_query` operation algebra
  are implemented. Package-aware and cross-language relationships remain
  separate evidence profiles rather than syntax-only claims.
- The proposed [ADR-0043](adr/0043-bounded-repository-topology-inventory.md)
  and [ADR-0044](adr/0044-explicit-private-local-onboarding.md) implementation
  slices are complete: topology is a path-only activation-gated receipt and
  onboarding is one explicit-root private-state CLI flow. Their decision status
  remains Proposed pending maintainer review.
- The proposed [ADR-0048](adr/0048-bounded-scip-relationship-traversal.md)
  implementation slice is complete: a selected overlay can expose bounded
  inbound or outbound producer-declared rows with explicit depth, edge, node,
  and output coverage. It remains Proposed pending maintainer review and does
  not broaden the native syntax graph or permit general graph queries.
- Initial progress: `phase2-evidence-balanced-v1` has a separate CLI/MCP contract and
  deterministic pinned syntax/current-memory allocation. An explicit exact SCIP symbol can
  contribute one source-verified unambiguous overlay occurrence. Unique in-scope native graph
  edges contribute structural/import or reference/call targets. Exact syntax identifier spans
  select matching unambiguous SCIP symbols automatically. The history tier admits only a current
  locally approved memory revision with an immutable `observed` Git receipt; it preserves the
  commit as historical provenance without asserting reachability or re-reading historical source.
  A regression fixture proves that a formerly approved and historically observed record is removed
  from both the memory and history providers after source revalidation makes it stale.
  A public synthetic two-task direct-call-chain evaluation compares lexical selector retrieval,
  graph-only selector retrieval, the supported Phase 0 context, and Phase 2; Phase 2 retains the
  direct call target and improves required source lines per content unit for each task. The
  versioned local pinned-corpus runner records all four baseline receipts, warm latency for both
  context profiles, precise-overlay syntax coverage, and stale-provider exclusion. Its opt-in
  isolated Codex evaluation runs the same two downstream tasks against Phase 0 and Phase 2 with
  zero stale-memory uses for both. A graph-only baseline independently labels unique one-hop source
  targets for three public-corpus direct-navigation tasks; Phase 2 improves their aggregate
  relevant-lines-per-content-unit result over Phase 0. The Phase 2 exit gate is met. Fresh macOS
  and Windows evaluator evidence remains intentionally deferred by maintainer direction.
- Add tests, ownership, and Git-history relationships.
- Evaluate context packs against lexical, graph-only, and supported incumbent baselines.
- Add second and third languages only after each meets identity, coverage, and retrieval gates.

Exit when precise overlays improve navigation without hiding syntax coverage, context packs improve relevant lines per token, and downstream-agent tests do not increase stale-answer rate.

## Phase 3 — durable engineering memory beta

- Complete team-memory synchronization and local personal memory under
  [ADR-0038](adr/0038-phase3-memory-scopes-and-kinds.md).
- Add remaining memory kinds and lifecycle policies through a compatible,
  separately versioned profile; preserve the strict version-1 team record.
- Expand correspondence review to multi-parent and archival workflows; add
  historical “as known at” queries under
  [ADR-0039](adr/0039-phase3-historical-correspondence.md).
- Add bounded task checkpoints, verification evidence, negotiated MCP Tasks,
  and ordinary polling fallback under
  [ADR-0040](adr/0040-phase3-task-checkpoints-and-verification.md).
- Test poisoning, secrets, concurrent Git edits, rewritten history, conflict preservation, and projection rebuilds.

Implementation is in progress. The first implementation slice includes the
compatible durable-state migration, owned SQLite task ports, and an opt-in
aggregate-only longitudinal Codex runner. The runner validates five fresh
paired candidate/source-only/naive-memory executions per snapshot and rejects
leakage or non-strict baseline evidence; it does not create an attestation. The
beta exit claim remains withheld until independently reviewed longitudinal
evidence is collected.

Exit when longitudinal tests show fewer repeated failures and less stale-memory use than source-only and naive text-memory baselines, with no cross-scope leakage. This is the first recommended public beta.

## Proposed local multi-repository MCP registry

The proposed [ADR-0049](adr/0049-local-multi-repository-mcp-registry.md)
addresses local operator ergonomics independently of the connected-workspace
indexing contract. It keeps one local stdio connection read-only and routes
each tool call only after an explicit registered opaque repository selection.
Its acceptance gate requires strict hostile-registry admission, path-free
diagnostics, no default or caller-path authority, two-repository isolation,
unchanged single-repository schemas, and installed-binary stdio coverage.
It deliberately defers connected-workspace selection, cross-repository queries,
per-entry configuration, registry reload/mutation, compatibility aliases, and
remote/team transport.

## Proposed opt-in Codex catalog onboarding

The proposed [ADR-0050](adr/0050-opt-in-codex-catalog-onboarding.md) makes the
local evidence surface practical across normal Codex worktrees without changing
the storage or MCP trust model. One explicit global installation owns only
marked Codex MCP and SessionStart records. A catalog server admits exactly the
process-current Git worktree before stdio starts, incrementally refreshes its
private isolated index, and defaults only that fixed entry. The acceptance gate
requires no parent/sibling/home discovery, no catalog mutation before complete
activation, exact cross-catalog selectors, path-free errors, repeat-session
coverage, reversible config ownership, and an explicitly reviewed non-mutating
Codex hook. Daemons, automatic watchers, remote/team catalog state, general
cross-repository queries, and MCP write tools remain deferred.

## Proposed explicit Codex connected-workspace catalog

The proposed [ADR-0051](adr/0051-explicit-codex-connected-workspace-catalog.md)
extends the one-entry Codex experience to an operator-declared product stack
without changing accepted connected-workspace semantics. Its acceptance gate
requires strict private catalog/manifest admission, two-member synthetic
installed-binary coverage, all-source atomic refresh before catalog startup,
default-current-member and explicit-other-member routing, source-slot receipts,
and no host-path disclosure. It deliberately defers membership inference,
automatic updates, catalog reload, background coordination, generic
cross-repository queries, and cross-source relationship heuristics.

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
