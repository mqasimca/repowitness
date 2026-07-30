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

RepoWitness now has a tested Phase 0 Rust, Go, TypeScript, TSX, and Python
indexing slice:
sanitized bounded Git discovery, capability-contained no-follow source reads,
exact SHA-256 source and manifest identities, semantics-complete artifact keys,
bounded language-specific Tree-sitter facts, aggregate coverage, and final
path/content revalidation. The complete pinned mini-redis product-loop
benchmark produces stable cold/warm results and now
covers persistence, exact reuse, retrieval, default-read-only MCP, canonical
memory write and approval, source-change revalidation, recall, and stale-memory
context exclusion. A separate pinned historical comparison checks bounded
lexical search and naive memory against the same before/after evidence. An
opt-in isolated Codex evaluation makes the correct decision at both revisions,
uses current memory, ignores stale memory, and rates the packet useful. One
immutable Phase 0 SQLite baseline, compatible accepted version-2 migration,
and provisional version-3 connected-workspace migration contain the
five-language artifact format, append-only memory journal,
memory-revalidation projection, reviewed correspondence, and exact review-event
idempotency. Version 3 adds bounded source-slot membership, atomic immutable
workspace views, a generation-scoped Rust graph, and explicit bounded
generation-retention plan/apply; its defaults and migration remain provisional.
The store persists each artifact's exact language and prepared Rust, Go,
TypeScript, TSX, or Python facts through owned writer and reader connections
and publishes immutable
source and memory generations atomically, prevents competing process mutation
owners with a deadline-bounded lease, performs bounded generation-scoped
retrieval, and creates validated online backups. Its double-buffered FTS5
projection rebuild uses bounded transactions, integrity checks, and one atomic
reader-visible slot switch. Exact repeat indexing loads only requested complete
artifacts, checks independent canonical key and payload digests, validates facts
against the current immutable source bytes, and reports analyzed-versus-reused
file counts separately for all five languages. One source-state fence,
canonical manifest, snapshot, generation, and activation cover every supported
language; artifact identity and reuse remain language-specific. The shared
application publication use case stages and activates through a narrow port
implemented by the SQLite owner. The shared `code_search` use case validates
and hashes literal queries, maps storage-neutral candidates to
syntax-attributed material results, and carries exact snapshot, generation,
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
failures, output errors, mixed-language reuse and invalidation,
index-to-search generation replacement, exact declaration retrieval for all
five supported languages, and rejection of stale generations and modified
source. The CLI also exposes memory revalidation/recall, context compilation,
repository diagnostics, and generation-pinned Rust graph status, search,
evidence, architecture, trace, and impact reads. The local stdio MCP server
exposes eleven read-only tools: `code_search`, `context_build`, `diagnostics`,
`graph_architecture`, `graph_evidence`, `graph_search`, `graph_status`,
`graph_trace`, `impact_analyze`, `memory_recall`, and `symbol_get`. It fixes
repository identity, root, and database at process startup, bounds input,
output, concurrency, timeout, and cancellation, and keeps stdout protocol-only.
Protocol and installed-binary tests cover initialization, exact schemas, all
eleven tools, stale selectors, graph evidence and traversal, cancellation,
backpressure, and configured real supported-language repositories.

