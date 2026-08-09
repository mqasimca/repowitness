# Architecture decision records

Architecture decision records preserve the context, choice, consequences, and validation for decisions that shape RepoWitness.

The current supporting evidence and unresolved library spikes are summarized in
the dated [architecture research report](../research/architecture-2026-07-22.md).
Repository-path findings are recorded separately in the
[path-identity research report](../research/path-identity-2026-07-23.md).
Its textual-boundary follow-up is recorded in the
[repository-path encoding research](../research/repository-path-boundary-encoding-2026-07-23.md).
Phase 0 source-state identity findings are recorded in the
[source-identity research](../research/source-identity-2026-07-25.md).

ADRs 0010 through 0021 and ADRs 0022 through 0024 are
implemented by the current indexing, generation-publication,
evidence-retrieval, memory-journal, context, and parser slice. ADR-0014's
domain model, strict parser, canonicalizer, and deterministic writer are also
implemented.

ADR-0018 implements projection, Rust correspondence, memory revalidation, and
recall paths. ADR-0021 implements canonical writes, observation-only Git-history
import, separate local approval, correspondence review, CLI management, and
opt-in MCP mutation. Their
persistence tables are rooted in the immutable SQLite baseline accepted by
ADR-0022 and upgraded through the compatible parser-diagnostic migration
accepted by ADR-0024.

Phase 3 begins from the accepted scoped-memory, historical-correspondence, and
application-owned task contracts in ADR-0038 through ADR-0040. They retain the
strict version-1 team record and extend the local-first system through
compatible migrations and explicit capability negotiation.

The
[Phase 0 ratification review](../research/phase0-ratification-review-2026-07-28.md)
recommended accepting ADR-0017, ADR-0019, and ADR-0023. Maintainers adopted
those recommendations after the clean release-platform benchmark passed and
ratified its budgets. The first
[privacy-reviewed real-task outcome](../research/phase0-design-partner-evaluation-2026-07-30.md)
was correct and useful but did not meet the material-decision-change gate. The
[second outcome](../research/phase0-design-partner-evaluation-2026-07-30-task-02.md)
passed, so maintainers accepted ADR-0018 and then ADR-0021.

## Index

