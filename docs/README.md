# RepoWitness documentation

This directory divides the detailed research in [`plan.md`](../plan.md) into
smaller documents. These documents can change with the implementation.

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
| [Phase 1 local-boundary dependency review (2026-07-29)](research/phase1-configuration-dependency-review-2026-07-29.md) | TOML admission, capability-based file access, stable file identity, and operating-system identity generation | Active |
| [Phase 2 SCIP decoder spike (2026-07-31)](research/phase2-scip-decoder-spike-2026-07-31.md) | Bounded decoder/dependency decision for the accepted SCIP precision overlay | Active |
| [Phase 2 local context evaluation (2026-07-31)](research/phase2-local-evaluation-2026-07-31.md) | Public synthetic and pinned-corpus context, stale-provider, latency, and opt-in Codex evidence | Completed local evidence |
| [Phase 0 preparation benchmark (2026-07-25)](research/phase0-preparation-benchmark-2026-07-25.md) | Pinned mini-redis environment, cold/warm local-preparation timings, resource use, correctness, and limitations | Provisional measurement |
| [Phase 0 product benchmark (2026-07-28)](research/phase0-product-benchmark-2026-07-28.md) | Pinned persistence, reuse, retrieval, MCP, memory-management, source-change revalidation, context, resource, and latency results | Provisional measurement |
| [Phase 0 clean benchmark attestation (2026-07-29)](research/phase0-clean-benchmark-attestation-2026-07-29.md) | Checksummed clean Ubuntu 24.04 benchmark evidence and budget ratification | Completed |
| [Phase 1 clean benchmark attestation (2026-07-30)](research/phase1-clean-benchmark-attestation-2026-07-30.md) | Historical checksummed clean Ubuntu 24.04 Phase 1 benchmark evidence | Completed |
| [Phase 1 portable-core validation (2026-07-30)](research/phase1-portable-core-validation-2026-07-30.md) | Exact-revision macOS 15 and Windows 2025 full-workspace CI evidence | Completed CI evidence |
| [Phase 1 release-platform attestation (2026-07-31)](research/phase1-release-platform-attestation-2026-07-31.md) | Checksummed Ubuntu 24.04 release benchmark for the current Phase 1 merge revision | Completed |
| [Phase 1 adversarial release matrix (2026-07-31)](research/phase1-adversarial-release-matrix-2026-07-31.md) | Bounded local release-mode evidence for 12 migration, recovery, fencing, configuration, mutation, compatibility, and shutdown cases | Completed local evidence |
| [Phase 1 Codex graph evaluation (2026-07-30)](research/phase1-codex-graph-evaluation-2026-07-30.md) | Repeated isolated evaluation of bounded MCP graph, source, and memory packets | Completed |
| [Phase 1 ratification review (2026-07-31)](research/phase1-ratification-review-2026-07-31.md) | Evidence supporting the accepted Phase 1 ADRs and budgets | Completed |
| [Phase 0 comparative evaluation (2026-07-28)](research/phase0-comparative-evaluation-2026-07-28.md) | Controlled before/after lexical, naive-memory, and evidence-backed memory comparison | Provisional measurement |
| [Phase 0 Codex utility evaluation (2026-07-28)](research/phase0-codex-utility-evaluation-2026-07-28.md) | Isolated before/after Codex decision, source grounding, memory use, stale-memory avoidance, and packet presentation | Provisional measurement |
| [Phase 0 design-partner evaluation outcome, task-01 (2026-07-30)](research/phase0-design-partner-evaluation-2026-07-30.md) | Privacy-reviewed categorical real-task outcome; correct and useful but no material decision change | Completed, non-passing outcome |
| [Phase 0 design-partner evaluation outcome, task-02 (2026-07-30)](research/phase0-design-partner-evaluation-2026-07-30-task-02.md) | Privacy-reviewed categorical real-task outcome; correct, useful, and materially decision-changing | Completed, passing outcome |
| [Phase 0 ratification review (2026-07-28)](research/phase0-ratification-review-2026-07-28.md) | Evidence-based ADR and benchmark-budget readiness recommendations | Recommendation |
| [Phase 0 design-partner evaluation protocol](research/phase0-design-partner-evaluation-protocol.md) | Privacy-preserving method and pass criteria used for the Phase 0 real-task gate | Completed |
| [Strict memory YAML spike (2026-07-25)](research/strict-memory-yaml-spike-2026-07-25.md) | Bounded hostile-YAML admission, canonical JSON, semantic hashing, and candidate dependency findings | Recommendation |
| [Benchmark manifests](../benchmarks/README.md) | Pinned corpora, tasks, change scenarios, baselines, environments, and budgets | Ratified |
| [Research plan](../plan.md) | Full research, alternatives, sources, and detailed backlog | Reference |

## Implementation snapshot

As of 2026-07-31 UTC, RepoWitness implements and tests the local path from
source change to revalidated context for five languages. It includes bounded Git
discovery, contained file reads, canonical source and artifact IDs, deterministic
extraction and reuse, immutable SQLite generations, Rust graph publication and
reads, retrieval, canonical memory management, observation-only Git history,
manual correspondence review, context compilation, CLI commands, and local
stdio MCP tools. MCP is read-only by default. An operator can authorize memory
writes explicitly.

The [product](product.md), [architecture](architecture.md), and
[roadmap](roadmap.md) identify the completed design-partner alpha, completed
Phase 1 local core, and deferred later phases. The adversarial release matrix
passes. The benchmark manifest and Phase 1 budgets have a checksummed clean
Ubuntu 24.04 release attestation. The
controlled public baseline comparison and isolated Codex utility evaluation are
reproducible. The first privacy-reviewed real-task outcome was correct and
useful but non-passing; the second passed the material-decision-change gate.
See the [repository README](../README.md) for all commands and local Codex setup
steps.

## Authority and precedence

When documents disagree, use this order:

1. An accepted ADR controls the decision that it covers.
2. The product, architecture, engineering, and roadmap documents control their
   own areas.
3. Dated research reports give evidence and rationale for proposed decisions.
4. `plan.md` gives broader product research and historical rationale.

Code and executable tests define implemented behavior. A difference between code
and documentation is a defect. It does not change an accepted decision.

## Status vocabulary

- **Draft:** Working direction that can change.
- **Active:** An enforced standard or review that changes with the
  implementation.
- **Proposed:** Ready for focused review, but not an accepted commitment.
- **Accepted:** The current decision. A change requires a superseding ADR.
- **Ratified:** A benchmark profile or budget set accepted after review and
  evidence. A material change requires a new review.
- **Implemented:** Accepted and enforced by released code or automated tests.
- **Implemented and promoted:** A research recommendation adopted by a
  controlling ADR or implementation contract.
- **Completed:** A bounded spike or research task that is complete and kept as
  evidence.
- **Provisional measurement:** Reproducible data with budgets that are not
  ratified.
- **Superseded:** Kept for history and linked to its replacement.
- **Recommendation:** Dated research advice. It is not binding until a focused
  document or ADR adopts it.
- **Reference:** Background or historical rationale. It is not the controlling
  contract.
- **Mixed:** An index or collection with entries that have different statuses.
  It is not a decision status.

An accepted ADR can define validation gates and review conditions. Put these
conditions in the ADR body. They do not create another status.

## Updating documentation

- Keep each document within its scope. Put decision rationale in an ADR.
- Link to a concept. Do not copy the same definition into several files.
- Include examples that identify their revision. State assumptions explicitly.
- Update the roadmap and success gates when a change modifies scope.
- Use a diagram only when prose cannot clearly show the relationship.
