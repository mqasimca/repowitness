# Product

- Status: Draft
- Last reviewed: 2026-08-02

## Definition

RepoWitness is a local code-intelligence and engineering-memory engine for
coding agents and developers.

> RepoWitness gives coding agents verified knowledge about a project. The
> knowledge identifies its source revision and whether it is still valid.

Its trust promise is:

> Each retrieved fact identifies its source, precision, valid time, and possible
> invalidation.

## Problem

Coding agents repeatedly rebuild context. Across tasks, sessions, branches, and
developers, they can:

- rediscover architecture and conventions;
- repeat approaches that already failed;
- trust documentation or memories that no longer match the active revision;
- retrieve many related files without finding the smallest useful working set;
- confuse a syntax-derived guess with a compiler-confirmed or runtime-observed fact;
- lose hypotheses, commands, diagnostics, and verification outcomes;
- treat generated summaries as truth without supporting evidence.

The main problem is not storage. The main problem is whether engineering
knowledge has evidence, applies to the current code, and is safe to use.

## Users and jobs

### Coding agent

- Find definitions, references, relationships, tests, history, and prior decisions.
- Build a small evidence pack for a task and token budget.
- Resume verified work without repeating failed approaches.
- Recognize unresolved, incomplete, conflicting, or stale information.

### Developer

- Inspect why a result was returned and which source supports it.
- Record decisions, failures, and procedures with explicit scope and evidence.
- Review shared memory through normal Git workflows.
- Check indexing and memory validity. Do not debug an opaque retrieval system.

### Team or platform owner

- Keep source and personal memory local by default.
- Define approval, sharing, retention, secret, and resource policies.
- Measure correctness, retrieval quality, latency, resource use, and scope isolation.
- Add a central service only when the team needs it.

## Differentiating loop

No one part is unique. The product combines source intelligence, engineering
experience, time, and validation:

```text
source change
    -> update code facts atomically
    -> identify affected memories, tasks, tests, and observations
    -> preserve, contradict, revalidate, or mark knowledge stale
    -> compile a new evidence-backed context pack
    -> record the next verified success or failure
```

This enables:

- Retrieval with evidence, coverage, and limits.
- Memory that follows supported refactors without guessing when a match is
  unclear.
- Queries that separate project validity from the time when the system recorded
  the knowledge.
- Verified procedures and failures instead of unreviewed chat summaries.
- Deterministic context compilation within a token budget.
- Team knowledge that Git can review, and local personal knowledge.
- More precision from syntax, SCIP or compiler evidence, human decisions, and
  optional runtime observations.

## Product principles

1. Current source is authoritative for source-derived facts.
2. Abstention is a feature. Unresolved and skipped work must stay visible.
3. Evidence and coverage are part of every material result.
4. Stored memories are claims. Evidence and policy must support them.
5. Time and scope are primary data: repository, revision, branch, worktree,
   path, symbol, user, and team.
6. Use deterministic results before opaque relevance tuning.
7. Local operation needs no LLM, embedding service, hosted account, or network
   connection.
8. Customization cannot bypass provenance, audit, scope isolation, query limits,
   or atomic index generations.
9. Optional compiler and runtime evidence adds to syntax coverage. It does not
   replace or hide syntax coverage.
10. Measure engineering results. Do not measure MCP tool count or supported
    language count.

## Phase 0 product slice

The first design-partner alpha proves one complete loop for Rust, Go,
TypeScript, TSX, and Python:

1. Index any mixture of the five supported languages into SQLite.
2. Retrieve evidence and build a compact context pack.
3. Attach a manually approved decision or failure to a logical symbol.
4. Rename or meaningfully change the symbol.
5. Relink only when evidence is strong. Otherwise, request review or mark the
   memory stale.
6. Rebuild the context pack and expose the changed validity.
7. Compare a real task with lexical/source-only retrieval and with RepoWitness.

The alpha does not include other languages, PostgreSQL, remote MCP, automatic
memory extraction, stored tasks, runtime telemetry, a UI, plugin execution,
vector retrieval, raw ranking weights, or general graph-query compatibility.

### Implemented and verified

As of 2026-07-30, the Phase 0 path from source to revalidated context is
implemented. Its local memory-management base is also implemented:

- bounded sanitized-Git discovery and canonical Git/worktree receipts;
- capability-contained, no-follow supported-language source reads with final
  identity and content revalidation;
