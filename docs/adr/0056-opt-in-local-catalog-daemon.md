# ADR-0056: Offer an opt-in local catalog daemon

- Status: Proposed
- Date: 2026-08-09
- Owners: Project maintainers
- Scope: Local Codex catalog lifecycle, watching, and MCP transport composition

## Context

The opt-in catalog mode in ADR-0050 refreshes the process-current worktree
before each stdio MCP server starts. The unchanged path is fast, but a changed
worktree must prepare and atomically publish a complete new generation before
the client can connect. Long-lived coding sessions benefit from a separately
supervised local process that can reconcile after edits and retain a warm MCP
service without adding network transport or database infrastructure.

The accepted ADR-0028 watcher contract already requires complete manifest
reconciliation and final source fencing; filesystem events can only be hints.
Any daemon must preserve that contract, one-writer ownership, bounded work,
and immutable generation publication.

The implementation enables Tokio's existing `net` feature for Unix-domain
sockets. Its newly resolved transitive `socket2` 0.6.5 dependency is pinned in
the lockfile, licensed `MIT OR Apache-2.0`, and is used only by the local
transport runtime; it introduces no network listener.

## Decision

Add an explicitly started, local-only catalog daemon for one process-current
Git worktree. It owns that worktree's reconciliation supervisor and serves the
existing read-only catalog MCP surface over one private Unix-domain socket.
The daemon is foreground-supervised: a user or service manager owns process
restart and shutdown. It never backgrounds itself, listens on TCP, scans for
repositories, or accepts repository roots through MCP.

`mcp-serve --catalog --daemon` remains a local stdio proxy. It resolves only
its own current worktree, reads the pre-existing private catalog entry, derives
that exact entry's socket, and copies MCP bytes between stdio and the socket.
It does not index or create catalog state. A missing daemon or absent entry
fails before protocol startup. Ordinary `mcp-serve --catalog` retains its
existing one-shot admission and refresh behavior.

The first implementation is Linux-only. Its event backend supplies bounded
hints and always performs periodic complete reconciliation; source capture,
final fencing, cancellation, and atomic activation remain authoritative.
Sockets and control files live only below the existing private catalog-state
root and are not returned through MCP.

## Alternatives considered

### Poll the complete worktree from every MCP process

This remains the default and is simple, but repeatedly pays source-capture
cost during long-lived editing sessions.

### A shared global daemon that accepts arbitrary roots

This would turn a socket protocol into new ambient filesystem authority. A
per-worktree daemon retains the current-process worktree boundary.

### TCP or remote MCP

TCP introduces authentication, authorization, tenancy, retention, and remote
threat-model work. It is not required for one local user and remains deferred.

### PostgreSQL-backed service

The measured changed path is dominated by source and graph preparation, not
SQLite connection startup. PostgreSQL adds operations without improving the
local single-writer case.

## Consequences

### Positive

- Codex reconnects can attach to a warm local MCP service.
- Edit-triggered preparation happens outside the interactive connection path.
- SQLite remains local and one immutable generation remains active at a time.

### Negative and risks

- Daemon lifecycle, private socket handling, stale-socket recovery, and
  shutdown require explicit tests.
- The initial Linux-only implementation is unavailable on macOS and Windows
  until equivalent local capability boundaries and watcher backends are
  validated.
- A daemon consumes bounded background resources and must make its polling,
  debounce, periodic-reconciliation, and failure behavior observable.

## Validation

- Test absent, stale, regular-file, and active socket handling without
  deleting a non-socket path.
- Test proxy failure before stdio protocol startup when the daemon or catalog
  entry is absent.
- Test Unix-socket MCP initialization and an ordinary read tool through the
  stdio proxy.
- Test event loss, overflow, debounce, periodic reconciliation, cancellation,
  crash/restart, and clean-versus-watched equality under ADR-0028.
- Measure idle CPU, edit-to-publication latency, reconnect latency, memory,
  database growth, and shutdown behavior on the ratified benchmark corpus.

## Follow-up

- Add Windows-local transport and watcher support only after equivalent ACL,
  cancellation, and recovery behavior is tested.
- Consider installer/service-manager integration only after the opt-in daemon
  passes the resource and crash-recovery gate.

## Supersession

This narrows and supersedes ADR-0050's daemon deferral for the explicit local
per-worktree mode only. ADR-0050's non-daemon catalog behavior remains valid.
