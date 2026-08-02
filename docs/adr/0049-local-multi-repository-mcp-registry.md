# ADR-0049: Serve an explicit local multi-repository MCP registry

- Status: Proposed
- Date: 2026-08-02
- Owners: Project maintainers
- Scope: Local stdio MCP startup, repository selection, and registry admission

## Context

The existing `mcp-serve` process deliberately fixes one repository identity,
source root, database, and optional connected-workspace source slot at startup.
That is safe but requires a coding-agent client to register a separate MCP
server for every independently indexed repository. A single local connection
is more usable when an operator intentionally works across several repositories.

The existing connected-workspace manifest is not an MCP connection catalogue:
it atomically publishes one connected workspace view and has different source
slot, persistence, and generation semantics. Treating an arbitrary caller
path, database, repository identity, or source slot as a per-tool selector
would weaken the current startup authority boundary and could mix receipts
from separate repositories.

## Decision

Add a separate local `mcp-serve --registry <path>` mode with a strict
version-1 JSON registry. Each entry contains one canonical `rwi1:h:` repository
identity plus one absolute UTF-8 source root and one absolute UTF-8 database
path. The registry is admitted as a bounded, no-follow regular control file;
it has at most 32 entries, no unknown fields, no duplicate repository IDs, and
no duplicate textual root or database paths. It is read once at process startup
and never reloaded.

Registry mode starts one local stdio MCP process with the canonical 24
read-only tools. Every tool input must include `repository_id`, which must be
one exact registry identity. The server removes that selector before validating
the native tool request and routes only to the matching fixed local service.
The selector schema enumerates the registered opaque identities but never
returns registry paths or databases. Missing, non-string, or unknown selectors
fail before repository work; there is no default repository and no ambient
current-directory discovery.

Version 1 supports independently indexed repositories only. It does not use
connected-workspace source slots, cross-repository queries, shared-database
workspace selection, registry mutation/reload, remote transport, or a general
repository/storage abstraction. It also refuses incumbent-compatible aliases,
memory writes, personal memory, native MCP Tasks, repository configuration, and source-slot
startup options. Explicit user and workspace configuration may still tighten
the one process-wide policy and limits.

The existing single-repository startup contract is unchanged: its input schema
does not gain `repository_id`, and all authority remains fixed at startup.

## Alternatives considered

### Register one single-repository server per repository

This is the existing safe option and remains suitable for one repository. It
becomes hard to manage when the same agent must orient across many independent
repositories, and it makes repository selection an external client concern.

### Reuse a connected-workspace manifest as the MCP registry

Connected workspaces model atomic multi-source indexing and immutable shared
views. A registry maps already-indexed, independently administered local
repositories. Combining the two would blur source-slot identity, publication,
and query scope.

### Accept roots, databases, or source slots in each tool request

This would turn untrusted tool input into local filesystem authority and makes
per-request isolation, policy, and generation guarantees substantially harder
to audit. The registry exposes only pre-admitted opaque identities.

### Add a remote catalog or team service

That requires authentication, tenant isolation, retention, and remote MCP
threat modeling. It belongs to the demand-gated server phase, not this local
stdio capability.

## Consequences

### Positive

- One agent configuration can intentionally access several local repositories.
- Repository selection is explicit, deterministic, and visible in every tool
  schema without exposing host paths.
- Existing repository services and their generation/evidence contracts remain
  isolated; no generic storage layer or cross-repository result is introduced.
- Registry changes require an explicit server restart, avoiding mutable
  process-lifetime authority.

### Negative and risks

- Version 1 accepts only absolute UTF-8 host paths and cannot represent a
  non-UTF-8 root.
- The operator must keep the registry and each indexed database current.
- A process-wide configuration cannot express per-entry preferences or policy.
- `tools/list` exposes registered opaque identities to the connected local MCP
  client, which is necessary for explicit selection.

## Validation

- Unit-test strict registry parsing: empty/oversize/changed files, unknown and
  duplicate fields, invalid identities, relative or non-UTF-8 paths, empty and
  over-limit registries, and duplicate identities/root/database paths.
- Verify tool schemas require and enumerate `repository_id` only in registry
  mode; the unchanged single-repository schemas reject it.
- Route two distinct fake services through the same in-process MCP server and
  prove missing, malformed, and unknown selections invoke neither service.
- Run an installed-binary stdio round-trip against two temporary indexed
  repositories, asserting exact routing, no default selection, no path output,
  and unchanged single-repository behavior.
- Exercise malformed registry startup before the Tokio runtime starts and
  confirm it emits only a stable path-free diagnostic.

## Follow-up

- Implement the strict registry reader and startup grammar in the CLI
  composition root.
- Add MCP routing/schema support without leaking transport selection into
  application or domain request types.
- Record registry-mode limitations in product, architecture, engineering,
  roadmap, schema, and user documentation.
- Consider a future version only after measured demand for per-entry policy,
  connected-workspace selection, or cross-repository queries; that version
  needs its own ADR and isolation fixtures.

## Supersession

None. This complements, but does not supersede, ADR-0026, ADR-0031, or
ADR-0032.
