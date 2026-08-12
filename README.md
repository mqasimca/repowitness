# RepoWitness

RepoWitness is a local-first Rust CLI and stdio MCP server that indexes Rust,
Go, TypeScript, TSX, and Python source into bounded, evidence-backed retrieval
and engineering-memory workflows.

The product loop is deliberately small:

```text
source change -> atomic index generation -> memory revalidation -> evidence-backed context
```

Every result identifies its generation, source evidence, coverage, and limits.

## Install

From a checkout:

```text
make install
~/.local/bin/repowitness --help
```

`make install` builds the locked release binary and atomically replaces
`~/.local/bin/repowitness`. Set `INSTALL_PATH` to choose another destination.

## Basic CLI

Index into a database outside the repository:

```text
repowitness index \
  --repository-id rwi1:h:<64-lowercase-hex> \
  --database /path/outside/repository/repowitness.sqlite3 \
  /path/to/repository
```

Useful read commands are `search`, `symbol-search`, `symbol-get`,
`locate-relevant-paths`, `architecture-map`, `architecture-overview`,
`repository-topology`, `graph`, `context-build`, `diagnostics`, `verify`, and
`memory-recall`. Use `repowitness <command> --help` for exact bounds.

`onboard --root <repository>` is the explicit private-state shortcut. `watch`
keeps one repository current in the foreground; it never starts a daemon.

Memory writes are explicit and local:

```text
repowitness memory-manage --help
```

## MCP

RepoWitness can expose one MCP connection for one repository or a private local
catalog containing all repositories onboarded by RepoWitness:

```text
repowitness mcp-serve \
  --repository-id rwi1:h:<64-lowercase-hex> \
  --database /path/outside/repository/repowitness.sqlite3 \
  --root /path/to/repository
```

For one MCP connection across repositories:

```text
repowitness mcp-serve --catalog
```

Run `repowitness onboard --root <repository>` once per repository. Catalog tool
calls select the registered repository identity; the current repository is the
default when the server starts inside it.

Catalog mode also provides `cross_repository_search` for bounded SQLite FTS5
literal search across all registered repositories. Results include each
repository's generation and coverage; matching text is candidate evidence, not
an inferred dependency relationship.

The catalog defaults to the shared user state location
(`$XDG_STATE_HOME/repowitness`, or `~/.local/state/repowitness`) so any local AI
client can use the same catalog. Use `--catalog-state-dir` only to deliberately
select another state root.

MCP also loads the optional shared user configuration from
`$XDG_STATE_HOME/repowitness/config.toml` (or
`~/.local/state/repowitness/config.toml`); `--user-config` overrides it.

The server is read-only by default. Add
`--enable-memory-writes --memory-actor <validated-actor>` only when the local
process should manage team memory. Stdout is reserved for MCP JSON-RPC; startup
help and errors go to stderr.

There is no daemon, connected-workspace CLI, SCIP import surface,
personal-memory surface, or durable task surface. The catalog is read-only by
default; explicitly enabling memory writes adds the same fixed-actor
`memory_manage` capability to each selected repository. It reloads its bounded
control file at MCP request boundaries; run
`onboard` and the next request sees the updated repository set without an MCP
restart. A malformed later catalog leaves the last valid snapshot in place.

## Development

Read [`AGENTS.md`](AGENTS.md), then:

```text
cargo fmt --all -- --check
cargo check --workspace --all-targets --all-features --locked
cargo test --workspace --locked
./scripts/check-docs
```

The repository uses the MIT License. See [`CONTRIBUTING.md`](CONTRIBUTING.md)
for provenance and clean-room requirements.
