# RepoWitness

**A temporal, evidence-backed code-intelligence and engineering-memory engine.**

RepoWitness is an early implementation of a local-first MCP server and CLI intended to help coding agents understand a repository, retain verified engineering experience, and recognize when that knowledge no longer applies.

Its defining promise is:

> Every retrieved fact explains where it came from, how precise it is, when it was true, and what could invalidate it.

## Intended differentiators

- proof-carrying code retrieval with explicit coverage and limitations;
- engineering memory connected to source evidence and Git history;
- refactor-aware memory correspondence and staleness detection;
- token-budgeted context compilation;
- Git-reviewable team memory with local personal memory;
- SQLite-first local operation, with a demand-gated PostgreSQL server mode later;
- a Rust core designed for deterministic, bounded, crash-safe indexing.

## Current status

RepoWitness now has a tested Phase 0 Rust indexing slice:
sanitized bounded Git discovery, capability-contained no-follow source reads,
exact SHA-256 source and manifest identities, semantics-complete artifact keys,
bounded Tree-sitter Rust facts, aggregate coverage, and final path/content
revalidation. The pinned mini-redis preparation probe produces stable cold/warm
results. The accepted Phase 0 SQLite v3 schema persists prepared Rust facts
through owned writer and reader connections, publishes immutable generations
atomically, prevents competing process mutation owners with a deadline-bounded
lease, performs bounded generation-scoped lexical retrieval, and creates
validated online backups. Its double-buffered FTS5 projection rebuild uses
bounded transactions, integrity checks, and one atomic reader-visible slot
switch. Exact repeat indexing now loads only requested complete artifacts,
checks independent canonical key and payload digests, validates facts against
the current immutable source bytes, and reports analyzed-versus-reused file
counts. The shared application publication use case stages and activates
through a narrow port implemented by the SQLite owner. The shared `code_search`
use case validates and hashes literal queries, maps storage-neutral candidates
to syntax-attributed material results, and carries exact snapshot, generation,
producer, coverage, and pre-limit match counts. The shared `symbol_get` use
case requires the complete search occurrence identity, verifies the active
snapshot and generation, re-reads source through the contained no-follow
boundary, checks its content digest, and returns one bounded declaration with
syntax evidence.

The production `index` command requires a canonical explicit repository ID and
database path, constructs versioned configuration/schema/producer identities,
captures canonical Git/worktree receipts around source revalidation, and
activates only a complete generation. Its installed-binary tests cover real
temporary Git repositories, SQLite persistence, repeat indexing, redacted
failures, output errors, index-to-search generation replacement, exact
declaration retrieval, and rejection of stale generations and modified source.
The CLI exposes evidence-bearing `search` and `symbol-get`. The local stdio MCP
server exposes the same application use cases as read-only `code_search` and
`symbol_get` tools, fixes repository identity, root, and database at process
startup, bounds input, output, concurrency, timeout, and cancellation, and
keeps stdout protocol-only. Protocol and installed-binary tests cover
initialization, exact schemas, both tools, stale selectors, cancellation,
backpressure, and real cloned Rust repositories. Memory and context
compilation are not production-ready yet.

The latest full local verification on 2026-07-26 passed `make ci`,
`make test-all`, all four opt-in SQLite release probes, repeated
cancellation/race/recovery stress loops, and real-repository persistence,
reuse, search, and MCP round-trips. The real-repository runs covered `netwhy`
(48 paths, 22 Rust files, 698 facts) and `nvctl` (115 paths, 85 Rust files,
1,814 facts), with zero Tree-sitter syntax-error nodes in both snapshots.
These are verification fixtures, not ratified release benchmarks.

The remaining Phase 0 milestone is deliberately narrow: attach a verified
decision or failure, change the associated code, revalidate the memory, and
compile an updated context pack. The indexing and evidence-retrieval foundation
for that loop is implemented; production memory and context compilation are
not.

## Local verification

GNU Make provides discoverable wrappers around the authoritative Cargo and
repository checks:

```text
make help
make ci
make test-all
```

`make ci` runs the required pull-request checks sequentially. `make test-all`
adds the no-default-feature and release test profiles. Manual SQLite timing and
resource probes remain opt-in through `make test-sqlite-benchmarks`.
The read-only GitHub Actions `ci` job runs both required command sets on
Ubuntu 24.04 for pull requests and pushes to `main`; branch protection requires
that job before merge.

Run the external Phase 0 preparation probe against a clean pinned mini-redis
checkout with:

```text
./scripts/run-phase0-benchmark /path/to/mini-redis 10
```

## CLI

Build the binary:

```text
cargo build -p repowitness-cli --locked
```

Create or update a local Rust index using a stable caller-assigned repository
identity:

```text
target/debug/repowitness index \
  --repository-id rwi1:h:0000000000000000000000000000000000000000000000000000000000000001 \
  --database /path/outside/the/worktree/repowitness.sqlite3 \
  ../repository
```

