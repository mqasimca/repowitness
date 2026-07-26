# ADR-0003: Store initial team memory in Git

- Status: Accepted
- Date: 2026-07-22
- Owners: Project maintainers
- Scope: Shared engineering memory in local and team-Git profiles

## Context

Teams need engineering memory that is reviewable, portable, branch-aware, and owned with their source. A database-only shared store would either require a server or make team knowledge difficult to review and version. Raw chat logs are not an acceptable source of team truth.

## Decision

For the local product, store canonical shared memory in the application repository under `.code-memory/records/<id>.yaml`.

- One current canonical file represents each immutable record ID.
- Pull requests review memory changes alongside source changes.
- Reachable Git history supplies portable shared history for record versions.
- SQLite materializes append-only memory versions and audit events for querying and locally retains previously observed versions according to policy.
- Personal memory remains outside the repository in a local, optionally encrypted store.
- Human or policy approval is required for active decisions, policies, and procedures.
- Repository text, inferred memory, and model-generated candidates are untrusted until validation and policy permit activation.
- A separate organization policy repository is deferred until centralized use is demonstrated.

The detailed serialization, concurrency, import, tombstone, and conflict rules
are covered by accepted
[ADR-0007](0007-git-memory-synchronization.md). Their production implementation
still depends on the Phase 0 record decision proposed in
[ADR-0014](0014-phase0-engineering-memory-record.md).

## Alternatives considered

### SQLite-only team memory

This is simple locally but does not provide a safe shared transport or normal code-review workflow. Sharing the live SQLite file over sync or network storage is explicitly unsupported.

### Hosted PostgreSQL memory

This offers centralized policy and concurrency but makes the local product depend on service operation, accounts, and network access.

### Separate policy repository from the first release

This can centralize organization-wide decisions but complicates revision alignment, permissions, checkout behavior, and onboarding before local value is proven.

### Commit raw agent conversations

Conversations contain noise, secrets, prompt-injection content, unverified conclusions, and excessive personal context. Only structured, scoped, evidence-linked records are eligible for shared memory.

## Consequences

### Positive

- Teams own and review memory with existing Git workflows.
- Branch and revision history provide useful temporal context.
- Shared knowledge remains portable and offline.
- Current state can be rebuilt reproducibly from declared reachable Git history and current files, with explicit coverage. Previously observed versions from unreachable commits require a retained database backup/export and are not covered by that rebuild promise.

### Negative and risks

- Large record counts may create repository and checkout overhead.
- Concurrent branches can produce semantic conflicts requiring review.
- Rebase, force-push, shallow-clone, and history-rewrite behavior must be explicit.
- Repository permissions do not automatically prove the identity claimed inside a record.
- Secrets require scanning before promotion and commit.

## Validation

- Round-trip canonical serialization without semantic changes.
- Idempotent projection rebuild from a declared set of reachable Git refs and current files, with explicit history coverage.
- Backup/export restoration for locally retained versions whose Git objects are no longer reachable.
- Concurrent edit and conflict fixtures.
- Tombstone and rewritten-history behavior.
- Branch/worktree scope isolation.
- Secret, actor, provenance, schema, and approval-policy validation.
- Repository-size and checkout measurements at realistic record counts.

## Supersession

None.