The accepted memory-record domain model, strict byte parser, canonicalizer, and
deterministic writer are implemented. A capability-contained boundary admits
or atomically writes only the exact canonical record path, rejects links,
aliases, special files, stale parents, and high-confidence secrets, and hashes
the exact presentation bytes. Publication revalidates the final file identity
and link count before reporting success. The shared import use case scope-checks
every record; the owned SQLite writer atomically appends or verifies immutable
versions, normalized child rows, observation-only Git history, separately
authorized local approvals, and exact manual review events. Re-import and
review retries are idempotent, cancellation and failure leave no partial
history, and online backups retain the journal. The SQLite baseline persists
versioned Rust occurrence fingerprints and immutable current-memory
projections.
Revalidation evaluates approved heads, Git-DAG or exact-worktree validity,
precision-first exact/rename/move or reviewed correspondence, ambiguity,
conflict, and meaning-change staleness before one atomic projection activation.
Recall returns the pinned projection with categorical freshness and coverage.
Context compilation deterministically fuses exact declaration bytes and
eligible current memory under a conservative byte budget, while diagnostics
reports the matching source/projection state, raw parser-error coverage, its
non-subtractive recognized-limitation subset, capabilities, and limitations.
Diagnostics profile 3 identifies the bounded Rust syntax graph capability and
states that graph coverage is syntax-derived and Rust-only: package-aware
resolution, macro expansion, SCIP, dynamic dispatch, and cross-language edges
remain unavailable or explicitly unresolved.
Exact declarations are directly readable UTF-8 when valid and display-safe,
and use an explicitly labeled lowercase-hexadecimal fallback otherwise. CLI
output JSON-escapes the untrusted declaration in one report field; MCP carries
separate encoding and declaration fields.
The CLI exposes explicit write, approve, review, and observation-only history
operations. Local MCP remains read-only by default and exposes the same
`memory_manage` use case only under explicit fixed-actor startup authorization.

TypeScript and TSX support is syntax-only and uses separate grammar and
artifact identities. JavaScript and MJS, TypeScript compiler semantics,
references, module resolution, and active `tsconfig.json` interpretation remain
unsupported.
The current parser is a checksum-pinned, MIT-licensed local grammar patch with
recorded [provenance](third_party/tree-sitter-typescript/REPOWITNESS-PROVENANCE.md).
It fixes a bounded set of valid TypeScript and TSX forms without suppressing
raw parser-error coverage; [ADR-0023](docs/adr/0023-vendor-typescript-grammar-fix.md)
records the clean-room review and upstream replacement conditions.

Python support is likewise syntax-only and uses an independent grammar and
artifact identity for case-sensitive `.py` and `.pyi` paths. RepoWitness does
not execute Python, resolve imports or environments, evaluate decorators,
infer dynamic dispatch, or extract references and calls.

Extended local verification passes `make ci`, `make test-all`, opt-in SQLite
release probes, repeated cancellation/race/recovery stress loops, and
production persistence, reuse, search, context, diagnostics, and MCP
round-trips across the supported languages. Locally configured external
repositories are confidential test inputs: their identities, paths, revisions,
symbols, contents, and per-repository measurements are intentionally not
recorded here. Publicly reproducible evidence comes from temporary
mixed-language fixtures and explicitly public pinned benchmark corpora.

The remaining Phase 0 milestone is deliberately narrow. The complete local
product loop, pinned correctness scenario, rewritten-history, review,
split/merge, canonical-file/SQLite publication-fault matrix, and controlled
public baseline comparison pass. A clean exact-revision Ubuntu 24.04 run also
passes the ratified benchmark budgets. ADR-0017, ADR-0019, and ADR-0023 are
accepted. One real design-partner task must still show that evidence-backed
memory changes a useful engineering decision before maintainers decide
ADR-0018 and ADR-0021.

## Local verification

GNU Make provides discoverable wrappers around the authoritative Cargo and
repository checks:

```text
make help
make ci
make test-all
```

`make ci` runs the required pull-request checks sequentially, including a
locked build and dependency-policy audit of the standalone fuzz crate.
`make test-all` adds the no-default-feature and release test profiles. Manual
SQLite timing and resource probes remain opt-in through
`make test-sqlite-benchmarks`. Executing the standalone
[memory-record fuzz target](fuzz/README.md) is also opt-in. The read-only GitHub
Actions `ci` job runs both required command sets on Ubuntu 24.04 for pull
requests and pushes to `main`; branch protection requires that job before
merge.

Run the external Phase 0 product-loop benchmark against a clean pinned
mini-redis checkout with:

```text
./scripts/run-phase0-benchmark /path/to/mini-redis
```

