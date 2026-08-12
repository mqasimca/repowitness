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
uses the normal onboarding user-state selection, defaulting to
`$XDG_STATE_HOME/repowitness` or `~/.local/state/repowitness` on Linux. The
catalog has a 64 KiB byte limit, must be a regular no-follow UTF-8
JSON file, and is loaded once before the MCP runtime starts.

`onboard` is the catalog admission point. It indexes one explicit root and
atomically adds that root and its private database to the catalog after
successful activation. MCP startup only reads the catalog; it does not scan,
index, or mutate repositories. It does not accept a root or database from MCP
input, traverse siblings, scan parents, or load configured roots.

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
watcher, daemon, root scan, remote catalog, semantic cross-repository query,
compatibility aliases, mutation, tasks, or personal-memory surface. Explicit
connected-workspace source-slot selection is available only through the
separate private catalog and its bounded source-view-aware tool subset.

Catalog mode additionally exposes `cross_repository_search`, a bounded
SQLite-FTS5 literal search over the registered indexes. It returns per-repository
generation and coverage receipts but makes no dependency, ownership, or
relationship claim from matching text.
