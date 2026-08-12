# ADR-0062: Enable explicit memory management in catalog MCP

- Status: Accepted
- Date: 2026-08-12
- Owners: Project maintainers
- Scope: Local multi-repository MCP memory writes

## Decision

`mcp-serve --catalog` remains read-only by default. When started with
`--enable-memory-writes --memory-actor <validated-actor>`, it exposes
`memory_manage` for each selected registered repository. The actor,
configuration, repository identity, source root, and database are fixed when
the catalog snapshot is admitted; callers may select only an exact registered
repository ID.

Catalog membership and the catalog control file remain read-only to MCP.
Reloads rebuild the complete service snapshot with the same fixed actor, and a
failed reload preserves the last valid snapshot.

## Rationale

One MCP connection should manage memory across the same repositories it can
read. Requiring one MCP process per repository would make the supported
multi-repository setup needlessly difficult. The explicit actor and startup
flag preserve the existing default-deny trust boundary.

## Validation

The MCP contract test proves that the capability is absent by default and that
an enabled catalog routes `memory_manage` to the selected repository. CLI
startup validation still requires a valid actor and rejects effective policy
denials.

This supersedes the catalog memory-mutation exclusion in the implementation
sections of ADR-0049, ADR-0050, and ADR-0061; their catalog admission and
repository-isolation decisions remain in force.