The runner creates separate disposable product and comparison worktrees before
creating memory or changing source. The manifest pins ten repeated warm
queries. Maintainers can run the same gate on Ubuntu 24.04 through the manual
`Phase 0 benchmark` GitHub Actions workflow. The workflow accepts only `main`,
uses the exact dispatched revision, and retains its public result as a
checksummed artifact. See the
[clean benchmark attestation](docs/research/phase0-clean-benchmark-attestation-2026-07-29.md),
[provisional development benchmark](docs/research/phase0-product-benchmark-2026-07-28.md),
and
[controlled comparative evaluation](docs/research/phase0-comparative-evaluation-2026-07-28.md)
for the latest environment, results, and remaining product gate.

Run the opt-in Codex usefulness evaluation against the same clean public
checkout with:

```text
./scripts/run-phase0-codex-evaluation /path/to/mini-redis 1
```

It supplies the structured MCP context packet to an ephemeral read-only Codex
process with shell, web, app, MCP, and collaboration tools disabled. The
runner rejects any tool event and verifies every cited evidence identifier
against the supplied packet. See the
[Codex utility evaluation](docs/research/phase0-codex-utility-evaluation-2026-07-28.md).

## CLI

Build the binary:

```text
cargo build -p repowitness-cli --locked
```

Runtime configuration is explicit. `index`, `workspace index`, `watch`, `gc`,
`search`, `graph`, `memory-recall`, `context-build`, `diagnostics`, and
`mcp-serve` accept any combination of:

```text
--user-config /path/to/user/repowitness.toml
--workspace-config /path/to/workspace/repowitness.toml
--repository-config ../repository/repowitness.toml
```

Only supplied files are read. They resolve in user, workspace, then repository
order regardless of option order. Ordinary preferences use that precedence,
while policy remains monotonic: a repository can tighten language, resource,
tool-profile, or memory-write policy but cannot grant authority denied by a
higher-trust layer. Configuration files are bounded to 65,536 bytes and fail
before repository/database work or MCP runtime initialization. Use
`repowitness config explain` for the path-free effective values and provenance,
and `repowitness doctor` to validate configuration plus optional explicit
repository/database targets. The strict format is documented in the
[configuration schema](docs/schemas/configuration-v1.md).

Create or update one local supported-language index using a stable
caller-assigned repository identity:

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

Atomically index an explicitly authorized connected workspace with a separately
selected manifest:

```text
target/debug/repowitness workspace index \
  --manifest /path/to/connected-workspace.toml \
  --database /path/outside/the/worktree/repowitness.sqlite3
```

The command has no ambient source discovery: the manifest is the complete,
bounded authorization set. Relative roots resolve only from its admitted parent
directory. Default output contains opaque digests and aggregate counts, never
roots, selector text, or manifest contents. The contract is defined by the
[connected-workspace manifest proposal](docs/adr/0032-explicit-connected-workspace-manifest.md).

Plan bounded generation retention without opening a writer or changing the
database, then explicitly apply that exact plan:

```text
target/debug/repowitness gc plan \
  --database /path/outside/the/worktree/repowitness.sqlite3

target/debug/repowitness gc apply \
  --database /path/outside/the/worktree/repowitness.sqlite3 \
  --plan-digest <retention_plan_sha256>
```

Planning emits only policy/plan digests and aggregate candidate, byte, root,
unresolved, truncation, and shared logical-row-work metrics. Apply recomputes
the policy, pins, roots, and candidate set and rejects a stale digest without
deletion. An exact committed retry returns the original aggregate receipt. If
an apply reports an unknown outcome, it may have committed: do not create a new
plan or change its pins or configuration. Re-run the identical apply command
with the same plan digest to recover the authoritative receipt.
Both commands have bounded deadlines and cooperative cancellation. `index` and
`watch` never run garbage collection automatically, and collection does not
run `VACUUM` or promise immediate SQLite file shrinkage.

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
persisted language, content and artifact digests, syntax tier, byte spans,
limitations, and independent coverage counts. The literal profile does not
expose raw FTS syntax.

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

