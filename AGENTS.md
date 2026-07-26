# RepoWitness agent guidance

## Scope and current state

- These instructions apply to the entire repository. Add a nested `AGENTS.md` only when a subtree gains genuinely different commands or rules.
- RepoWitness is in early Phase 0 implementation with a tested local Rust
  indexing and evidence-retrieval vertical slice. The workspace includes a
  usable CLI, local stdio MCP server, and implemented SQLite v3 schema with
  migrations from versions 1 and 2. Production engineering memory,
  correspondence/revalidation, context compilation, and a stable public API do
  not exist yet; verify the current command surface in `README.md` and the
  implementation boundary in `docs/roadmap.md`.
- Preserve the user's working tree. Do not commit, push, tag, publish, open a pull request, or rewrite Git history unless the user explicitly asks.
- RepoWitness uses the MIT License and the clean-room/provenance policy in [`CONTRIBUTING.md`](CONTRIBUTING.md). Do not copy or port upstream source, tests, fixtures, generated code, or substantial documentation without prior maintainer approval and recorded provenance, version, license compatibility, notices, and rationale. Independent research and behavioral comparison are allowed.

## Read before changing

Use the smallest relevant source set:

1. [`README.md`](README.md) for status and navigation.
2. [`docs/product.md`](docs/product.md) for product scope and non-goals.
3. [`docs/architecture.md`](docs/architecture.md) for system boundaries and invariants.
4. [`docs/engineering.md`](docs/engineering.md) for implementation and verification standards.
5. [`docs/roadmap.md`](docs/roadmap.md) for sequencing and phase gates.
6. [`docs/adr/README.md`](docs/adr/README.md) and the relevant ADRs for decision status and rationale.
7. [`docs/research/architecture-2026-07-22.md`](docs/research/architecture-2026-07-22.md) for unresolved architecture spikes and primary sources.
8. [`plan.md`](plan.md) only when broader research context or historical rationale is needed.

Accepted ADRs control the decisions they cover. Focused product, architecture, engineering, and roadmap documents control their areas next. Proposed ADRs are reviewable direction, not accepted fact. Do not change an ADR's status or broaden a phase without explicit maintainer intent.

## Discovery and planning

- For code discovery, use the codebase knowledge-graph MCP tools when they are available and the repository is indexed: `search_graph`, `trace_path`, `get_code_snippet`, then `query_graph` or `get_architecture`. Run `index_repository` first when the tool is available but has no current graph.
- Use `rg`/`rg --files` for documentation, configuration, literal text, scripts, or when graph results are unavailable or insufficient. Do not block work merely because the optional graph service is absent.
- Inspect `git status --short` before editing and again before handoff. Existing changes belong to the user unless the current task clearly created them.
- For a non-trivial change, state assumptions and make a short plan before implementation. Resolve architecture uncertainty through an ADR or named spike instead of silently choosing a permanent contract.
- When research is requested or a claim is date-sensitive, verify it from current primary sources: official specifications/docs, upstream repositories, or original papers. Record material findings and dates in the appropriate research or decision document.

## Product and architecture guardrails

- Protect the differentiating loop: source change -> atomic code-fact update -> memory revalidation -> evidence-backed context pack.
- Phase 0 proves one Rust-language vertical slice. Do not add PostgreSQL, remote MCP, vectors, general graph queries, plugin execution, runtime telemetry, a UI, or broad language support unless the task explicitly changes scope and the roadmap/ADR is updated.
- ADR-0004 through ADR-0008 and ADR-0010 through ADR-0013 are accepted implementation contracts. Do not silently weaken or bypass their identity, temporal-validity, generation-publication, Git-memory, path, source-state, SQLite-generation, or dependency-direction decisions; supersede a decision through a new ADR when necessary.
- Enforce ADR-0008's dependency direction:

  ```text
  repowitness-cli -> repowitness-mcp -> repowitness-application
  repowitness-cli -> repowitness-local -> repowitness-application
  repowitness-application -> repowitness-analysis -> repowitness-domain
  repowitness-local -> repowitness-analysis / repowitness-domain
  ```

- Domain code must not depend on Tokio, SQLite, Git, Tree-sitter, Serde wire DTOs, MCP SDK types, or filesystem I/O.
- Analysis consumes immutable content/snapshot inputs and performs no direct filesystem or database I/O.
- Keep CLI and MCP thin adapters over the same application use cases. Keep persisted and wire DTOs separate from validated domain types.
- Use narrow ports at real I/O, ownership, security, or multi-adapter boundaries. Do not introduce a generic storage backend, plugin ABI, microservice, or crate per feature preemptively.
- Snapshot correctness comes from canonical content manifests. Filesystem watcher events are hints and must converge through reconciliation.
- Readers pin one immutable generation. Index failure, cancellation, or a newer source epoch must leave the previous active generation readable.
- Reuse analysis only when source and every semantics-affecting adapter, grammar, schema, and configuration input match the artifact key.
- Material results expose evidence, producer, concrete snapshot/generation, categorical resolution, coverage, and unresolved or truncated work. Never turn missing evidence into confidence.
- Treat symbol correspondence as attributed evidence. Ambiguity, split/merge, and no-match are explicit; weak heuristics must not silently relink high-trust memory.

