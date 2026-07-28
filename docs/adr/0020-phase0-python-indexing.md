# ADR-0020: Add Python to the Phase 0 generation

- Status: Accepted
- Date: 2026-07-27
- Last reviewed: 2026-07-27
- Owners: Project maintainers
- Scope: Phase 0 source selection, syntax analysis, artifact identity, SQLite
  persistence, retrieval, CLI, MCP, context, and diagnostics behavior

## Context

Accepted ADR-0016 defines one atomic Rust, Go, TypeScript, and TSX source
generation. A maintainer need demonstrated that repositories centered on
Python would otherwise produce no useful code evidence. The locally configured
validation repository's identity, revision, contents, and measurements are
intentionally omitted from this public ADR.

Python declarations, decorators, lexical nesting, and type-stub files require
language-specific extraction and identity. Treating Python as another
language, omitting it from the combined snapshot profile, or persisting it as
an existing language would permit false reuse and incorrect evidence
attribution.

The official
[`tree-sitter-python`](https://github.com/tree-sitter/tree-sitter-python)
package exposes one Python grammar through the Tree-sitter language interface.
Version 0.25.0 is MIT licensed, publishes its node schema for identity
fingerprinting, and uses the same `tree-sitter-language` and C build boundary
as the existing grammar packages.

## Decision

Phase 0 additionally selects regular repository paths with exact,
case-sensitive `.py` and `.pyi` extensions. Python shares the existing
source-state stability fence, canonical manifest, immutable SQLite generation,
and atomic activation with Rust, Go, TypeScript, and TSX.

Python is a distinct source-language value with an independent,
semantics-complete artifact identity. The identity commits to the adapter
implementation, pinned grammar node schema and version, resolved selection
configuration, analysis schema, and canonicalization version. The combined
source profile and source-snapshot encoding advance to a new version that
commits to all five language identities and the exact extension policy.
Existing per-language artifact identities retain their meanings.

The Python syntax adapter emits deterministic source-order facts for:

- classes;
- synchronous and asynchronous functions;
- functions whose direct lexical definition container is a class, categorized
  as methods;
- identifier module variables, including annotated and chained assignments;
  and
- Python 3.12 `type` alias statements with identifier names.

Qualified names contain syntactic class and function ancestors followed by the
declaration name, using stable `::` separators. A nested function remains a
function even when its outer function is a method. Decorated classes and
functions retain the full decorated declaration span while their identifier
span remains exact. Anonymous expressions, destructuring targets, attributes,
imports, parameters, and instance or class attributes do not produce facts.

Python support is syntax-only. Phase 0 does not execute Python, import project
configuration, resolve modules or types, evaluate decorators, infer dynamic
dispatch, select environments, inspect installed packages, distinguish active
build targets, or extract references and calls. `.py` and `.pyi` use one
grammar and artifact identity; the persisted repository path preserves which
file form supplied the evidence.

The [SQLite schema version 7](../schemas/phase0-sqlite-v7.md) rebuilds
`analysis_artifacts` with the exact existing constraints plus `python` in the
closed language set. It preserves every
artifact row and dependent foreign-key relationship, recreates the immutable
artifact triggers, validates foreign keys before publication, and leaves
migrations 1 through 6 byte-for-byte unchanged. Existing fact kinds already
cover the admitted Python declarations.

This decision does not broaden the Rust-only memory-evidence and automatic
correspondence contracts.

## Alternatives considered

### Keep Python unsupported

This preserves the four-language boundary but fails the named maintainer need
and cannot produce evidence for Python implementations.

### Invoke CPython or a Python language server

Compiler-level and environment-aware semantics could eventually improve
resolution. They require hostile project configuration, subprocess,
environment, import, and dependency policies beyond the bounded Phase 0 syntax
slice.

### Reuse an existing persisted language

This avoids a migration only by corrupting artifact identity and evidence
attribution. It is rejected.

### Add a generic language-plugin ABI

One internal adapter seam is sufficient for the named fifth language. A public
plugin lifecycle, trust boundary, and compatibility contract remain
premature.

## Consequences

### Positive

- Python repositories produce searchable, exactly retrievable code evidence.
- Python participates in the same atomic generation and exact artifact-reuse
  contracts as every existing language.
- Decorated and nested definitions retain deterministic evidence spans and
  names without executing repository code.

### Negative and risks

- The native grammar adds build time, binary size, and another versioned
  producer input.
- Syntax-only indexing cannot determine runtime bindings, import resolution,
  decorator effects, or environment-dependent behavior.
- Module assignment extraction can include configuration and test data that is
  not part of a public API.
- SQLite v7 must rebuild a referenced immutable table without losing rows,
  triggers, or foreign-key integrity.

## Validation

- Golden analyzer fixtures cover classes, decorated and async definitions,
  methods, nested functions, module assignments, type aliases, malformed
  syntax, UTF-8 identifiers, bounds, cancellation, and deadlines.
- Mixed five-language fixtures prove one manifest and generation, exact
  language/path checks, distinct artifact identities, and clean-versus-reused
  equivalence.
- Migration tests upgrade populated versions 1 through 6, preserve artifact
  rows and dependent facts/generation files, admit only the exact five
  languages, and pass `PRAGMA foreign_key_check` and `PRAGMA integrity_check`.
- Installed CLI contracts index, search, retrieve, and build source-only
  context from Python evidence; the stdio MCP contract performs the same exact
  search-to-symbol transfer, and diagnostics reports Python in stable language
  order.
- An opt-in cold/warm production external-worktree probe verified Python
  indexing, explicit syntax coverage, exact retrieval, unchanged artifact
  reuse, schema integrity, input preservation, and disposable database
  cleanup. Its repository identity and per-repository results remain local.

## Follow-up

- Decide Python memory evidence and correspondence semantics separately.
- Evaluate imports, references, and environment-aware analysis only after the
  Phase 0 release gates.

## Supersession

The pre-release schema-version and migration-compatibility clauses are
superseded by [ADR-0022](0022-squash-pre-release-sqlite-schema.md). This
decision's Python scope and artifact identity remain accepted.