`symbol-get` report schema 2 returns one definition declaration as labeled
display-safe UTF-8 or exact lowercase hexadecimal. The data remains one
JSON-escaped field, so untrusted source bytes cannot forge terminal report
lines. This presentation schema is independent of symbol profile 3. Retrieval
fails visibly if the selector is no longer in the active generation or the
current source bytes no longer match the indexed content digest. Phase 0 does
not return references.

Rebuild the immutable current-memory projection after indexing or changing
source:

```text
target/debug/repowitness memory-revalidate \
  --repository-id rwi1:h:0000000000000000000000000000000000000000000000000000000000000001 \
  --database /path/outside/the/worktree/repowitness.sqlite3 \
  ../repository
```

Recall projected memory with either bounded literal terms or `--all`:

```text
target/debug/repowitness memory-recall \
  --repository-id rwi1:h:0000000000000000000000000000000000000000000000000000000000000001 \
  --database /path/outside/the/worktree/repowitness.sqlite3 \
  --query Widget \
  --limit 20
```

Create or replace one canonical shared record from a complete strict
version-1 YAML input:

```text
target/debug/repowitness memory-manage write \
  --repository-id rwi1:h:0000000000000000000000000000000000000000000000000000000000000001 \
  --input /path/to/record.yaml \
  ../repository
```

The input and destination must be distinct directly contained regular files.
Create requires no parent; update and tombstone require one exact current
parent and the next display revision. The writer rejects path aliases,
concurrent replacement, and high-confidence credential forms without echoing
matched values.

Import reachable Git memory as observations only, then separately approve one
exact current record with a locally asserted actor:

```text
target/debug/repowitness memory-manage import-history \
  --repository-id rwi1:h:0000000000000000000000000000000000000000000000000000000000000001 \
  --database /path/outside/the/worktree/repowitness.sqlite3 \
  --actor local-reviewer \
  ../repository

target/debug/repowitness memory-manage approve \
  --repository-id rwi1:h:0000000000000000000000000000000000000000000000000000000000000001 \
  --database /path/outside/the/worktree/repowitness.sqlite3 \
  --record-id rwm1:h:00000000000000000000000000000000 \
  --actor local-reviewer \
  ../repository
```

Repository-authored actor text never approves itself. `import-history`
reports bounded coverage and may preserve successfully observed history while
reporting an incomplete shallow or over-limit traversal. Use
`memory-manage review --help` to append an approve, reject, or manual-link
event for one exact record-evidence and target-occurrence selector.

Compile exact source and eligible current memory under a conservative content
budget:

```text
target/debug/repowitness context-build \
  --repository-id rwi1:h:0000000000000000000000000000000000000000000000000000000000000001 \
  --database /path/outside/the/worktree/repowitness.sqlite3 \
  --root ../repository \
  --intent Widget \
  --budget 32768 \
  --limit 20
```

The `utf8_bytes_upper_bound_v1` budget is deterministic and conservative; it is
not an exact model-token count. If no memory projection exists, context
compilation remains source-only and reports that omission. Exact declarations
use labeled display-safe `utf8` or exact `lowercase_hex` representations; the
CLI puts their data in one JSON-escaped field so source text cannot forge
report lines.

Inspect the exact active generation, optional matching memory projection, raw
and recognized parser diagnostics, coverage, capabilities, limitations, and
the path-free resolved configuration digest/schema/resolver/profile without
mutation:

```text
target/debug/repowitness diagnostics \
  --repository-id rwi1:h:0000000000000000000000000000000000000000000000000000000000000001 \
  --database /path/outside/the/worktree/repowitness.sqlite3
```

Read the native immutable Rust syntax graph:

```text
target/debug/repowitness graph status \
  --repository-id rwi1:h:0000000000000000000000000000000000000000000000000000000000000001 \
  --database /path/outside/the/worktree/repowitness.sqlite3

target/debug/repowitness graph search \
  --repository-id rwi1:h:0000000000000000000000000000000000000000000000000000000000000001 \
  --database /path/outside/the/worktree/repowitness.sqlite3 \
  --query run
```

