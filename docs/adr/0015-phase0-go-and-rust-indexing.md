# ADR-0015: Index Go and Rust in one Phase 0 generation

- Status: Accepted
- Date: 2026-07-27
- Last reviewed: 2026-07-27
- Owners: Project maintainers
- Scope: Phase 0 source selection, syntax analysis, artifact identity, SQLite
  persistence, retrieval, CLI, and MCP behavior

## Context

The implemented Phase 0 vertical slice selected only case-sensitive `.rs`
paths. A maintainer need demonstrated that Rust-only selection could publish
an internally consistent but empty generation when the relevant source was Go.
The locally configured validation repository's identity, revision, contents,
and measurements are intentionally omitted from this public ADR.

The maintainer subsequently required indexing to support Go and Rust together.
Treating Go as Rust, sharing an artifact producer identity across grammars, or
publishing one generation per language would violate existing evidence,
artifact-reuse, and immutable-generation invariants.

The official `tree-sitter-go` grammar package provides the same Tree-sitter
language interface already used by the Rust adapter. Version 0.25.0 is MIT
licensed and maintained in the
[official grammar repository](https://github.com/tree-sitter/tree-sitter-go).
It adds a C parser build script and the small `tree-sitter-language` interface
dependency; the workspace already accepts the equivalent native grammar
boundary for `tree-sitter-rust`.

## Decision

Phase 0 selects regular repository paths with an exact, case-sensitive `.rs`
or `.go` extension. Both languages are captured by one source-state stability
fence, canonical source manifest, source snapshot, immutable SQLite generation,
and atomic activation. Other paths remain explicit skipped coverage.

Each selected file carries an explicit language value that must agree with its
exact case-sensitive repository extension at preparation and retrieval
boundaries. Rust and Go use independent, semantics-complete artifact identities
containing their exact adapter implementation, pinned grammar schema and
version, resolved configuration, analysis schema, and canonicalization version.
Artifact reuse must match the file's language-specific identity; identical
source bytes in a Go and Rust file can never share an analysis artifact.

The snapshot identity uses a separate versioned Go-and-Rust profile that
commits to both language profiles and the mixed-language selection policy.
Existing Rust-only snapshot and artifact identities retain their original
meaning and cannot collide with mixed-language snapshots.

The Go syntax adapter emits deterministic source-order declarations for:

- free functions and receiver methods;
- named structs and interfaces;
- other defined types and true type aliases;
- constants and package variables.

Qualified Go names use the package name, followed by the receiver type for
methods, with stable `::` separators. Multi-name `const` and `var`
specifications emit one fact per declared name in source order. Every fact
retains its exact identifier and declaration byte spans.

SQLite persists the language on each analysis artifact and admits the Go-only
`interface`, `defined_type`, and `variable` fact kinds through a forward
migration. Search and exact symbol retrieval expose the persisted language
rather than guessing it from a path.

Go support in this decision is syntax-only. Phase 0 does not resolve imports,
build constraints, type aliases across packages, embedded members, call sites,
references, generated-code provenance, or active build targets. It does not
change the proposed engineering-memory record in ADR-0014 or claim Go memory
correspondence support.

## Alternatives considered

### Keep Rust-only Phase 0

This preserves the smallest original scope but fails the named maintainer need
and produces no searchable evidence for Go-only worktrees.

### Run separate language indexes

Independent databases or active generations simplify each parser path but
make mixed repositories observable as inconsistent snapshots and prevent one
atomic search/retrieval view.

### Reuse one producer identity for both grammars

This reduces plumbing but makes content-addressed reuse unsound: identical
bytes could restore facts produced by the wrong grammar.

### Map Go declarations onto Rust-only fact kinds

Mapping interfaces to traits, defined types to aliases, or variables to
statics avoids a migration but misstates evidence. Honest categorical output
is more important than preserving an internal version-3 constraint.

### Add a generic language-plugin interface

Two built-in adapters do not justify a stable plugin ABI or generic storage
backend. Language adapters remain modules behind the existing analysis and
application boundaries.

## Consequences

### Positive

- Go and Rust repositories, including mixed repositories, produce searchable
  evidence in one atomic generation.
- Artifact reuse remains exact and language-safe.
- Coverage distinguishes indexed supported files from skipped unsupported
  paths.
- The change exercises the existing language-adapter seam without creating a
  plugin compatibility contract.

### Negative and risks

- The additional native grammar increases supply-chain, build, binary-size,
  and fuzzing surface.
- SQLite requires a version-4 migration and compatibility fixtures.
- Syntax-only Go extraction can include files excluded by a particular build
  constraint and cannot prove type or reference relationships.
- Internal Rust-specific type and function names require compatibility aliases
  or staged renaming until the public API is deliberately stabilized.
- Mixed-language integration and real-worktree probes increase the required
  test matrix.

## Validation

- Golden Go analyzer fixtures cover functions, pointer and generic receivers,
  structs, interfaces, defined types, aliases, multi-name constants and
  variables, malformed syntax, UTF-8, deterministic order, bounds,
  cancellation, and deadlines.
- Mixed Go/Rust fixtures prove one canonical manifest and generation,
  language-specific artifact keys, identical-byte separation, exact reuse,
  one-file invalidation, and clean-versus-incremental equivalence.
- SQLite migration fixtures upgrade versions 1, 2, and 3, preserve existing
  Rust artifacts, reject invalid languages/kinds, and rebuild both FTS
  projections.
- CLI and MCP contract tests search and retrieve exact symbols from both
  languages and keep language, generation, path, content, artifact, ordinal,
  spans, evidence, and coverage aligned.
- Application and SQLite corruption regressions reject a persisted or
  adapter-returned language that disagrees with the exact repository extension.
- An opt-in external-worktree smoke test finds Go declarations and completes a
  search-to-exact-retrieval round trip without changing its input. Its
  repository identity and measurements remain local.
- The locked workspace check, Clippy, test, rustdoc, `cargo-deny`, dependency
  boundary, benchmark, and documentation checks remain green.

## Follow-up

- Completed 2026-07-27: implement and document the built-in Go adapter,
  mixed-language profile, SQLite v4 migration, and CLI/MCP language fields.
- Completed 2026-07-27: run a production external-worktree probe and verify Go
  indexing, explicit coverage, unchanged artifact reuse, and exact retrieval.
  The input identity and per-repository results remain local.
- Completed 2026-07-27: an isolated clean-`HEAD` release build using the same
  lockfile, toolchain, profile, and a separate target directory produced a
  7,894,000-byte Rust-only CLI. The accepted four-language source slice
  produced an 11,594,624-byte CLI, a 3,700,624-byte or 46.88% increase. The
  release `tree-sitter-go` archive was 353,232 bytes. The source-slice
  measurement includes implementation code and the later TypeScript/TSX
  extension, so it is not attributed to the Go grammar alone.
- Decide Go memory evidence and correspondence semantics separately before
  broadening ADR-0014.

## Supersession

The pre-release schema-version and migration-compatibility clauses are
superseded by [ADR-0022](0022-squash-pre-release-sqlite-schema.md). This
decision's language scope and identity rules remain accepted.
