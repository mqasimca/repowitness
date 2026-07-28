# ADR-0016: Add TypeScript and TSX to the Phase 0 generation

- Status: Accepted
- Date: 2026-07-27
- Last reviewed: 2026-07-27
- Owners: Project maintainers
- Scope: Phase 0 source selection, syntax analysis, artifact identity, SQLite
  persistence, retrieval, CLI, and MCP behavior

## Context

Accepted ADR-0015 adds exact Go and Rust syntax indexing. A maintainer need
demonstrated that repositories centered on TypeScript and TSX would otherwise
publish an internally honest but unhelpful unsupported generation. The locally
configured validation repository's identity, revision, contents, and
measurements are intentionally omitted from this public ADR.

TypeScript and TSX share much of their syntax but use distinct Tree-sitter
grammars. Treating both extensions as one artifact language would allow
identical bytes to reuse analysis produced by the wrong grammar. Treating
TypeScript as JavaScript, Go, or Rust would likewise corrupt producer and
evidence attribution.

The official `tree-sitter-typescript` package exposes separate TypeScript and
TSX grammars. Version 0.23.2 is MIT licensed and maintained in the
[official grammar repository](https://github.com/tree-sitter/tree-sitter-typescript).
It uses the `tree-sitter-language` and C build dependencies already present in
the workspace grammar boundary.

## Decision

Phase 0 additionally selects regular repository paths with exact,
case-sensitive `.ts` and `.tsx` extensions. TypeScript, TSX, Go, and Rust share
one source-state stability fence, canonical source manifest, source snapshot,
immutable SQLite generation, and atomic activation.

TypeScript and TSX are distinct source-language values with independent,
semantics-complete artifact identities. Each identity includes the exact
adapter implementation, its pinned grammar schema and version, resolved
configuration, analysis schema, and canonicalization version. Artifact reuse
must match the exact dialect identity. Language/path agreement is validated at
preparation, adapter-output, SQLite, search, and exact-retrieval boundaries.

The combined snapshot profile commits to all four language identities and the
exact `.rs`/`.go`/`.ts`/`.tsx` selection policy. Adding either TypeScript
dialect changes the mixed snapshot identity without changing the meaning of
the independent Rust or Go artifact identities. The supported-language
worktree-state receipt likewise uses a domain and version distinct from the
Rust-only and interim Go-and-Rust profiles.

The TypeScript syntax adapter emits deterministic source-order facts for:

- named functions, generator functions, and function signatures;
- named classes and abstract classes;
- interfaces, enums, type aliases, and internal modules/namespaces;
- method definitions and method signatures with identifier-like names; and
- identifier variable declarators, including exported `const` declarations.

Qualified names use syntactic namespace, class, and interface ancestors,
followed by the declaration name, with stable `::` separators. The adapter
does not use a filesystem path as a semantic name input. Facts retain exact
identifier and declaration byte spans.

The version-4 migration was still in flight and unreleased while this decision
was proposed. Its closed language set is extended with `typescript` and `tsx`,
and its fact-kind set is extended with `class`. Migrations 1 through 3 and
their checksums remain byte-for-byte unchanged. Search and exact retrieval
expose the authoritative persisted dialect.

TypeScript support is syntax-only. Phase 0 does not evaluate `tsconfig.json`,
package exports, module resolution, types, overload correspondence, control
flow, references, call sites, decorators, JSX elements, generated-code
provenance, or active build targets. JavaScript and MJS remain unsupported.
Computed property names, property declarations, destructuring bindings, and
anonymous default declarations do not produce symbol facts in this profile.

This decision does not broaden the proposed ADR-0014 memory-evidence schema,
which remains Rust-symbol-only.

## Alternatives considered

### Report the repository as unsupported

This preserves the Go/Rust scope but provides no searchable evidence for the
named maintainer need.

### Parse `.ts` and `.tsx` with one grammar identity

This reduces plumbing but makes content-addressed reuse unsound across
TypeScript and JSX-aware TSX parsing.

### Use the TypeScript compiler API

Compiler semantics would enable richer facts, but embedding a Node.js toolchain
or TypeScript compiler exceeds the local bounded syntax slice and introduces
subprocess, package-resolution, configuration, and versioning boundaries.

### Add JavaScript at the same time

Some TypeScript/TSX worktrees also contain JavaScript or MJS, but broadening
another grammar is not required for this decision. JavaScript can be proposed
with its own measured need and identity.

### Introduce a generic language-plugin ABI

Four built-in dialect adapters still do not justify a public plugin contract
or generic storage backend. The existing internal language seam remains
sufficient.

## Consequences

### Positive

- TypeScript/TSX worktrees receive useful searchable evidence from their
  supported source languages.
- TSX grammar behavior and reuse remain distinct from plain TypeScript.
- Mixed Rust, Go, TypeScript, and TSX repositories remain one atomic view.
- Existing evidence, source-state, and generation invariants remain intact.

### Negative and risks

- Two additional native parsers increase build, binary-size, supply-chain, and
  fuzzing surface.
- Syntax-only variable facts can be noisier than typed symbol resolution.
- Unsupported declaration shapes are intentionally absent from search.
- The in-flight version-4 schema and mixed snapshot golden vectors change
  before their first accepted release.
- Large TypeScript repositories expand performance and crash-recovery testing.

## Validation

- Golden TypeScript and TSX fixtures cover every admitted declaration category,
  nested qualification, exported variables, JSX, malformed syntax, UTF-8,
  stable order, spans, bounds, cancellation, and deadlines.
- Mixed four-language fixtures prove one manifest and generation, exact
  dialect-specific artifact identities, identical-byte separation, one-file
  invalidation, and clean-versus-incremental equivalence.
- SQLite migration fixtures preserve versions 1 through 3, admit exactly four
  language values and the `class` kind, and reject every alternate value.
- CLI and MCP contracts search and exactly retrieve TypeScript and TSX symbols
  with language, evidence, generation, and coverage aligned.
- An opt-in production external-worktree probe indexes both extensions,
  retrieves exact symbols, reuses every unchanged artifact, checks SQLite
  integrity, and leaves its input unchanged. Its identity and measurements
  remain local.
- The locked workspace, Clippy, test, rustdoc, dependency, benchmark, and
  documentation gates remain green.

## Implementation evidence — 2026-07-27

An opt-in production external-worktree probe verified atomic TypeScript/TSX
indexing, unchanged artifact reuse, exact retrieval with distinct persisted
producer manifests, database integrity, retired-generation rejection, and
input preservation. Repository identity, revision, symbols, corpus
measurements, and timings remain local.

The probe exposed a result-level attribution bug that had emitted the combined
snapshot producer for both dialects; per-occurrence artifact evidence and
installed-binary regression coverage now prevent that collapse. Final review
also corrected aggregate search-output accounting for the producer manifest
carried by every occurrence and added an exact-boundary regression.

Tree-sitter error or missing nodes remain explicit unresolved coverage.
[ADR-0023](0023-vendor-typescript-grammar-fix.md) records the checksum-pinned
local fixes for a bounded set of valid syntax shapes. Any remaining grammar
limitations still contribute raw error or missing-node counts. The evidence
contract does not convert partial syntax coverage into successful analysis.

The pinned `tree-sitter-typescript` package adds one direct analysis dependency
and builds both dialect grammars in a 3,388,024-byte release `.rlib` on the
probe's macOS arm64 host. The resulting stripped release CLI is 11,594,624
bytes. An isolated clean-`HEAD` build with the same lockfile, toolchain,
release profile, and a separate target directory produced a 7,894,000-byte
Rust-only CLI. The accepted four-language source slice therefore adds
3,700,624 bytes, or 46.88%. This delta includes Go and all supporting
implementation changes and is not attributed to the TypeScript grammar alone.

## Follow-up

- Completed 2026-07-27: review and accept the proposal after the complete
  implementation, real-worktree, dependency, and validation evidence passed.
- Completed 2026-07-27: measure the isolated release binary-size delta recorded
  above.
- Track the observed grammar limitations and re-evaluate them against a pinned,
  reviewed upstream upgrade.
- Decide JavaScript support and TypeScript memory correspondence separately.

## Supersession

The pre-release schema-version and migration-compatibility clauses are
superseded by [ADR-0022](0022-squash-pre-release-sqlite-schema.md). This
decision's TypeScript/TSX scope and identity rules remain accepted.