## Rust implementation rules

When the Rust workspace exists:

- Use stable Rust 2024 Edition with virtual-workspace `resolver = "3"`, a full patch release pinned in `rust-toolchain.toml`, declared/tested MSRV, and committed `Cargo.lock`. The toolchain file uses the minimal profile with rustfmt and Clippy; required CI and release jobs use `--locked`.
- Safe first-party crates forbid unsafe code. Any unavoidable first-party `unsafe` requires the narrow audited boundary, safety documentation, and tests defined in the engineering standard.
- Use explicit domain newtypes/enums for identities, states, scopes, evidence classes, units, and limits. Do not persist `usize`, enum discriminants, randomized hashes, or lossy platform paths.
- Every queue, worker count, traversal, parse, subprocess, query, result, and captured output has a bound, deadline, cancellation path, and diagnostic outcome.
- Tokio owns transport and orchestration. An owned bounded Rayon pool owns CPU work; blocking discovery and SQLite connections have dedicated owners. Never hold a synchronous guard or SQLite transaction across `.await`.
- SQLite WAL requires version 3.51.3 or newer, or an explicitly documented fixed backport. Configure busy timeout/checkpoint behavior explicitly and use the online backup API for live databases.
- Treat repository source, Git/config data, memory YAML, SCIP, MCP inputs, paths, and adapter output as hostile. Validate before constructing domain values; do not log source, memory, queries, secrets, credentials, or raw personal paths by default.
- Keep behavior deterministic: stable ordering and tie-breaking, versioned formats/profiles, canonical semantic hashing, injected clocks/IDs in tests, and no observable dependence on filesystem or hash-map iteration order.
- Do not enforce a source-file line cap. Split modules by responsibility and ownership; treat Clippy's function-length and cognitive-complexity warnings as review signals, with reasoned local exceptions.
- New production dependencies require a stated need, license/source/advisory review, minimal features, and consideration of maintenance, build scripts, proc macros, native code, binary size, and MSRV impact. At workspace bootstrap, enforce the committed `cargo-deny` policy and the dependency-vetting process in [`docs/engineering.md`](docs/engineering.md).

## Editing documentation and decisions

- Keep `AGENTS.md` practical; put rationale and detailed designs in focused docs or ADRs rather than duplicating them here.
- Update product, architecture, engineering, roadmap, glossary, schemas, and ADR links in the same change when their contract changes.
- Use [`docs/adr/0000-template.md`](docs/adr/0000-template.md) for a consequential decision. Record alternatives, negative consequences, validation, and revisit conditions.
- Do not rewrite accepted ADR history to make a new decision appear original. Add a superseding ADR.
- Use relative Markdown links inside repository documents and verify local targets.

## Validation

Run the narrowest relevant checks first, then the full applicable set. Do not claim a check ran if its prerequisite does not exist.

Documentation-only changes currently require:

```text
./scripts/check-docs
git diff --check
git status --short
git diff
```

`scripts/check-docs` enumerates both tracked and untracked Markdown files and checks local link targets, trailing whitespace, and LF line endings. Because `git diff` does not display untracked file contents, explicitly review every untracked path reported by `git status --short` before handoff.

Once the Rust workspace is bootstrapped, the baseline checks are:

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
./scripts/check-benchmarks
```

Also run the affected golden, property, integration, migration, crash/recovery, clean-versus-incremental, and MCP contract tests described in [`docs/engineering.md`](docs/engineering.md). A bug in parsing, identity, temporal validity, indexing, migration, cancellation, or recovery needs a durable regression fixture.

## Definition of done

- The requested outcome is implemented without unrelated cleanup or scope expansion.
- Architecture and evidence invariants still hold; failure, cancellation, stale input, and resource limits have explicit behavior.
- Relevant tests/checks pass, or the handoff names exactly what could not run and why.
- User changes are preserved, the final diff is reviewed, and generated/temp artifacts are not left behind.
- Documentation and ADRs match observable behavior.

## Code Review Rules

Prioritize correctness and trust failures over style. Flag:

- mixed-generation reads, partial activation, stale artifact reuse, or watcher-only correctness;
- unsupported certainty, hidden truncation, missing coverage, or heuristic relinking of trusted memory;
- unbounded work, blocking on Tokio workers, transactions/locks across `.await`, or unsupervised tasks;
- path traversal, symlink escape, unsafe Git/subprocess configuration, secret leakage, or untrusted input reaching domain/storage unchecked;
- destructive memory updates, loss of conflict/audit history, incorrect Git-DAG validity, or dirty snapshots treated as commits;
- protocol/storage types leaking into domain APIs, nondeterministic output, or changes that bypass Phase 0 gates;
- missing cancellation, crash recovery, cross-platform path behavior, regression fixtures, or documentation for a changed contract.

Reserve formatting concerns for rustfmt, Clippy, Markdown tooling, and CI.
