# Engineering standard

- Status: Active
- Last reviewed: 2026-07-29

This document defines the default implementation and review standard. Exceptions require an ADR with motivation, scope, owner, tests, and a review or removal date.

## Current enforcement

The workspace implements the local baseline through pinned Rust 1.97.1, Rust
2024 Edition, resolver 3, inherited workspace lints, forbidden first-party
unsafe code, exact production dependency versions, a committed lockfile,
`cargo-deny` policy, dependency-direction and benchmark-manifest scripts, and
GNU Make wrappers. `make ci` runs formatting, all-target/all-feature checking,
Clippy with warnings denied, default and all-feature tests, doc tests,
warning-free rustdoc, dependency policy, documentation, benchmark, and diff
checks. Vendored grammar sources and generated parsers have an independent
closed-inventory, checksum, symlink, and handwritten-capability gate.
`make test-all` adds no-default-feature and release all-feature tests.
The GitHub Actions `ci` job runs both command sets on a fixed Ubuntu 24.04
runner for pull requests and `main`, with read-only repository permissions and
the checkout action pinned by full commit. Branch protection requires that job
before merge.

Implemented specialized coverage includes deterministic/property-style domain
tests, hostile path and Git fixtures, clean-versus-incremental equivalence,
SQLite migrations, corruption detection, recovery, activation, checkpoint,
backup, process-termination and file-identity races, installed CLI contracts,
local stdio MCP contracts, and strict-memory hostile-input, golden,
property/mutation, output-bound, resource, and coverage-guided fuzz checks.
Manual SQLite resource/timing probes and longer memory resource/fuzz campaigns
remain opt-in and are not release budgets.

Cross-platform CI, Miri, Loom, sanitizer-backed fuzzing, general coverage,
`cargo-vet`, SemVer checks, packaging smoke tests, and latest-dependency
automation remain release/scheduled requirements rather than completed local
infrastructure.

## Toolchain and workspace

