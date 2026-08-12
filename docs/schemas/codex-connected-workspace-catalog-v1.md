# Codex connected-workspace catalog version 1

This document specifies the private control file used by accepted
[ADR-0051](../adr/0051-explicit-codex-connected-workspace-catalog.md). It
declares which explicitly created connected workspaces the one global Codex
catalog may recognize. It is not an MCP response, a repository artifact, or a
user-editable integration surface.

## Location and lifecycle

The file is named `mcp-connected-workspaces-v1.json` under the private shared
user-state root:

```text
$XDG_STATE_HOME/repowitness/mcp-connected-workspaces-v1.json
```

When `XDG_STATE_HOME` is unset on Linux, the default is
`~/.local/state/repowitness`. Other clients use this same catalog by passing
the same state root to `mcp-serve --catalog --catalog-state-dir` when needed.

Each workspace has a generated manifest at
`workspaces/<connected-workspace-id>/connected-workspace.toml` and its shared
SQLite database at `workspaces/<connected-workspace-id>/index.sqlite3`. The
directory and all control files are created through the same private,
no-follow capability path as catalog onboarding; platforms without an
equivalent private-state boundary fail closed.

Creation writes the generated manifest and indexes every source slot before
atomically replacing this file. A failure therefore cannot publish a new
catalog membership. Removing a workspace name only removes its registration;
the manifest and database remain subject to normal retention/recovery policy.

## JSON shape

```json
{
  "schema_version": 1,
  "workspaces": [
    {
      "name": "product-stack",
      "connected_workspace_id": "cwi1:h:0000000000000000000000000000000000000000000000000000000000000001",
      "members": [
        {
          "repository_id": "rwi1:h:0000000000000000000000000000000000000000000000000000000000000001",
          "source_slot_id": "ssi1:h:0000000000000000000000000000000000000000000000000000000000000001",
          "root": "/canonical/absolute/worktree-a"
        },
        {
          "repository_id": "rwi1:h:0000000000000000000000000000000000000000000000000000000000000002",
          "source_slot_id": "ssi1:h:0000000000000000000000000000000000000000000000000000000000000002",
          "root": "/canonical/absolute/worktree-b"
        }
      ]
    }
  ]
}
```

The file is bounded to 64 KiB and is reloaded at MCP catalog request boundaries
alongside the existing catalog snapshot. It has exactly
the shown fields: schema version is integer `1`; `workspaces` has at most 32
entries; workspace names are one through 64 lowercase letters, digits, and
interior hyphens; every workspace has two through 32 members. All opaque
identities must be canonical and unique in their required scope. A root must
be a canonical absolute UTF-8 worktree path, and no root may be a member of
more than one workspace. Unknown/duplicate fields, symlinks, non-regular
files, malformed JSON, non-canonical identities/paths, duplicate names,
identities, or roots are rejected.

The recorded generated manifest must exactly match the catalog membership and
its database must be the private path implied by the connected-workspace
identity. Catalog startup never accepts roots, database paths, source slots,
or membership changes from MCP input.

## MCP selection and relationship scope

When the process-current worktree is a member of exactly one registered
workspace, catalog startup refreshes all declared source slots atomically and
makes that member the default selector. The same process can select another
member only by its enumerated opaque `repository_id`; host paths and catalog
contents are never returned.

A catalog workspace is a shared immutable indexing view, not evidence of an
inferred semantic relationship. Cross-source links require attributed output
from a supported source-specific producer. Version 1 has no root scanning,
membership inference, watcher, daemon, remote/team state, arbitrary
cross-repository query, or MCP mutation surface.