- deterministic language-specific Tree-sitter symbol extraction, one mixed
  canonical manifest and source snapshot, independent Rust, Go, TypeScript,
  TSX, and Python artifact keys, and exact clean-versus-incremental reuse;
- owned SQLite connections, immutable generations, atomic activation,
  bounded/cancellable startup recovery, double-buffered FTS5, checkpoints, and
  validated online backup;
- bounded multi-language `architecture_map` file inventory with exact source/artifact receipts,
  source-only `architecture_overview` structural orientation and syntax-only `function main` candidates,
  and a separately digested path-only `repository_topology` inventory of cached tracked
  non-source assets (excluding untracked and deleted paths),
  evidence-bearing literal `code_search`, bounded lexical `locate_relevant_paths` that groups returned declaration evidence by canonical path without semantic inference, typed exact/prefix `symbol_search`, exact digest-verified `symbol_get`,
  exact declaration-contained `outbound_sites`, repository-scoped `test_markers`, and exact raw-target `syntax_site_search` raw parser observations without target resolution,
  and the finite MCP `code_graph_query` envelope over those discovery operations,
  and immutable-view Rust graph status, exact-name search, site evidence,
  architecture, trace, and conservative impact application use cases;
- `index`, `onboard`, `architecture-map`, `architecture-overview`, `repository-topology`, `search`, `locate-relevant-paths`, `symbol-search`, `outbound-sites`, `syntax-site-search`, `test-markers`,
  `symbol-get`, `scip-symbol-resolve`, `scip-relationship-trace`, `graph`, `memory-manage`, `memory-revalidate`, `memory-recall`,
  `context-build`, `diagnostics`, `mcp-serve`, `codex`, and `inspect-paths` CLI commands,
  plus path-free `config explain` and read-only `doctor`; explicit bounded
  user/workspace/repository configuration is
  resolved once and enforced by indexing, retrieval, graph reads, context,
  diagnostics, and MCP startup, with twenty-four read-only retrieval, graph,
  context, and diagnostic tools exposed by default over local stdio MCP and an
  explicitly enabled, fixed-actor `memory_manage` mutation tool;
- a proposed local multi-repository MCP registry mode that routes each
  read-only tool request through one explicit opaque registered repository ID;
  it does not accept paths from callers, cross-query repositories, alter an
  index, or broaden the single-repository startup contract;
- a proposed opt-in Codex catalog mode and idempotent install/remove command:
  one global local MCP connection admits and refreshes only the current Git
  worktree at process startup, keeps its bounded private catalog path-free to
  callers, and defaults only that process-fixed repository without adding a
  daemon, watcher, home/sibling scan, remote service, or MCP mutation;
- a proposed explicit Codex connected-workspace catalog: an operator may name
  two through thirty-two supplied worktrees as one product stack; starting in
  any member refreshes and atomically publishes that full source-slot view,
  while cross-member inspection uses opaque selectors and only attributed
  producer evidence can claim a cross-repository relationship. It never
  infers stack membership from layout or imports, exposes paths, or adds a
  generic cross-repository query;
- the accepted bounded version-1 memory domain, hostile-YAML parser,
  canonicalizer, deterministic writer, and capability-contained exact-file
  worktree admission;
- a scope-checked import use case and owned-writer SQLite baseline journal with
  immutable versions, normalized evidence, tombstones, separately trusted
  idempotent observation/approval audit events, rollback, reopen, corruption
  detection, and online-backup coverage;
- contained canonical record create/update/tombstone writes, fixed
  high-confidence secret rejection, bounded observation-only reachable-Git
  history import, and explicit local approval;
- an immutable SQLite baseline, compatible accepted version-2 migration, and
  accepted version-3 connected-workspace migration containing occurrence
  fingerprints, immutable current-memory projection, idempotent manual
  correspondence review, precision-first Rust correspondence,
  Git-DAG/worktree validity, conflicts, categorical staleness and review
  states, bounded current-memory recall, atomic projection activation, bounded
  source-slot membership, and immutable workspace-view publication;
- deterministic reciprocal-rank context compilation from exact source and
  eligible current memory, conservative byte-budget admission, explicit
  omissions, transactionally pinned diagnostics, and source-only fallback;
