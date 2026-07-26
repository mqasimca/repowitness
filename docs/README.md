# RepoWitness documentation

This directory turns the detailed research in [`plan.md`](../plan.md) into smaller documents that can evolve with implementation.

## Documentation map

| Document | Purpose | Current status |
|---|---|---|
| [Product](product.md) | Problem, users, differentiation, scope, success signals, and current implementation boundary | Draft |
| [Architecture](architecture.md) | System boundaries, data model, invariants, deployment profiles, and implemented Phase 0 path | Proposed |
| [Engineering standard](engineering.md) | Rust, testing, security, CI, and release expectations | Draft |
| [Roadmap](roadmap.md) | Delivery sequence and milestone exit criteria | Proposed |
| [Glossary](glossary.md) | Shared terminology used by protocols, storage, and documentation | Draft |
| [Architecture decisions](adr/README.md) | Accepted decisions governing the current implementation direction | Accepted |
| [Versioned schemas](schemas/README.md) | Concrete persistence and boundary encodings governed by accepted decisions | Implemented |
| [Architecture research (2026-07-22)](research/architecture-2026-07-22.md) | Primary-source review, recommended runtime/storage boundaries, alternatives, and Phase 0 spikes | Recommendation |
| [Path-identity research (2026-07-23)](research/path-identity-2026-07-23.md) | Git byte identity, Rust host paths, cross-platform conversion, and contained filesystem access | Implemented |
| [Repository-path encoding research (2026-07-23)](research/repository-path-boundary-encoding-2026-07-23.md) | Text-boundary alternatives, canonical Base16 profile, limits, and corpus size | Implemented |
| [Source identity research (2026-07-25)](research/source-identity-2026-07-25.md) | Explicit repository identity, Git/worktree receipts, and concurrent-mutation fencing | Implemented |
| [SQLite generation spike (2026-07-23)](research/sqlite-generation-spike-2026-07-23.md) | Bounded staging, activation, crash recovery, backup, direct-versus-RAM measurements, and production promotion record | Completed |
| [FTS5 retrieval spike (2026-07-25)](research/fts5-retrieval-spike-2026-07-25.md) | Generation-scoped deterministic lexical retrieval, literal-query admission, bounds, projection rebuild, and production promotion | Implemented |
| [Phase 0 dependency review (2026-07-25)](research/phase0-dependency-review-2026-07-25.md) | Need, versions, features, licenses, native/build surfaces, and verification for production dependencies | Active |
| [Phase 0 preparation benchmark (2026-07-25)](research/phase0-preparation-benchmark-2026-07-25.md) | Pinned mini-redis environment, cold/warm local-preparation timings, resource use, correctness, and limitations | Provisional measurement |
| [Strict memory YAML spike (2026-07-25)](research/strict-memory-yaml-spike-2026-07-25.md) | Bounded hostile-YAML admission, canonical JSON, semantic hashing, and candidate dependency findings | Recommendation |
| [Benchmark manifests](../benchmarks/README.md) | Pinned corpora, tasks, change scenarios, baselines, environments, and budgets | Proposed |
| [Research plan](../plan.md) | Full research, alternatives, sources, and detailed backlog | Reference |

## Implementation snapshot

As of 2026-07-26, RepoWitness implements and tests the local Rust
source-to-evidence path: bounded Git discovery and contained reads, canonical
source and artifact identity, deterministic Rust extraction and reuse,
immutable SQLite generations, FTS5 retrieval, exact declaration retrieval,
CLI commands, and local stdio MCP tools. The
[product](product.md), [architecture](architecture.md), and
[roadmap](roadmap.md) distinguish that completed foundation from the remaining
memory, correspondence, revalidation, context-compiler, and release-gate work.
The complete command surface and local Codex setup are in the
[repository README](../README.md).

## Authority and precedence

When documents disagree, use this order:

1. an accepted ADR controls the specific decision it covers;
2. the focused product, architecture, engineering, and roadmap documents control their respective areas;
3. dated research reports provide evidence and rationale for proposed decisions;
4. `plan.md` provides broader product research context and historical rationale.

Code and executable tests are the strongest statement of implemented behavior. A mismatch between code and documentation is a defect; it does not silently amend an accepted decision.

## Status vocabulary

- **Draft:** useful working direction that is expected to change.
- **Active:** an enforced living standard or review that evolves with the
  implementation.
- **Proposed:** complete enough for focused review but not yet an accepted commitment.
- **Accepted:** the current decision; changes require a superseding ADR.
- **Implemented:** accepted and enforced by shipped code or automated tests.
- **Implemented and promoted:** a research recommendation adopted by a
  controlling ADR or focused implementation contract.
- **Completed:** a bounded spike or research task finished and retained as an
  evidence record.
- **Provisional measurement:** reproducible observed data whose budgets have
  not been ratified.
- **Superseded:** retained for history and linked to its replacement.
- **Recommendation:** dated research advice that remains non-binding until adopted by a focused document or ADR.
- **Reference:** background or historical rationale that is not the controlling contract.
- **Mixed:** an index or collection containing entries with more than one status; not a decision status itself.

An accepted ADR may still define validation gates and explicit revisit conditions. Those conditions belong in the ADR body; they do not create an additional status value.

## Updating documentation

- Keep documents scoped. Put decision rationale in an ADR rather than growing every guide indefinitely.
- Link to a concept instead of copying its definition into several files.
- Include revision-sensitive examples and state assumptions explicitly.
- Update roadmap and success gates in the same change that materially changes scope.
- Prefer diagrams only when they clarify relationships that prose would obscure.
