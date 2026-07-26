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

ADRs 0010 through 0013 are implemented by the current Rust indexing,
generation-publication, and evidence-retrieval slice. ADR-0014 remains
proposed; its parser dependencies and record shape are not production
contracts until explicit review changes that status.

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
| [0014](0014-phase0-engineering-memory-record.md) | Define a strict Phase 0 engineering-memory record | Proposed |

Use [0000-template.md](0000-template.md) for a new decision.

## Process

1. Copy the template and allocate the next four-digit number.
2. Open the ADR as `Proposed` before implementation depends on it.
3. Record alternatives and negative consequences, not only the preferred design.
4. Link fixtures, benchmarks, security analysis, and implementation changes.
5. Change status through review. Do not rewrite the rationale of an accepted ADR to make history look cleaner.
6. Replace an accepted decision with a new ADR marked as superseding the old one.

Accepted ADRs take precedence over broader architecture and planning documents for the decision they cover.
