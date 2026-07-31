# Local configuration schema version 1

- Status: Implemented under accepted ADR-0025
- Schema version: 1
- Resolver version: 1
- Canonical digest version: 1
- Last reviewed: 2026-07-29

This document defines the strict bounded `repowitness.toml` boundary implemented
by `repowitness-local` and the path-free semantic resolution implemented by
`repowitness-application`. The controlling decision is
[ADR-0025](../adr/0025-versioned-local-configuration-and-policy.md).

## Admission boundary

One file is at most 65,536 UTF-8 bytes. `schema_version` is required and must
equal integer `1`. Unknown root or table fields, duplicate TOML keys, invalid
types, unsupported enum spellings, excessive arrays, duplicate array values,
and out-of-range integers are errors.

Only regular files reached without symlink or reparse-point traversal are
admitted. Unix opens are nonblocking and no-follow so a FIFO or other special
file is rejected without waiting for a peer and a symlink is never traversed.
Windows opens reparse points without following them and rejects the resulting
handle. File type and size are taken from the opened handle. A shared
capability-contained reader records same-file identities for the root and every
parent, requires a uniquely linked final file, rereads and rehashes the bytes,
then performs a fresh no-follow walk before admission. Detected ancestor or
file replacement, aliasing, or in-place mutation fails closed.

The file DTO is decoded and discarded before validated application values are
constructed. Parser errors never retain or render input text. Version 1 has no
path, credential, token, command, hook, plugin, endpoint, or arbitrary string
field.

The root schema is:

```toml
schema_version = 1
profile = "local" # optional; user file only

[preferences]
query_results = 20
context_bytes = 65536
graph_depth = 8
graph_results = 1000
watcher_poll_interval_ms = 2000
mcp_tool_profile = "canonical"

[policy]
allowed_languages = ["rust", "go", "typescript", "tsx", "python"]
allowed_mcp_tool_profiles = ["canonical", "incumbent-compatible"]
max_source_file_bytes = 268435456
max_source_files = 1000000
max_query_results = 100
max_context_bytes = 1048576
max_graph_depth = 64
max_graph_results = 10000
retained_generations_per_source_slot = 2
max_retention_generation_candidates = 64
max_retention_rows = 1000000
max_retention_bytes = 536870912
deny_memory_writes = false
follow_symlinks = false
```

Every field other than `schema_version` is optional. Empty allowed sets are
valid tightening requests. Valid enum spellings are:

- profile: `local`;
- MCP tool profile: `canonical`, `minimal`, or `incumbent-compatible`; and
- language: `rust`, `go`, `typescript`, `tsx`, or `python`.

Every text scalar is at most 32 UTF-8 bytes. Language arrays contain at most
five unique elements. MCP tool-profile arrays contain at most three unique
elements.

## Numeric ranges

| Field | Inclusive range | Unit |
|---|---:|---|
| `query_results`, `max_query_results` | 1–100 | results |
| `context_bytes`, `max_context_bytes` | 1–1,048,576 | bytes |
| `graph_depth`, `max_graph_depth` | 1–64 | edges |
| `graph_results`, `max_graph_results` | 1–10,000 | results |
| `watcher_poll_interval_ms` | 100–86,400,000 | milliseconds |
| `max_source_file_bytes` | 1–268,435,456 | bytes |
| `max_source_files` | 1–1,000,000 | files |
| `retained_generations_per_source_slot` | 1–4,096 | generations per source slot |
| `max_retention_generation_candidates` | 1–4,096 | generations per transaction |
| `max_retention_rows` | 1–100,000,000 | shared logical row work per transaction |
| `max_retention_bytes` | 1–17,179,869,184 | estimated bytes per transaction |

Values outside these absolute ranges are rejected rather than clamped.
`follow_symlinks = true` is unsupported and rejected in version 1.

## Layering and provenance

The local parser admits user, workspace, and repository file categories.
Environment and CLI values use the same validated application types but are
not parsed as configuration files. Built-in and named-profile layers are
synthesized internally.

Ordinary preferences use this deterministic order:

```text
built-in defaults
    -> named profile
    -> user
    -> workspace
    -> repository
    -> environment
    -> CLI
```

The last ordinary request wins and records its category. Only user and CLI
layers may select `profile`.

Policy is monotonic:

- allowed language and MCP tool-profile sets are intersected;
- numeric ceilings take the minimum;
- `retained_generations_per_source_slot` floors take the maximum;
- memory-write denials are unioned; and
- source symlink following remains false.

Each effective policy value records every binding category. A numeric policy
ceiling may cap an ordinary default; both the ordinary supplier and binding
policy categories remain available.

`mcp_tool_profile` is a request, not startup authorization. The compiled
version-1 capability allow-list contains `canonical` and the opt-in
`incumbent-compatible` alias surface. `allowed_mcp_tool_profiles` can preserve
or shrink that set but can never grow it. A request for `minimal` remains
visible with no authorized profile; `doctor` and MCP startup reject it until a
separate implementation and contract make that profile available.

## Canonical semantic identity

The resolver hashes one fixed-order binary encoding with SHA-256. It starts
with the domain `RepoWitness\0resolved-semantic-configuration\0`, then encodes
digest, schema, and resolver versions; selected profile; effective numeric
preferences; requested and authorized MCP tool profiles; allowed-language and
allowed-tool-profile bit sets; effective numeric policy; memory-write denial;
no-follow policy; and the four effective retention-policy values.

Integers use fixed-width big-endian encoding and booleans use one byte.
Provenance categories, host paths, parser presentation, comments, and error
text are excluded. Equal effective semantics therefore produce the same
`ConfigurationDigest` regardless of layer order or provenance.

## Runtime binding

The CLI admits these files only through explicit `--user-config`,
`--workspace-config`, and `--repository-config` options. The options are
available on `index`, `watch`, `gc`, `search`, `graph`, `memory-recall`,
`context-build`, `diagnostics`, and `mcp-serve`; `config explain` and `doctor`
accept the same three paths for inspection. Configuration options may be
interleaved with other named options, but an index repository path after `--`
is always positional and is never reinterpreted as configuration.

Resolution completes before an operation adapter is invoked. The resulting
object is passed to local request builders so indexing applies the effective
language/source policy and query, recall, and context operations apply the
strictest caller/configuration bounds. Diagnostics wire schema 3 exposes only
`digest_sha256`, `schema_version`, `resolver_version`, and `profile` under its
`configuration` object.

MCP startup supports the default canonical profile and the opt-in compiled
`incumbent-compatible` alias profile. It rejects an unavailable or unauthorized
request before runtime creation. Enabling `memory_manage` still requires the
explicit fixed-actor startup capability and also fails when `deny_memory_writes`
is effective. A later repository layer cannot reverse a user/workspace denial
or expand the compiled profile allow-list.