- a public pinned-corpus product-loop runner covering persistence, exact reuse,
  retrieval, default-read-only MCP, canonical write/approval, current-memory
  context, one-file invalidation, stale recall, and stale-memory exclusion; and
- regression coverage for cancellation, deadlines, limits, hostile Git and
  path state, stale generations, source mutation, process termination,
  database alias/replacement races, recovery overflow, migration, backup, and
  clean-versus-incremental equivalence.

The team tested this slice on the pinned mini-redis product benchmark, this
workspace, temporary adversarial and mixed-language repositories, and
confidential external smoke inputs. The clean Ubuntu 24.04 benchmark passes
every numeric limit. It gave the evidence for maintainer ratification. One
isolated public Codex task also passes three paired runs. It makes both correct
decisions, uses current memory, ignores stale memory, and rates each MCP packet
as useful. These runs do not meet the Phase 0 real design-partner exit criteria.

TypeScript and TSX are separate syntax-only dialects. The profile does not
evaluate TypeScript types, `tsconfig.json`, package or module resolution,
references, call sites, or active build targets. It does not select JavaScript
or MJS files.

Python is a separate syntax-only language for case-sensitive `.py` and `.pyi`
paths. The profile does not run repository code, load Python environments,
resolve imports or types, evaluate decorators, infer dynamic dispatch, or
extract references and calls.

### Phase 0 completion and remaining product gates

The local product loop and clean release benchmark pass, and the budgets are
ratified. The first
[privacy-reviewed real design-partner outcome](research/phase0-design-partner-evaluation-2026-07-30.md)
was correct and useful but did not change the decision. The
[second outcome](research/phase0-design-partner-evaluation-2026-07-30-task-02.md)
changed the useful decision relative to both declared baselines and completed
the Phase 0 product gate. ADR-0017, ADR-0018, ADR-0019, ADR-0021, and ADR-0023
are accepted. The rewritten-history, obsolete-snapshot, competing-review,
split/merge, and publication-fault matrix passes. A stable public API is still
deferred. This design-partner alpha does not support a public-beta or
production-readiness claim.

## Phase 3 durable engineering-memory beta

Phase 3 extends the completed local loop without changing the evidence-first
trust promise. It adds isolated local personal memory, additional bounded
memory kinds, reviewed multi-parent team merges, archival and `as-known-at`
reads, and resumable task/verification receipts. Team records remain
Git-reviewable; personal records never enter the repository or a default MCP
response. A procedure becomes verified guidance only after a successful
evidence-bearing verification receipt. Opt-in MCP Tasks project durable
application-owned task identities and states; only their bounded result
payload cache is process-local. Ordinary CLI and polling reads remain the
mandatory fallback for clients that do not negotiate the extension.

The beta gate is longitudinal: a declared source-only and naive-text-memory
baseline must show more repeated failures or stale-memory use than the scoped
memory path, while isolation fixtures show no personal-to-team or
cross-repository leakage. The opt-in aggregate-only runner validates the
declared paired execution and receipt shape, but it remains `not-attested` and
does not by itself satisfy the beta gate.

## Non-goals for the first public beta

- Matching broad claimed language counts.
- Implementing compiler frontends or a new language server for every language.
- Becoming an autonomous coding agent.
- Promoting conversations directly into durable team truth.
- Requiring embeddings, a graph database, a hosted account, or network access.
- Competing through dozens of narrowly different MCP tools.
- Automatically rewriting source before indexing and evidence semantics are trustworthy.

## Success signals

RepoWitness is useful when reproducible fixtures and design-partner tasks show that:

- an agent finds the correct working set with better token efficiency and no higher stale-answer rate than lexical search;
- every material result exposes evidence, precision, revision, coverage, and unresolved work;
- incremental and clean indexes are equivalent;
- supported renames preserve applicable memory, ambiguous matches request review, and meaning-changing edits invalidate memory;
- failed approaches are recalled only where their evidence still applies;
- verified procedures are promoted only after their checks pass;
- Git-memory projections rebuild deterministically from declared reachable history, preserve conflicts, and report incomplete history coverage;
- memory-enabled agents repeat fewer failed approaches and use less stale knowledge than source-only and naive text-memory baselines;
- the core remains useful offline without an LLM or vector service.

See the [roadmap](roadmap.md) for milestone gates and the [architecture](architecture.md) for the system designed to provide these guarantees.