For a graph read from a connected-workspace index, replace `--repository-id`
with both explicit selectors. RepoWitness never chooses an arbitrary member of
a multi-source view:

```text
target/debug/repowitness graph status \
  --connected-workspace-id cwi1:h:0000000000000000000000000000000000000000000000000000000000000001 \
  --source-slot-id ssi1:h:0000000000000000000000000000000000000000000000000000000000000001 \
  --database /path/outside/the/worktree/repowitness.sqlite3
```

The `status`, `search`, `evidence`, `architecture`, `trace`, and `impact`
operations return bounded JSON with the concrete workspace view, graph
generation, complete publication receipt, categorical evidence, coverage, and
truncation. Copy exact definition or site JSON from one response into
`--start-json` or `--site-json`; optionally repeat the returned
`--workspace-view` and `--graph-generation` pair to read the same immutable
context. `trace` and `impact` require one or more `--edge-kind` values from
`import`, `reference`, and `call`. These are Rust-only syntax-derived
relationships, not compiler- or package-resolved claims.

Serve the same active index to Codex over local stdio:

```text
target/debug/repowitness mcp-serve \
  --repository-id rwi1:h:0000000000000000000000000000000000000000000000000000000000000001 \
  --database /path/outside/the/worktree/repowitness.sqlite3 \
  --root ../repository
```

The repository must be indexed first. To expose the mutation tool, the
operator must add both `--enable-memory-writes` and one fixed
`--memory-actor <local-actor>` to `mcp-serve`. Without both options, the server
lists only read tools. The default canonical profile lists eleven. A user-owned
configuration may opt into the incumbent-compatible profile, which adds seven
bounded read-only aliases. Their receipts currently claim only name
compatibility: request shapes are incompatible with the pinned public
observation, while response and behavior compatibility are not assessed.
Startup requires the configured tool profile to remain authorized and refuses
mutation when any effective layer denies memory writes. The enabled
`memory_manage` tool cannot choose the repository identity, root, database,
actor, host input path, timestamp, deadline policy, history revision, or
resource limits. Its request shape remains version 1. Its current output
receipt schema is version 2: database-backed approval, review, and history
receipts report checkpoint, shutdown, and final database-path identity
separately. They never report aggregate `complete` when a step is deferred or
the final identity is changed or unconfirmed.

When the database contains a connected workspace, add
`--connected-workspace-id <cwi1:h:...>` and `--source-slot-id <ssi1:h:...>`
together at `mcp-serve` startup to select the graph source. The graph tools
then read that exact source slot; the other MCP tools retain the configured
repository context.

Register the default read-only built binary with Codex:

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
server exposes the read-only `code_search`, `context_build`, `diagnostics`,
`graph_architecture`, `graph_evidence`, `graph_search`, `graph_status`,
`graph_trace`, `impact_analyze`, `memory_recall`, and `symbol_get` tools by
default. Call `code_search` first and pass its complete exact selector unchanged
to `symbol_get` when retrieving a declaration directly. Call `graph_search`
before graph trace or impact so its exact selector and immutable context can be
reused unchanged. Enable mutation only in a trusted local configuration whose
operator intends to grant that capability.

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
- [Phase 0 Codex utility evaluation](docs/research/phase0-codex-utility-evaluation-2026-07-28.md)
- [Phase 0 ratification review](docs/research/phase0-ratification-review-2026-07-28.md)
- [Full research and implementation plan](plan.md)

Accepted architecture decisions take precedence for the areas they cover. The
focused documents above describe current behavior and remaining work;
`plan.md` remains a historical research and rationale source.

## License

RepoWitness is licensed under the [MIT License](LICENSE). Contributions follow the clean-room and provenance rules in [CONTRIBUTING.md](CONTRIBUTING.md).
