# ADR-0061: Reload the bounded local MCP catalog at request boundaries

- Status: Accepted
- Date: 2026-08-12
- Owners: Project maintainers
- Scope: Local catalog MCP lifecycle and repository selection

## Context

The local catalog is intentionally the single onboarding-backed source of
truth for one MCP connection across independently indexed repositories.
Requiring a process restart after every successful `onboard` makes a long-lived
coding-agent session stale and is unnecessary: onboarding publishes one small,
bounded JSON control file atomically, while each repository service remains
independently generation-pinned.

The existing catalog authority boundary must remain unchanged. MCP must not
scan directories, accept roots or databases from tool input, mutate catalog
membership, or replace a valid service set with a partial or malformed read.

## Decision

Catalog MCP loads the existing bounded `mcp-catalog-v1.json` before startup and
again at each MCP request boundary. A reload reads and validates the complete
control file, constructs the complete bounded service map, and atomically swaps
the catalog snapshot only after successful admission. A failed later reload
returns a generic MCP error for that request and preserves the last valid
snapshot. In-flight requests retain their already selected service snapshot.

`tools/list` regenerates repository selector schemas from the current snapshot.
The server accepts exact registered identities from tool input even when a
client has not refreshed its cached schema; no path or database is exposed.
Single-repository MCP is unchanged and never reloads a catalog.

Catalog startup remains read-only by default. When startup also supplies the
explicit fixed-actor memory-write capability, each admitted repository service
receives that actor and exposes `memory_manage`; the catalog file and
membership remain read-only.

Reload work is bounded by the existing catalog byte and repository-count
limits. It uses a serialized blocking file-read task and does not start a
watcher, daemon, timer, scanner, or background indexer.

## Alternatives considered

### Require an MCP restart

This is simpler, but makes onboarding changes invisible to a long-lived agent
session and creates avoidable setup work.

### Add a background watcher or daemon

This would add lifecycle, cancellation, and resource-management complexity for
a 64 KiB control file. Request-boundary reads are sufficient for the current
local development workflow.

### Add a catalog mutation or refresh MCP tool

Catalog membership remains an explicit CLI/onboarding operation. A tool would
blur the read-only MCP boundary and still require a refresh mechanism.

## Consequences

### Positive

- Adding or removing an onboarded repository takes effect without restarting
  MCP.
- Invalid, partial, or unavailable later reads cannot discard a valid catalog.
- Repository service isolation, path admission, generation pinning, and the
  existing bounded FTI behavior remain unchanged.

### Negative and risks

- Each catalog MCP request performs one small bounded control-file reload.
- Clients that cache `tools/list` must request the list again to see newly
  enumerated repository identities; calls with an exact identity are accepted
  after the reload even before schema refresh.
- A catalog update may make a repository disappear between requests; the next
  request fails categorically instead of retargeting another repository.

## Validation

- In-process MCP tests prove a new repository and default selection are visible
  after reload without restarting the server.
- Tests prove a failed reload preserves the prior valid snapshot.
- Existing bounded catalog admission, routing, cross-repository search,
  cancellation, and full workspace validation remain green.

## Follow-up

Add a list-change notification or background file observation only if real MCP
clients fail to refresh `tools/list` when they need newly onboarded identities.

## Supersession

This supersedes the process-restart-only lifecycle in proposed
[ADR-0049](0049-local-multi-repository-mcp-registry.md) and the catalog reload
deferral in [ADR-0050](0050-opt-in-codex-catalog-onboarding.md). It does not
introduce the daemon proposed by ADR-0056.
