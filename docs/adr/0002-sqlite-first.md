# ADR-0002: Use SQLite as the local-first storage engine

- Status: Accepted
- Date: 2026-07-22
- Owners: Project maintainers
- Scope: Local index, memory projection, tasks, evidence, and audits

## Context

RepoWitness must work for one developer and multiple related repositories without requiring database administration or a network service. Its core workload combines transactional generations, structured graph relationships, full-text search, memory lifecycle queries, and append-only audits.

The project may later need a centralized multi-user server, but those requirements have not been demonstrated. Designing a generic backend interface now would risk weakening SQLite usage and predicting PostgreSQL behavior from assumptions.

## Decision

Use SQLite through `rusqlite` as the reference and required storage engine for local and team-Git profiles.

- Use one database per connected workspace; a workspace may contain one or many related repositories.
- Store the database under the platform's per-user state directory by default.
- Use WAL mode, bounded readers, and one dedicated transactional writer.
- Require a shipped or verified SQLite build containing the WAL-reset fix: SQLite 3.51.3 or newer, or an explicitly documented fixed backport.
- Configure busy timeout and checkpoint policy explicitly; keep read transactions short and observe WAL/checkpoint behavior.
- Build immutable staging generations and activate them atomically.
- Use FTS5 for initial lexical search.
- Represent bounded graph traversal through relational tables and deterministic SQL.
- Keep current Git-tracked team memory canonical. SQLite projects reachable Git state and also retains previously observed unreachable versions locally; those historical observations require backup/export if they must survive database loss.
- Use SQLite's online backup API for live backups; do not copy only the main database file while WAL is active.
- Never place the live database on a shared network filesystem.
- Do not add a separate graph engine, vector database, or search engine without a measured bottleneck.

Domain services use narrow internal repository APIs and do not expose `rusqlite` types. Those APIs are not a stable public storage SPI.

## Alternatives considered

### PostgreSQL from the start

PostgreSQL is appropriate for concurrent writers, centralized permissions, and multi-user operations. It would impose a service dependency on the local product before those requirements exist.

### Embedded graph database

A graph database may make some traversals natural but adds another persistence model, migration surface, consistency boundary, and distribution dependency. Bounded traversals should first prove SQLite insufficient.

### Separate search/vector stores

They may improve selected workloads, but maintaining several indexes creates synchronization and operational costs. FTS5 plus structural ranking is the baseline.

## Consequences

### Positive

- Zero required external infrastructure.
- Transactional generation activation and straightforward backup/rebuild behavior.
- One local artifact per workspace with mature diagnostics and tooling.
- A credible path for individual users and multiple repositories.

### Negative and risks

- SQLite supports one writer at a time and is unsuitable as a live shared-network database.
- Long-running readers can starve checkpoints and grow the WAL; an unfixed 2026 SQLite WAL-reset race could corrupt a multi-connection database.
- Large monorepos may expose FTS, index-size, or traversal limits.
- A later server backend will require explicit concurrency, authorization, migration, and search decisions.

## Validation and revisit conditions

Measure full and incremental indexing, writer contention, P50/P95/P99 queries, database size, peak memory, WAL size, checkpoint latency/starvation, crash recovery, backup/restore, and multi-repository traversal. `doctor` reports SQLite version, compile options, journal mode, integrity status, and unsafe filesystem placement.

Prototype PostgreSQL only when design partners need centralized concurrent access, permissions, or operations Git plus local projections cannot provide. Extract a shared domain behavior suite from working SQLite and PostgreSQL implementations; keep backend-specific search and concurrency behavior separate.

## Implementation status

Implemented for the local source index through accepted ADR-0012 and the
accepted compatible SQLite schema version 3. Owned connections, WAL
configuration, immutable generations, atomic activation, bounded FTS5,
checkpoints, recovery, and validated online backup have production adapters
and regression tests. Version 3 adds the storage and owned-writer foundation
for bounded connected-workspace source slots, atomic immutable views,
generation-pinned Rust syntax graphs, and explicit bounded retention.
Read-only `doctor`, connected-workspace composition, and retention plan/apply
adapters are implemented under proposed Phase 1 ADRs. Their release evidence
and ratification, plus any PostgreSQL prototype, remain.

## Supersession

None.
