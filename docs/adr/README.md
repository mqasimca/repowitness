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

ADRs 0010 through 0013, 0015, 0016, 0020, and 0022 are implemented by the current
indexing, generation-publication, and evidence-retrieval slice. ADR-0014 is
accepted and its domain model, strict parser, canonicalizer, and deterministic
writer are implemented. The worktree admission, trusted import, and
append-only SQLite journal described by proposed ADR-0017 are also
implemented. Proposed ADR-0018 and ADR-0019 now have implemented projection,
Rust correspondence, memory revalidation/recall, bounded context compilation,
and diagnostics paths. Proposed ADR-0021 has implemented canonical writes,
observation-only Git-history import, separate local approval, correspondence
review, CLI management, and opt-in MCP mutation. Its persistence tables are now
part of the one-migration SQLite baseline accepted by ADR-0022. All four
review statuses remain proposed; their residual release matrices, explicit
ratification, and the comparative design-partner evaluation remain.

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
| [0017](0017-phase0-memory-journal.md) | Persist an append-only Phase 0 memory journal in SQLite | Proposed |
| [0018](0018-phase0-memory-revalidation.md) | Revalidate Phase 0 Rust memory through precision-first correspondence | Proposed |
| [0019](0019-phase0-context-compilation-and-diagnostics.md) | Compile bounded Phase 0 context from exact source and current memory | Proposed |
| [0020](0020-phase0-python-indexing.md) | Add Python to the Phase 0 generation | Accepted |
| [0021](0021-phase0-memory-management-and-review.md) | Complete Phase 0 memory management through explicit local trust | Proposed |
| [0022](0022-squash-pre-release-sqlite-schema.md) | Squash the pre-release SQLite chain into one baseline | Accepted |

Use [0000-template.md](0000-template.md) for a new decision.

## Process

1. Copy the template and allocate the next four-digit number.
2. Open the ADR as `Proposed` before implementation depends on it.
3. Record alternatives and negative consequences, not only the preferred design.
4. Link fixtures, benchmarks, security analysis, and implementation changes.
5. Change status through review. Do not rewrite the rationale of an accepted ADR to make history look cleaner.
6. Replace an accepted decision with a new ADR marked as superseding the old one.

Accepted ADRs take precedence over broader architecture and planning documents for the decision they cover.