The ID is an opaque logical repository identity, not a secret and not a digest
of the path, remote, or commit. Reuse the same ID intentionally across clones
or linked worktrees that represent the same logical repository. The command
rejects a database inside the indexed worktree, uses a fixed 30-second
end-to-end policy, prints only aggregate results, and leaves the previous active
generation readable if preparation, staging, activation, cancellation, or
staleness fails. Index writers coordinate through a persistent sibling
`<database>.repowitness-mutation.lock` file; the file is intentionally retained
after shutdown so every process continues to lock the same filesystem object.
Detected database hard-link aliases are rejected because they could split the
lease identity or make indexing mutate the captured worktree. The writer opens
an identity-checked file guard and revalidates the path after SQLite opens but
before connection policy, migration, recovery, or publication can write. If a
newly reserved database fails before startup completes, only that verified new
file is removed; an existing database is never deleted by startup cleanup.

Search the active generation with bounded literal terms:

```text
target/debug/repowitness search \
  --repository-id rwi1:h:0000000000000000000000000000000000000000000000000000000000000001 \
  --database /path/outside/the/worktree/repowitness.sqlite3 \
  --query Widget \
  --limit 20
```

Search output uses canonical byte-preserving repository paths and includes the
query profile and digest, source snapshot, active generation, categorical
resolution, exact returned/total match counts, fact ordinal, producer manifest,
content and artifact digests, syntax tier, byte spans, limitations, and
independent coverage counts. The literal profile does not expose raw FTS
syntax.

Retrieve the exact declaration identified by one search match:

```text
target/debug/repowitness symbol-get \
  --repository-id rwi1:h:0000000000000000000000000000000000000000000000000000000000000001 \
  --database /path/outside/the/worktree/repowitness.sqlite3 \
  --root ../repository \
  --snapshot <snapshot_sha256> \
  --generation <generation> \
  --path <match_0_path> \
  --content <match_0_content_sha256> \
  --artifact <match_0_artifact_sha256> \
  --fact <match_0_fact_ordinal>
```

`symbol-get` returns one definition declaration as lowercase hexadecimal so
untrusted source bytes cannot inject terminal controls. It fails visibly if
the selector is no longer in the active generation or the current source bytes
no longer match the indexed content digest. Phase 0 does not return references.

Serve the same active index to Codex over local stdio:

```text
target/debug/repowitness mcp-serve \
  --repository-id rwi1:h:0000000000000000000000000000000000000000000000000000000000000001 \
  --database /path/outside/the/worktree/repowitness.sqlite3 \
  --root ../repository
```

The repository must be indexed first. Register the built binary with Codex:

```text
codex mcp add repowitness -- \
  /absolute/path/to/repowitness/target/debug/repowitness mcp-serve \
  --repository-id rwi1:h:0000000000000000000000000000000000000000000000000000000000000001 \
  --database /absolute/path/to/repowitness.sqlite3 \
  --root /absolute/path/to/repository
codex mcp list
```

Alternatively, place the following in a trusted repository's
`.codex/config.toml` or in `~/.codex/config.toml`:

```toml
[mcp_servers.repowitness]
command = "/absolute/path/to/repowitness/target/debug/repowitness"
args = [
  "mcp-serve",
  "--repository-id",
  "rwi1:h:0000000000000000000000000000000000000000000000000000000000000001",
  "--database",
  "/absolute/path/to/repowitness.sqlite3",
  "--root",
  "/absolute/path/to/repository",
]
```

Restart the Codex client after changing configuration, then use `/mcp` in the
terminal UI to inspect the connection. Codex CLI, the IDE extension, and the
ChatGPT desktop app share the local Codex MCP configuration; see the current
[Codex MCP documentation](https://learn.chatgpt.com/docs/extend/mcp). The
server intentionally exposes only `code_search` and `symbol_get` in Phase 0.
Call `code_search` first and pass its complete exact selector unchanged to
`symbol_get`.

To inspect only aggregate repository-path discovery facts without indexing:

```text
target/debug/repowitness inspect-paths ../repository
```

`inspect-paths` invokes Git without a shell, enforces fixed deadline, output,
path-count, path-byte, and component bounds, validates exact path bytes, and
rejects symlink or special-file `.git` markers before Git runs. It does not
print path contents, ingest discovered file contents for analysis, persist
data, or create an index. Use `repowitness --help` for the complete current
command surface.

## Documentation

- [Codex and coding-agent guidance](AGENTS.md)
- [Documentation index](docs/README.md)
- [Product definition](docs/product.md)
- [Architecture](docs/architecture.md)
- [Architecture research and Phase 0 spikes](docs/research/architecture-2026-07-22.md)
- [Engineering standard](docs/engineering.md)
- [Roadmap](docs/roadmap.md)
- [Glossary](docs/glossary.md)
- [Architecture decisions](docs/adr/README.md)
- [Versioned schemas](docs/schemas/README.md)
- [Benchmark manifests](benchmarks/README.md)
- [Full research and implementation plan](plan.md)

Accepted architecture decisions take precedence for the areas they cover. The
focused documents above describe current behavior and remaining work;
`plan.md` remains a historical research and rationale source.

## License

RepoWitness is licensed under the [MIT License](LICENSE). Contributions follow the clean-room and provenance rules in [CONTRIBUTING.md](CONTRIBUTING.md).
