# Local MCP repository registry version 1

Status: proposed boundary format under [ADR-0049](../adr/0049-local-multi-repository-mcp-registry.md).

`repowitness mcp-serve --registry <path>` admits exactly one JSON document at
process startup. It is a local operator control file, not repository input and
not a persisted RepoWitness record. The server never returns its contents,
host paths, or database paths.

```json
{
  "schema_version": 1,
  "repositories": [
    {
      "repository_id": "rwi1:h:0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF",
      "root": "/absolute/path/to/repository-a",
      "database": "/absolute/path/to/repowitness-a.sqlite3"
    },
    {
      "repository_id": "rwi1:h:89ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF01234567",
      "root": "/absolute/path/to/repository-b",
      "database": "/absolute/path/to/repowitness-b.sqlite3"
    }
  ]
}
```

The root object has exactly `schema_version` and `repositories`. Every entry
has exactly `repository_id`, `root`, and `database`; all values have the shown
JSON types. `schema_version` is the integer `1`. The array contains 1 through
32 entries. Repository IDs use the canonical `rwi1:h:` text boundary and are
unique. Roots and databases are non-empty absolute UTF-8 paths, have no NUL,
and must not repeat textually within the registry. Relative paths, unknown
fields, duplicate keys, non-finite/numeric substitutions, and malformed JSON
are rejected.

The file is admitted through the bounded no-follow control-file reader with a
64 KiB byte limit. Its contents are read once; changing the file requires
starting a new MCP process. A registry entry is not a cross-repository query,
connected-workspace source slot, or permission to discover neighboring paths.

In registry mode every advertised read-tool schema adds a required
`repository_id` string constrained to the registered IDs. The selected value is
routing metadata only: it is removed before native tool validation and does
not alter any repository's index, active generation, evidence, or policy.