| ADR | Decision | Status |
|---|---|---|
| [0001](0001-rust-core.md) | Use Rust for the local engine, CLI, and MCP server | Accepted |
| [0002](0002-sqlite-first.md) | Use SQLite as the local-first storage engine | Accepted |
| [0003](0003-git-native-team-memory.md) | Store initial shared team memory in Git | Accepted |
| [0004](0004-logical-symbol-identity.md) | Separate durable symbols, occurrences, and correspondence | Accepted |
| [0005](0005-git-dag-temporal-memory.md) | Model project-valid and system-recorded time explicitly | Accepted |
| [0006](0006-immutable-index-generations.md) | Publish indexes through immutable generations | Accepted |
| [0007](0007-git-memory-synchronization.md) | Use canonical, versioned, conflict-preserving Git-memory synchronization | Accepted |
| [0008](0008-layered-modular-monolith.md) | Start as a layered modular monolith with inward dependencies | Accepted |
| [0009](0009-mit-license-and-clean-room-contributions.md) | Use the MIT License and clean-room contribution rules | Accepted |
| [0010](0010-repository-path-identity.md) | Separate repository path identity from filesystem authorization | Accepted |
| [0011](0011-repository-path-text-encoding.md) | Encode repository paths as tagged uppercase Base16 at text boundaries | Accepted |
| [0012](0012-phase0-sqlite-schema-and-ownership.md) | Use a versioned immutable-generation SQLite schema and owned connections | Accepted |
| [0013](0013-phase0-repository-and-source-state-identity.md) | Require explicit repository identity and canonical Git/worktree receipts | Accepted |
| [0014](0014-phase0-engineering-memory-record.md) | Define a strict Phase 0 engineering-memory record | Accepted |
| [0015](0015-phase0-go-and-rust-indexing.md) | Index Go and Rust in one Phase 0 generation | Accepted |
| [0016](0016-phase0-typescript-and-tsx-indexing.md) | Add TypeScript and TSX to the Phase 0 generation | Accepted |
| [0017](0017-phase0-memory-journal.md) | Persist an append-only Phase 0 memory journal in SQLite | Accepted |
| [0018](0018-phase0-memory-revalidation.md) | Revalidate Phase 0 Rust memory through precision-first correspondence | Accepted |
| [0019](0019-phase0-context-compilation-and-diagnostics.md) | Compile bounded Phase 0 context from exact source and current memory | Accepted |
| [0020](0020-phase0-python-indexing.md) | Add Python to the Phase 0 generation | Accepted |
| [0021](0021-phase0-memory-management-and-review.md) | Complete Phase 0 memory management through explicit local trust | Accepted |
| [0022](0022-squash-pre-release-sqlite-schema.md) | Squash the pre-release SQLite chain into one baseline | Accepted |
| [0023](0023-vendor-typescript-grammar-fix.md) | Vendor a reviewed TypeScript grammar fix | Accepted |
| [0024](0024-persist-parser-diagnostics-migration.md) | Persist recognized parser diagnostics through migration 2 | Accepted |
| [0025](0025-versioned-local-configuration-and-policy.md) | Resolve versioned local configuration with monotonic policy | Accepted |
| [0026](0026-connected-workspace-source-slots-and-views.md) | Model connected workspaces through source slots and immutable views | Accepted |
| [0027](0027-phase1-rust-syntax-graph.md) | Publish a bounded Rust syntax graph with explicit resolution coverage | Accepted |
| [0028](0028-reconciliation-watching-and-source-epochs.md) | Reconcile watched source through complete state and durable epochs | Accepted |
| [0029](0029-bounded-generation-retention-and-garbage-collection.md) | Collect unreachable generations through bounded mark-and-sweep | Accepted |
| [0030](0030-bounded-incumbent-mcp-compatibility.md) | Offer bounded incumbent-compatible MCP aliases | Proposed |
| [0031](0031-source-slot-selectors-and-package-scopes.md) | Resolve source-slot selectors in caller-provided worktrees | Accepted |
| [0032](0032-explicit-connected-workspace-manifest.md) | Admit connected workspaces through an explicit manifest | Accepted |
| [0033](0033-bounded-mutation-outcome-resolution.md) | Resolve mutation outcomes without denying committed work | Accepted |
| [0034](0034-phase1-codex-graph-evaluation.md) | Evaluate bounded Phase 1 graph packets through an evidence envelope | Accepted |
| [0035](0035-phase2-scip-precision-overlay.md) | Import SCIP as a bounded precision overlay | Accepted |
| [0036](0036-phase2-context-ranking-profiles.md) | Compile Phase 2 context through named evidence-ranking profiles | Accepted |
| [0037](0037-phase2-scip-overlay-source-slot-scope.md) | Scope each SCIP overlay to one source slot in a pinned workspace view | Accepted |
| [0038](0038-phase3-memory-scopes-and-kinds.md) | Separate Phase 3 team, personal, and archival memory | Accepted |
| [0039](0039-phase3-historical-correspondence.md) | Preserve reviewed multi-parent and historical correspondence | Accepted |
| [0040](0040-phase3-task-checkpoints-and-verification.md) | Keep task checkpoints application-owned and project them through MCP Tasks | Accepted |
| [0041](0041-bounded-multi-language-architecture-map.md) | Map exact multi-language indexed source files without relationship inference | Proposed |
| [0042](0042-evidence-backed-agent-code-discovery.md) | Add bounded multi-language agent discovery and raw syntax-site navigation | Proposed |
| [0043](0043-bounded-repository-topology-inventory.md) | Inventory bounded non-source repository topology without semantic inference | Proposed |
| [0044](0044-explicit-private-local-onboarding.md) | Onboard one explicit repository into private local state | Proposed |
| [0045](0045-exact-scip-declaration-receipts.md) | Resolve exact declaration receipts to SCIP symbols | Proposed |
| [0046](0046-bounded-lexical-path-navigation.md) | Group bounded lexical declaration evidence into canonical paths | Proposed |
| [0047](0047-bounded-raw-syntax-target-search.md) | Navigate exact all-language raw syntax targets without relationship inference | Proposed |
| [0048](0048-bounded-scip-relationship-traversal.md) | Traverse validated SCIP relationships through a bounded precision profile | Proposed |
| [0049](0049-local-multi-repository-mcp-registry.md) | Serve explicitly registered local repositories through one read-only MCP process | Proposed |
| [0050](0050-opt-in-codex-catalog-onboarding.md) | Admit the current Codex worktree into a private local MCP catalog | Proposed |
| [0051](0051-explicit-codex-connected-workspace-catalog.md) | Compose explicitly declared Codex product stacks through connected source slots | Proposed |
| [0052](0052-explicit-rust-scip-producer-import.md) | Produce and import one Rust SCIP overlay only through explicit local execution | Proposed |
| [0053](0053-explicit-go-scip-producer-import.md) | Produce and import one Go SCIP overlay only through explicit local execution | Proposed |
| [0054](0054-bounded-revision-pinned-change-receipts.md) | Produce bounded revision-pinned change receipts for review | Proposed |
| [0055](0055-canonical-evidence-balanced-context-command.md) | Use one canonical evidence-balanced context builder | Proposed |

Use [0000-template.md](0000-template.md) for a new decision.

## Process

1. Copy the template and allocate the next four-digit number.
2. Open the ADR as `Proposed` before implementation depends on it.
3. Record alternatives and negative consequences, not only the preferred design.
4. Link fixtures, benchmarks, security analysis, and implementation changes.
5. Change status through review. Do not rewrite the rationale of an accepted ADR to make history look cleaner.
6. Replace an accepted decision with a new ADR marked as superseding the old one.

Accepted ADRs take precedence over broader architecture and planning documents for the decision they cover.
