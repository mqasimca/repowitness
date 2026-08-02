# Local MCP catalog version 1

This document defines the private control file used only by the proposed
[ADR-0050](../adr/0050-opt-in-codex-catalog-onboarding.md) `mcp-serve --catalog`
startup mode. It is not an MCP response, repository artifact, or user-editable
registry.

## Location and admission

The file is named `mcp-catalog-v1.json` below the private onboarding state
directory, at `<user-state>/repowitness/mcp-catalog-v1.json`. The enclosing
directory is created through the same private no-follow Unix capability path as
`onboard`; unsupported private-ACL platforms fail closed. Direct catalog mode
uses the normal onboarding user-state selection. The `repowitness codex install`
command configures an explicit `<Codex-home>/repowitness-state` user-state root
instead, so the one global integration does not depend on an ambient state-home
layout. The catalog has a 64 KiB byte limit, must be a regular no-follow UTF-8
JSON file, and is loaded once before the MCP runtime starts.

The startup process resolves only its own current directory to the containing
Git worktree. It does not accept a root or database from MCP input, traverse
siblings, scan parents after finding that root, scan a home directory, or load
configured roots. It indexes that one root using the resolved process
configuration. A new entry is written atomically only after complete index
activation; index or catalog failure leaves the previous file and previous
active generation readable.

The separately versioned connected-workspace catalog may be present beneath
the same private state root. It is admitted only after this current-worktree
resolution finds an exact explicit membership and is defined by
[codex-connected-workspace-catalog-v1.md](codex-connected-workspace-catalog-v1.md).
It does not alter the current-worktree authority boundary of this file.

## JSON shape

```json
{
  "schema_version": 1,
  "repositories": [
    {
      "repository_id": "rwi1:h:0000000000000000000000000000000000000000000000000000000000000001",
      "root": "/canonical/absolute/worktree",
      "database": "/canonical/absolute/private-state/repowitness/repositories/rwi1:h:0000000000000000000000000000000000000000000000000000000000000001/index.sqlite3"
    }
  ]
}
```

`schema_version` is exactly integer `1`. Unknown or duplicate fields are
rejected. `repositories` contains one through 32 entries. Every identity is a
canonical `rwi1:h:` value; every root and database is an absolute UTF-8 path
that canonicalizes to its recorded spelling. Repository IDs, roots, and
databases are each unique. No entry can select an arbitrary database: its path
must equal the private onboarding convention for its identity.

## MCP selection

Each server process captures the identity of the worktree it admitted at
startup. Its native read-tool schemas enumerate all catalog identities but make
`repository_id` optional only for the captured identity; omission selects that
one fixed entry. Selecting any other entry requires one exact opaque identity.
The catalog, host paths, and database paths are never returned. The static
registry schema and behavior remain separate: all registry calls require an
explicit selector and no static registry process gains a default.

Catalog v1 has no reload, status/list API, manual edit guarantee, background
watcher, daemon, root scan, remote catalog, general cross-repository query,
compatibility aliases, mutation, tasks, or personal-memory surface. Explicit
connected-workspace source-slot selection is available only through the
separate private catalog and its bounded source-view-aware tool subset.