- Use stable Rust 2024 Edition with virtual-workspace `resolver = "3"`, as required for a virtual workspace by the [Cargo workspace reference](https://doc.rust-lang.org/cargo/reference/workspaces.html).
- Commit `rust-toolchain.toml` with a full stable patch release, `profile = "minimal"`, and the `clippy` and `rustfmt` components, using the checked-in format documented by [rustup](https://rust-lang.github.io/rustup/overrides.html#the-toolchain-file). Do not use a floating `stable` channel in CI or release builds.
- Treat toolchain updates like dependency updates: review the release notes, run the full relevant matrix, and prioritize stable point releases that correct compiler or standard-library defects.
- Declare and test `workspace.package.rust-version`; the supported MSRV is a machine-readable contract. Before 1.0, start with the stable version selected at workspace bootstrap and raise it deliberately. Before publishing a crate, document an MSRV support window; the default is at least six months, with increases announced in release notes.
- Shipped code builds on stable. Nightly is limited to isolated advisory jobs such as Miri, sanitizers, or fuzzing.
- Ship with `Cargo.lock`. Use `--locked` in required CI, reproducibility, packaging, and release jobs; a distinct latest-dependencies job intentionally updates the lockfile in a disposable checkout.
- Inherit edition, license, repository, MSRV, dependencies, profiles, and lints from the workspace.
- Set rustfmt `edition = "2024"` and `style_edition = "2024"`, following the [rustfmt edition guidance](https://github.com/rust-lang/rustfmt#rusts-editions).
- Keep Cargo features additive, documented, and tested. Prefer runtime configuration when several compiled backends are available.
- Test default, no-default, all-features, and supported production profiles. Define a separate minimal supported feature set when `--no-default-features` alone is not meaningful.
- Use `cargo tree -e features` to explain feature activation and `cargo tree --duplicates` to investigate avoidable duplicate builds. Adopt [`cargo-hack`](https://github.com/taiki-e/cargo-hack) only when the feature matrix is large enough to justify another pinned CI tool.

Enforce the six-package dependency direction accepted in [ADR-0008](adr/0008-layered-modular-monolith.md). Split further only for demonstrated dependency direction, compile-time, safety, ownership, release, or public-API reasons. Keep initial packages private to the workspace unless an external support commitment is intentional.

## Lints and code quality

- Deny compiler warnings in CI.
- Define the lint baseline once in the root workspace and require every safe first-party package to inherit it. Start with:

  ```toml
  [workspace.lints.rust]
  unsafe_code = "forbid"
  unsafe_op_in_unsafe_fn = "deny"
  unused_must_use = "deny"
  unexpected_cfgs = "warn"
  missing_docs = "warn"

  [workspace.lints.clippy]
  correctness = { level = "deny", priority = -1 }
  suspicious = { level = "warn", priority = -1 }
  complexity = { level = "warn", priority = -1 }
  perf = { level = "warn", priority = -1 }
  style = { level = "warn", priority = -1 }
  cognitive_complexity = "warn"
  too_many_lines = "warn"
  ```

- Enable selected pedantic, restriction, and API-related Clippy lints only when each rule improves this codebase. Do not enable an entire restriction group blindly.
- `unsafe_code = "forbid"` applies to safe first-party crates.
- Avoid crate-wide lint allowances. A narrow allowance includes a reason and issue/removal condition when temporary.
- Prefer explicit domain newtypes and enums over primitive strings/integers for IDs, states, scopes, evidence classes, and units.
- Optimize for contributor comprehension before clever lifetime or type-level machinery.
- Do not impose a maximum number of lines per source file. Split a module when it has multiple responsibilities, unrelated reasons to change, or an unclear ownership boundary. A function crossing Clippy's [default 100-line `too_many_lines` threshold](https://doc.rust-lang.org/clippy/lint_configuration.html#too-many-lines-threshold) or the configured cognitive-complexity threshold triggers review, not an automatic split.
- Let stable rustfmt own code-line wrapping. Follow the [Rust Style Guide](https://doc.rust-lang.org/style-guide/) for prose and comments rather than maintaining a second hand-formatted width convention.

## API and dependency boundaries

- Keep protocol/wire DTOs separate from validated domain types.
- Do not add Serde derives to internal domain aggregates merely for MCP, database, fixture, or config convenience; map through versioned boundary DTOs.
- Do not expose `rusqlite`, Tokio, tree-sitter, MCP SDK, or future PostgreSQL types through stable domain APIs.
- Use `thiserror`-style typed errors in reusable layers; add operation, path, repository, revision, and generation context at boundaries.
- `anyhow`-style reports are acceptable in the CLI and top-level process boundary, not as a substitute for domain errors.
- Public APIs document errors, cancellation, blocking behavior, complexity, feature requirements, and panics.
- Every crate has crate-level rustdoc describing its responsibility, invariants, dependency direction, and failure behavior, with an executable example when useful. Intentionally supported APIs follow the [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/checklist.html) for naming, standard traits, conversions, documentation, and future-proofing.
- Panic only for genuine invariant violations. Untrusted input, missing files, unsupported syntax, cancellation, and database contention are handled outcomes.
- No hidden global runtime, database, registry, or configuration singleton.

## Async, CPU work, and cancellation

- Tokio owns MCP transport, scheduling, I/O orchestration, cancellation, and shutdown.
- An owned, fixed-size Rayon pool owns parsing and CPU-heavy graph work; do not rely on the process-global pool.
- A dedicated OS thread owns the SQLite write connection, and each bounded read worker owns its read connection. Async tasks do not call blocking SQLite directly.
- A blocking discovery/reconciliation worker owns bulk filesystem enumeration and hashing; file watchers only submit dirty-path hints.
- Disable symlink/reparse following in discovery and watchers by default. If
  policy enables it, authorize the actual opened target beneath an allowed root
  through a contained-open operation; a canonicalize-and-prefix-check followed
  by a later reopen is not sufficient.
- Every queue, semaphore, channel, traversal, and result set has an explicit bound.
- Structured tasks are supervised. Dropping a future must not strand a child task that continues mutating state.
- Cancellation flows through discovery, parsing, resolution, persistence, validation, and activation.
- Tree-sitter parsing uses its progress callback for deadline/cancellation checks and resets parser state after an interrupted parse before reuse.
- Use immutable snapshots and message passing for index state; justify shared mutable state narrowly.
- Test shutdown during every pipeline stage and recover or discard staging state deterministically on restart.

## SQLite rules

- Use one writer ownership model with explicit transactions and WAL checkpoint policy.
- Acquire one OS-backed process mutation lease before opening the writer
  connection or running migration/recovery. Retry only within the operation's
  absolute deadline, keep the lease on the owner thread through shutdown, and
  retain the lock file after release so path replacement cannot split owners
  across different filesystem objects. One-shot local publication acquires the
  lease before source capture so contention cannot make a prepared snapshot
  stale before it reaches the writer.
- Database files must not have hard-link aliases. Reject a non-unique link
  count or a handle-identity match against a discovered worktree path before
  source-content capture or writer startup. Retain an opened file guard and
  revalidate its identity after SQLite opens but before the first write; path
  canonicalization alone does not merge hard links or close replacement races.
- If startup fails after atomically reserving a previously absent database,
  remove only that still-identity-matched new file after closing SQLite and
  file handles. Never apply failed-startup cleanup to a pre-existing database
  or a path whose identity changed.
- Before persistent connection configuration such as switching journal mode,
  require a pre-existing database to carry the RepoWitness application ID and
  an exact supported migration ledger. Never adopt an existing unmarked SQLite
  file as a new RepoWitness database.
- The local format has an immutable baseline-version-1 migration and
  compatible, monotonically numbered forward migrations. Accept only exact
  supported ledgers, and reject retired development versions 1 through 8
  without modifying them or creating journal sidecars; never reset them
  automatically because local approvals and review events may not be
  reconstructable.
- Startup recovery admits at most 4,096 incomplete generations, selects at
  most one extra identity to detect overflow before mutation, and runs under
  the caller's cancellation flag and absolute deadline through SQLite's
  progress handler. An over-limit or interrupted recovery rolls back without
  partially changing generation state.
- Configure busy timeouts and surface contention diagnostics.
- Bundle or verify a WAL-reset-fixed SQLite version: 3.51.3 or newer, or an explicitly documented fixed backport. Fail `doctor`/startup policy rather than silently use an affected multi-connection WAL build.
- Keep read transactions short, never across `.await`, and record WAL size, checkpoint progress/latency, busy outcomes, and reader-starvation diagnostics.
- Build into immutable staging generations and activate atomically.
- Keep migrations separate from request handling.
- Migrations are restartable and transactional when supported; back up before destructive transformation.
- Test fresh baseline creation, exact reopen, retired-version rejection,
  interruption, backup/restore, and projection rebuild. Once a compatible
  version 2 exists, test upgrades from every supported version.
- Use the SQLite online backup API for live backups. Never treat copying the main file without its WAL as a valid online backup.
- Reserve partial-backup and sidecar paths without clobbering, reject occupied
  sidecar namespaces, and let SQLite remove its own temporary journal state.
  Cleanup never unlinks an unowned journal, WAL, or shared-memory path.
- Never host or synchronize the live database on a shared network filesystem.
- Keep the default database under the platform user-state directory. Warn if an explicit worktree-local database is tracked or indexed.

## Unsafe and native dependencies

- Upstream native code does not require RepoWitness to own unsafe glue.
- If first-party unsafe is unavoidable, contain it in the smallest private audited module; create a dedicated crate only when the boundary justifies it.
- A package that owns unsafe code cannot inherit `unsafe_code = "forbid"`. It defines an otherwise-equivalent local lint table, sets `unsafe_code = "deny"`, and permits unsafe only in the audited boundary; document why that boundary is or is not a separate package.
- Every unsafe block has a `// SAFETY:` explanation; every unsafe function has a `# Safety` section.
- Keep `unsafe_op_in_unsafe_fn = "deny"`.
- Validate pointer, length, ownership, lifetime, thread-affinity, callback, and unwind invariants.
- Never unwind a Rust panic across a C ABI.
- Pin and inventory native sources and grammars, including checksums and licenses.
- Miri covers compatible safe/core code; sanitizers and fuzzing cover the native boundary on supported targets.

## Dependency and supply-chain policy

- The project MIT License and clean-room/contribution policy are recorded in [ADR-0009](adr/0009-mit-license-and-clean-room-contributions.md) and [`CONTRIBUTING.md`](../CONTRIBUTING.md). Preserve third-party licenses and notices; do not treat MIT licensing of original RepoWitness work as permission to copy upstream material.
- Every new production dependency states its need and reviews maintenance, source, license, advisories, enabled/default features, build scripts, proc macros, native code, binary-size impact, and MSRV impact. Prefer the smallest required feature set.
- Keep `deny.toml` committed and use
  [`cargo-deny`](https://github.com/EmbarkStudios/cargo-deny) as the primary
  check for RustSec advisories, allowed licenses, banned or duplicate
  packages, and trusted sources. Include development and build dependencies in
  license checks and development dependencies in duplicate-version checks; a
  test-only oracle is still supply-chain input. Check every committed Rust
  lockfile, including standalone fuzz or tooling workspaces excluded from the
  production workspace.
- Advisory or policy exceptions name an owner, exploitability rationale, compensating controls, and review/removal date. An ignore without those fields is not acceptable.
- Introduce [`cargo-vet`](https://mozilla.github.io/cargo-vet/) before release-critical or native third-party code ships, unless an ADR records an equivalent source-review process. Initial exemptions are explicit debt and are ratcheted down.
- Do not run a second lockfile advisory scanner merely for duplicate output. Use [`cargo-audit`](https://github.com/rustsec/rustsec/tree/main/cargo-audit) separately only when auditing a built artifact or deployment is a defined release requirement.

## Determinism and serialization

- Version every persisted, Git-tracked, MCP, task, and extension format.
- Decode wire data, validate it, then construct domain values.
- Use explicit units and bounded integer types for sizes, timeouts, depths, offsets, counts, and token budgets.
- Keep exact repository identity separate from host filesystem authorization.
  Follow the byte-preserving contract in
  [ADR-0010](adr/0010-repository-path-identity.md); never persist a target-local
  `PathBuf`, lossy string, or `OsStr::as_encoded_bytes()` value as repository
  identity.
- At textual boundaries, follow
  [ADR-0011](adr/0011-repository-path-text-encoding.md): use the exact
  `rwp1:h:` uppercase-Base16 scalar, require encoded and decoded byte limits,
  reject non-canonical input, and revalidate the decoded domain path. Optional
  display text never reconstructs identity.
- Test Unix bytes, Windows drive/UNC-looking input, case and Unicode aliases,
  non-UTF-8 names, reserved names, symlinks/reparse points, traversal, limits,
  and concurrent rename/swap behavior.
- Persist lossless repository identities and use host paths and line/column
  positions only for access or display.
- Specify ordering for every API collection and query result.
- Use canonical semantic serialization and content hashing for Git memory. YAML is presentation only; parse a strict bounded schema and hash a domain-separated, versioned canonical JSON form rather than YAML bytes.
- Artifact keys include source digest, schema, adapter/grammar/producer versions, resolved semantics-affecting configuration, and the canonicalization version.
- Never depend on hash-map iteration order, locale, wall-clock timing, or filesystem enumeration order for observable results.

## Observability and privacy

- Use structured tracing spans with repository, operation, generation, task, duration, counts, and cancellation outcome.
- Do not log source text, memory content, queries, environment values, credentials, or raw telemetry attributes by default.
- MCP stdio reserves stdout for protocol traffic; diagnostics use stderr or explicitly configured telemetry.
- Metrics have bounded cardinality and do not use raw paths or symbol names as labels.
- Diagnostic bundles are previewable, redactable, and opt-in.

## Testing strategy

| Layer | Required focus |
|---|---|
| Unit | Domain transitions, normalization, ranking, validation, and error mapping |
| Golden | Extraction, evidence envelopes, coverage receipts, context packs, and compatibility schemas |
| Property | Identity, memory lifecycle, deterministic ranking, canonical serialization, and migration invariants |
| Differential | Clean versus incremental index, syntax versus SCIP overlays, SQLite rebuild, and each explicitly claimed incumbent compatibility level |
| Integration | MCP stdio, Git worktrees, file watching, cancellation, crash recovery, and Git-memory import |
| Fuzz | Protocol/config decoders, paths/URIs, tree-sitter queries, imports, migrations, and query limits |
| Concurrency | Writer ownership, shutdown races, generation activation, cancellation, and backpressure |
| End-to-end | Source change through memory revalidation and context compilation |

Every defect in parsing, indexing, correspondence, validity, migration, or cancellation receives the smallest durable regression fixture.

Watcher integration tests must deliberately drop, duplicate, reorder, and coalesce events; reconciliation must still converge to the same manifest as a clean scan.

`cargo test` remains the authoritative default runner and doctest runner. If `cargo-nextest` is adopted for timeouts, slow-test reporting, or CI partitioning, retain a separate `cargo test --doc` job. CI jobs and relevant test groups have explicit timeouts.

Locally configured external repositories are confidential smoke-test inputs.
Their names, paths, revisions, symbols, source contents, and per-repository
measurements must not appear in repository files, snapshots, screenshots, or
default logs. Public validation records may state generic pass/fail coverage.
Reproducible correctness and performance claims use synthetic committed
fixtures or an explicitly public, pinned corpus.

The ignored real-repository path probe resolves relative inputs from the
workspace root and exercises the same production-shaped local adapter as the
diagnostic CLI. The adapter invokes Git without a shell, bounds its deadline,
captured output, path count, path bytes, and component count, supports a
polled cancellation signal, validates output into domain paths, and does not
print repository paths:

```text
REPOWITNESS_REAL_REPOSITORY=../repository \
  cargo test -p repowitness-local --test real_repository_paths --locked \
  -- --ignored --exact validates_all_discovered_git_paths --nocapture
```

The corresponding black-box diagnostic is:

```text
target/debug/repowitness inspect-paths ../repository
```

It reports aggregate counts and `index_created=false`; it does not ingest
source contents for analysis or stand in for the Phase 0 indexer.

The production one-shot index composition is exercised directly with:

```text
target/debug/repowitness index \
  --repository-id rwi1:h:0000000000000000000000000000000000000000000000000000000000000001 \
  --database /explicit/path/to/index.sqlite3 \
  ../repository
```

The command requires both identity and database path, uses the same bounded
local preparation and application publication use case as integration tests,
rejects a database inside the indexed worktree, and emits no repository,
database, identity, source, or symbol text. CLI contract tests use temporary
mixed Rust, Go, TypeScript, TSX, and Python Git repositories to prove one atomic
generation, language-specific analyzed/reused counts, one-language
invalidation, failure cleanup, redaction, and output-failure behavior. The
installed `search` and `symbol-get` contract additionally proves that complete
occurrence selectors retrieve exact declarations from all five languages with
persisted language, while an obsolete generation or source changed after
indexing fails without leaking its path, identity, or digest.

The MCP server contract is tested at three levels: wire DTO and bounded-line
unit tests; in-process SDK initialization, schema, tool, semaphore,
cancellation, and encoded-output tests; and an installed-binary stdio
round-trip. The black-box test indexes a temporary five-language worktree,
negotiates MCP `2025-11-25`, and lists exactly thirteen read-only tools:
`code_search`, `context_build`, `phase2_context_build`, `diagnostics`, `graph_architecture`,
`graph_evidence`, `graph_search`, `graph_status`, `graph_trace`,
`impact_analyze`, `memory_recall`, `scip_evidence`, and `symbol_get`. It retrieves exact
declarations from every language, builds exact UTF-8 source contexts through
both the preserved Phase 0 and the separately versioned Phase 2 profiles,
round-trips Rust graph status/search/evidence/architecture/trace/impact,
reindexes, and proves the old generation selector fails.
Focused protocol tests cover context, memory, diagnostics, graph and SCIP schemas,
contained SCIP import, read-only annotations, exact view/generation pinning,
categorical evidence, coverage, truncation, cancellation, backpressure, and
encoded-output bounds.
Stdout is parsed only as JSON-RPC and shutdown must leave stderr empty. A
durable ignored variant exercises the same index-to-exact-retrieval-and-context
path against a configured real supported-language worktree:

```text
REPOWITNESS_REAL_REPOSITORY=../repository \
  cargo test --release -p repowitness-cli --test cli_contract \
  mcp_stdio_round_trips_an_exact_symbol_from_a_real_repository --locked \
  -- --ignored --exact
```

Use the release test binary for this corpus-sized contract. Debug-mode parser
and SQLite overhead can consume the production index deadline before the MCP
assertions begin.

The complete opt-in local Rust preparation probe uses the same environment
variable and adds capability-contained source reads, content hashing,
Tree-sitter analysis, and final path/content revalidation:

```text
REPOWITNESS_REAL_REPOSITORY=../repository \
  cargo test -p repowitness-local --test real_rust_analysis --locked \
  -- --ignored --exact \
  configured_repository_prepares_and_revalidates_every_rust_source --nocapture
```

The path and complete Rust probes have passed against locally configured
external worktrees without modifying them. These are private smoke checks, not
committed performance corpora or release budgets; their input identities and
per-repository results remain local.

The complete pinned Phase 0 product-loop benchmark uses a clean external
mini-redis checkout and a disposable clone:

```text
./scripts/run-phase0-benchmark /path/to/mini-redis
```

It covers release SQLite publication, exact unchanged and one-file
incremental reuse, all required evidence targets, repeated query latency,
canonical memory write and local approval, current/stale revalidation and
context eligibility, default-read-only stdio MCP, database/WAL size, result
size, and peak RSS. A dirty development run is provisional evidence. Release
attestation requires an exact clean RepoWitness revision and a reviewed
manifest. The Phase 0 manifest pins ten warm-query samples and has ratified
budgets. The manual `Phase 0 benchmark` GitHub Actions workflow runs this gate
only from `main` on Ubuntu 24.04 with read-only repository permission and
retains the public checksummed output for review. Its first clean attestation
is recorded in the
[Phase 0 release evidence](research/phase0-clean-benchmark-attestation-2026-07-29.md).

The opt-in downstream-agent evaluation obtains the actual structured
`context_build` result through stdio MCP and supplies it to an ephemeral
read-only Codex process under a versioned response schema:

```text
./scripts/run-phase0-codex-evaluation /path/to/mini-redis 1
```

It disables shell, web, app, MCP, and collaboration tools; captures the JSONL
event stream; rejects every tool or unsupported event; and validates cited
source and memory identifiers against exact packet items. It also has fixed
per-run time and output limits and checks the before/after decision, source
grounding, current-memory use, stale-memory non-use, and packet usefulness.
The gate prevents runtime retrieval outside the packet but cannot remove
knowledge already present in model weights. Model token totals remain
observations rather than release budgets.

The production SQLite end-to-end probe adds schema migration, bounded staging,
atomic activation, an owned read connection, one evidence-bearing lexical
query, exact declaration retrieval from the real worktree, and retrieval
equivalence after rebuilding the disposable search projection:

```text
REPOWITNESS_REAL_REPOSITORY=../repository \
  cargo test -p repowitness-local --test real_sqlite_index --locked \
  -- --ignored --exact \
  configured_repository_persists_activates_and_searches_every_prepared_rust_fact
```

The expanded SQLite and installed-binary MCP probes have also passed against
locally configured supported-language worktrees. They exercise immutable
publication, unchanged reuse, bounded search, exact retrieval, source-only
context, diagnostics, retired-generation rejection, database integrity, clean
shutdown, disposable-artifact cleanup, and input-worktree preservation. Their
repository identities and individual observations remain local.

When non-trivial Cargo features exist, test default, no-default, all-features, and selected production configurations. Use a bounded `cargo-hack` matrix rather than an unbounded feature powerset.

## Required pull-request checks

When the Rust workspace and dependency-policy configuration exist, the baseline commands are:

```text
cargo fmt --all -- --check
cargo check --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --all-features --locked
cargo test --workspace --doc --all-features --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps --locked
cargo deny --locked check
./scripts/check-workspace-deps
./scripts/check-vendored-grammars
./scripts/check-benchmarks
```

The root [`Makefile`](../Makefile) provides convenience targets without
redefining this standard. Run `make help` to list them, `make ci` for the
required pull-request checks, and `make test-all` for the additional
no-default-feature and release test profiles. Manual SQLite benchmark probes
remain opt-in. The currently verified dependency-policy tool is
`cargo-deny` 0.19.4; `make deny` reports its exact installation command when
the binary is unavailable.

The affected change also runs the applicable specialized checks:

- configuration/schema validation;
- SQLite migration, mutation-lease contention, transaction, generation, and recovery tests;
- SQLite runtime-version/compile-option, WAL checkpoint, and online backup tests;
- clean-versus-incremental golden equivalence;
- artifact-key and canonical payload-integrity validation, including nullable
  payload reanalysis and corruption rejection;
- package dependency-policy and MCP boundary DTO checks;
- dependency advisory, license, and source-policy checks;
- applicable Linux, macOS, Windows, pinned-toolchain, and MSRV jobs.

Following the [Cargo CI guidance](https://doc.rust-lang.org/cargo/guide/continuous-integration.html), a scheduled job exercises the latest compatible dependency graph in a disposable checkout. A separate advisory job tests the Rust beta toolchain, as encouraged in the [Rust release guidance](https://blog.rust-lang.org/releases/latest/). Neither changes the required locked or pinned-toolchain results, and both notify an owner even when configured as non-blocking.

Scheduled or release jobs also include Miri, Loom, sanitizers, fuzzing, coverage, longer crash/concurrency stress, `cargo-vet`, SemVer checks for published crates, performance baselines, and packaging smoke tests.

CI actions and external tools are pinned by immutable revision or verified artifact. Automated dependency updates pass the relevant matrix and do not merge solely because constraints resolve.

## Performance discipline

- Record corpora, commits, hardware, OS, configuration digest, and cold/warm state.
- Measure before optimizing: wall time, CPU time, allocations, peak RSS, database size, queue depth, commit latency, and P50/P95/P99 query latency.
- Benchmark full indexing, single-file updates, generation activation, search, traversal, context building, memory recall, and projection rebuild.
- Treat relevant source lines per output token and stale-answer rate as product performance, not only runtime performance.
- Do not add a vector database, graph database, or additional search engine until profiling identifies a measured bottleneck.

## Security review baseline

- Treat repositories, memories, imported data, and plugin output as hostile inputs.
- Bound file sizes, parser work, graph depth, query results, deadlines, and captured subprocess output.
- Validate repository identity, then authorize the actual opened host resource
  beneath an allowed root. Do not rely on a checked path string that is reopened
  later.
- Keep secret values out of config diagnostics, logs, evidence packs, and shared memory.
- Fuzz parser boundaries, URIs, paths, imports, and query budgets.
- Tie approval/audit actors to the strongest available principal.
- Threat-model remote transport, PostgreSQL, extension execution, runtime ingestion, and UI before implementing each.
- When invoking Git CLI, never use a shell; sanitize configuration/environment, disable prompts, pagers, hooks, fsmonitor, and external diff, and enforce deadline/output bounds. Treat `gix` and CLI results as untrusted until validated into domain types.

## Definition of done

A feature is not complete until:

- its observable contract and limitations are documented;
- deterministic ordering, bounds, cancellation, and error behavior are defined;
- domain, integration, and regression tests cover its important invariants;
- diagnostics explain unsupported, skipped, unresolved, stale, or truncated outcomes;
- security and privacy implications are reviewed;
- benchmark impact is measured when the feature affects indexing, retrieval, memory, or resource use;
- relevant architecture, roadmap, glossary, configuration, and ADR documents are updated.
