# ADR-0044: Explicit private local onboarding

- Status: Proposed
- Date: 2026-08-01
- Owners: Project maintainers
- Scope: CLI onboarding, local state-directory ownership, generated repository
  identity, database placement, and MCP handoff guidance

## Context

RepoWitness currently requires callers to supply a repository identity, a
database path, and a root before indexing or serving MCP. That is precise and
safe, but it adds friction for coding agents compared with an automatically
indexed code-graph service. Ambient discovery of sibling, parent, remote, or
home-directory repositories would broaden authority, leak host layout, and
create unexpected local writes.

## Decision

Add an explicit `onboard` CLI use case after the discovery read paths are
implemented.

- `repowitness onboard --root <path> [--state-dir <path>]
  [--repository-id <id>]` requires one explicit root and never searches parent
  or sibling repositories.
- It generates a cryptographically random repository identity only when the
  caller does not provide a canonical identity. It never derives identity from
  a path, remote, Git object, or source content.
- It creates or opens a database under a private user-state directory named by
  the opaque identity, invokes the existing bounded indexing use case, and
  returns only opaque identity, result receipt, generation, and the documented
  state-directory convention.
- It validates the explicit root through the normal repository inspector before
  creating any state. On Unix it creates and opens the state path through
  no-follow directory capabilities and requires non-group/non-world-writable
  ancestors; on platforms without an equivalent private ACL implementation it
  fails closed rather than claiming private state.
- It does not write the repository, source tree, repository configuration,
  Codex configuration, or a global registry of raw roots. The database remains
  outside the indexed worktree and retains the existing mutation lease,
  no-follow, hard-link, and final-fence protections.
- MCP remains read-only by default. Onboarding does not add a mutating MCP
  `index_repository` tool or grant any startup capability implicitly.

## Alternatives considered

### Derive identity from remote/path/commit

Rejected. It changes the caller-controlled logical identity contract and can
link repositories unexpectedly.

### Automatically find repositories around the working directory

Rejected. It broadens filesystem authority and leaks layout; explicit connected
workspaces already provide an intentional multi-repository boundary.

### Add default MCP indexing

Rejected. Indexing creates durable state and needs a separately authorized
mutation capability.

## Consequences

### Positive

- Agents can start safely with one root argument.
- Existing index, evidence, and database safety paths remain authoritative.
- No host topology or source content needs a global registry.

### Negative and risks

- State-directory ownership and identity lifecycle become a maintained CLI
  contract.
- Users who want several clones to share an identity must explicitly supply it.

## Validation

- Synthetic onboarding fixtures prove explicit-root-only behavior, random and
  caller-supplied identity handling, idempotency, private state permissions,
  database-outside-root enforcement, no repository mutation, interruption,
  reindex/reopen, and path/identity redaction.
- CLI help and invalid-input tests prove no I/O before validation; installed
  contracts prove the returned database can serve the expected read-only MCP
  surface.

## Follow-up

Implement only after the ADR-0042 and ADR-0043 read paths prove useful. A
future persistent root registry requires its own authorization and privacy ADR.

## Supersession

None.
