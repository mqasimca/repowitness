# Product

- Status: Draft
- Last reviewed: 2026-07-28

## Definition

RepoWitness is a local-first code-intelligence and engineering-memory engine for coding agents and developers.

> RepoWitness gives coding agents verified, revision-aware knowledge of how a project works, what has already been tried, and whether that knowledge is still valid.

Its trust promise is:

> Every retrieved fact explains where it came from, how precise it is, when it was true, and what could invalidate it.

## Problem

Coding agents repeatedly spend time and tokens rebuilding context. Across tasks, sessions, branches, and developers, they may:

- rediscover architecture and conventions;
- repeat approaches that already failed;
- trust documentation or memories that no longer match the active revision;
- retrieve many related files without finding the smallest useful working set;
- confuse a syntax-derived guess with a compiler-confirmed or runtime-observed fact;
- lose hypotheses, commands, diagnostics, and verification outcomes;
- treat generated summaries as truth without supporting evidence.

The central problem is not storage. It is deciding whether engineering knowledge is supported, applicable to the current code, and safe to use.

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
- Diagnose indexing and memory validity instead of debugging an opaque retrieval system.

### Team or platform owner

- Keep source and personal memory local by default.
- Define approval, sharing, retention, secret, and resource policies.
- Measure correctness, retrieval quality, latency, resource use, and scope isolation.
- Add centralized operation only when team demand justifies it.

## Differentiating loop

No single ingredient is unique by itself. The product is the integration of source intelligence, engineering experience, time, and validation:

```text
source change
    -> update code facts atomically
    -> identify affected memories, tasks, tests, and observations
    -> preserve, contradict, revalidate, or mark knowledge stale
    -> compile a new evidence-backed context pack
    -> record the next verified success or failure
```

This enables:

- proof-carrying retrieval with explicit coverage and limitations;
- memory that can follow supported refactors without guessing through ambiguity;
- bitemporal queries separating project validity from when knowledge was recorded;
- verified procedures and failures rather than unreviewed chat summaries;
- deterministic context compilation under a token budget;
- Git-reviewable team knowledge and local personal knowledge;
- progressive precision from syntax, SCIP/compiler evidence, human decisions, and optional runtime observations.

## Product principles

1. Current source is authoritative for source-derived facts.
2. Abstention is a feature; unresolved and skipped work must remain visible.
3. Evidence and coverage are part of every material result.
4. Stored memories are claims, not truth, until evidence and policy support them.
5. Time and scope are first-class: repository, revision, branch, worktree, path, symbol, user, and team.
6. Determinism comes before opaque relevance tuning.
7. Local operation requires no LLM, embedding service, hosted account, or network connection.
8. Customization cannot bypass provenance, audit, scope isolation, query limits, or atomic index generations.
9. Optional compiler and runtime evidence strengthens syntax coverage; it does not silently replace or hide it.
10. Measure downstream engineering outcomes, not MCP tool count or supported-language count.

## Phase 0 product slice

The first design-partner alpha proves one complete loop with one atomic Rust,
Go, TypeScript, TSX, and Python source profile:

1. Index any mixture of the five supported languages into SQLite.
2. Retrieve evidence and build a compact context pack.
3. Attach a manually approved decision or failure to a logical symbol.
4. Rename or meaningfully change the symbol.
5. Relink only when evidence is strong; otherwise request review or mark the memory stale.
6. Rebuild the context pack and expose the changed validity.
7. Compare a real task with lexical/source-only retrieval and with RepoWitness.

The alpha deliberately excludes languages beyond Rust, Go, TypeScript, TSX,
and Python, PostgreSQL, remote MCP, automatic memory extraction, persisted
tasks, runtime telemetry, UI, plugin execution, vector retrieval, raw ranking
weights, and general graph-query compatibility.

### Implemented and verified

As of 2026-07-28, the Phase 0 source-to-revalidated-context path and its local
memory-management foundation are implemented:

- bounded sanitized-Git discovery and canonical Git/worktree receipts;
- capability-contained, no-follow supported-language source reads with final
  identity and content revalidation;
- deterministic language-specific Tree-sitter symbol extraction, one mixed
  canonical manifest and source snapshot, independent Rust, Go, TypeScript,
  TSX, and Python artifact keys, and exact clean-versus-incremental reuse;
- owned SQLite connections, immutable generations, atomic activation,
  bounded/cancellable startup recovery, double-buffered FTS5, checkpoints, and
  validated online backup;
- evidence-bearing literal `code_search` and exact digest-verified
  `symbol_get` application use cases;
- `index`, `search`, `symbol-get`, `memory-manage`, `memory-revalidate`,
  `memory-recall`, `context-build`, `diagnostics`, `mcp-serve`, and
  `inspect-paths` CLI commands, with five read-only
  retrieval/context/diagnostic tools exposed by default over local stdio MCP
  and an explicitly enabled, fixed-actor `memory_manage` mutation tool;
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
- one clean SQLite baseline-version-1 migration containing occurrence
  fingerprints, immutable current-memory projection, idempotent manual
  correspondence review, precision-first Rust correspondence, Git-DAG/worktree validity,
  conflicts, categorical staleness and review states, bounded current-memory
  recall, and atomic projection activation;
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

The slice has been exercised on the pinned mini-redis product benchmark, this
workspace, temporary adversarial and mixed-language repositories, and
neighboring real repositories. The development benchmark passes every proposed
numeric ceiling, but these runs do not ratify the manifest or establish the
real design-partner outcome required by the Phase 0 exit criteria.

TypeScript and TSX are distinct syntax-only dialects. The implemented profile
does not evaluate TypeScript types, `tsconfig.json`, package/module resolution,
references, call sites, or active build targets, and it does not select
JavaScript or MJS files.

Python is a separate syntax-only language for case-sensitive `.py` and `.pyi`
paths. The implemented profile does not execute repository code, load Python
environments, resolve imports or types, evaluate decorators, infer dynamic
dispatch, or extract references and calls.

### Remaining product gates

The local product loop is implemented, but the Phase 0 release gate is not yet
ratified. Maintainers must accept, revise, or reject proposed ADR-0017 through
ADR-0019 and ADR-0021; finish the residual rewritten-history,
obsolete-snapshot, competing-review, and publication-fault matrix; rerun the
pinned benchmark from a clean exact RepoWitness revision; ratify or revise its
retrieval/resource budgets; and record a real design-partner task whose
engineering decision improves relative to the declared baselines. A stable
public API also remains deferred. No public-beta or production-readiness claim
follows from the implemented design-partner-alpha loop.

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
