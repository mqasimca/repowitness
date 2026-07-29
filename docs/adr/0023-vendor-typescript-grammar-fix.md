# ADR-0023: Vendor a reviewed TypeScript grammar fix

- Status: Accepted
- Date: 2026-07-28
- Last reviewed: 2026-07-29
- Owners: Project maintainers
- Scope: TypeScript/TSX parser dependency, producer identity, and third-party provenance

## Context

The accepted TypeScript slice pins `tree-sitter-typescript` 0.23.2. That
release emits recovery nodes or structurally incorrect trees for several valid
forms, including typed tagged templates, import-member array types, contextual
`override` expressions, `in` type members, raw JSX ampersands, and generic
calls whose type argument starts with `typeof import(...)`. The
[upstream repository](https://github.com/tree-sitter/tree-sitter-typescript)
has a [narrow proposed fix](https://github.com/tree-sitter/tree-sitter-typescript/pull/332),
but no released Rust package contains its regenerated parsers. A Git revision
alone is insufficient because the Rust build consumes committed generated C.

RepoWitness must not suppress raw parser errors or reuse artifacts across
different parser semantics. Approved third-party reuse must also preserve the
source, exact revision, license, notices, transformation, and rationale.

Confidential black-box differential validation used the same immutable
TypeScript/TSX input for two upstream proposals. It reported a material
parser-error reduction for the narrow change and a regression for the broader
optional-type-argument change. Per-fixture measurements are intentionally not
retained in this public repository. Reproducible correctness claims use
committed clean-room fixtures.

## Decision

Vendor the runtime subset of `tree-sitter-typescript` 0.23.2 at upstream commit
`75b3874edb2dc714fb1fd77a32013d0f8699989f`, apply reviewed patch commit
`4777a6b3605b5d6227ccc7d5349b52e0d91e53b3`, and regenerate both parsers with
an audit-clean minimal lock that pins `tree-sitter-javascript` 0.23.1. Acquire
the `tree-sitter-cli` 0.24.4 generator only through the bounded regeneration
checker, which allow-lists supported upstream release assets and verifies
their platform-specific SHA-256 digests before execution.

Local clean-room revision 2 adds five bounded corrections: contextual
expression use of `override`, raw JSX ampersand text, import-member array
types, automatic type-member separation before an `in` property, and generic
call disambiguation for a `typeof import(...)` type argument. The scanner
lookahead for the last correction is fixed-length, returns only the `typeof`
token extent, and activates only while the type-query token is valid. The
automatic-separator change uses the existing expression-context signal so
expression-level `in` remains unchanged.

Keep the upstream MIT license and a local provenance record beside the
vendored source. Do not import upstream tests, fixtures, examples, diagnostic
programs, caches, or prebuilt binaries.

Use an ADR-recorded source review instead of adding `cargo-vet` for this narrow
patch. The review requires an exact base and patch commit, inspection of the
handwritten delta, an audit-clean minimal generation lock, byte-identical
clean regeneration with a hash-verified CLI asset, complete runtime-source and
provenance checksums, Rust license/source/advisory checks, and the full
affected parser/indexing matrix. Any runtime-source or generation-input change
increments the local patch version and repeats that review.

An independent comparison with
[`codebase-memory-mcp`](https://github.com/DeusData/codebase-memory-mcp/tree/d90986b2185badc3f8ce4fa881fff3991b851a92)
found that it vendors the unpatched generated TypeScript and TSX parsers from
the same base revision. Its separate
[grammar manifest](https://github.com/DeusData/codebase-memory-mcp/blob/d90986b2185badc3f8ce4fa881fff3991b851a92/internal/cbm/vendored/grammars/MANIFEST.md)
records the repository and revision, while its indexing path preserves
recoverable facts and reports bounded top-most `ERROR` or missing-node line
ranges as partial coverage. The comparison does not provide an equivalent
grammar fix. Its
[generic update script](https://github.com/DeusData/codebase-memory-mcp/blob/d90986b2185badc3f8ce4fa881fff3991b851a92/scripts/vendor-grammar.sh)
shallow-clones current upstream `HEAD`, does not select the recorded revision,
and does not reproduce its local TypeScript scanner include relocation. Its
[incremental path](https://github.com/DeusData/codebase-memory-mcp/blob/d90986b2185badc3f8ce4fa881fff3991b851a92/src/pipeline/pipeline_incremental.c)
classifies files by size and modification time and omits a cross-file semantic
pass. RepoWitness does not adopt those weaker
reproducibility and convergence properties: canonical content identity and
clean-versus-incremental equivalence remain required.

RepoWitness adopts the general supply-chain lesson by rejecting vendored
symlinks, inventory drift, and privileged capabilities in handwritten scanner,
build, and generation inputs. Per-file error ranges remain a separate future
coverage-contract decision; this ADR does not add them.

Identify the patched grammar as `0.23.2+repowitness.2`. Producer identity uses
the exact dialect parser and external-scanner checksums, not only
`node-types.json`. Revision 2 changes both parse behavior and the allowed child
schema for array types.

Continue to report all Tree-sitter error and missing nodes as raw syntax
coverage. A recognized parser-limitation count is a non-subtractive subset and
must never reduce or replace the raw count.

## Dependency review

- Need: TypeScript and TSX syntax indexing requires both generated grammars.
- Source and maintenance: the exact MIT-licensed upstream base, patch, and
  local maintenance/replacement conditions are recorded beside the source.
- Cargo surface: the crate has no features or proc macros. Its upstream build
  script uses `cc` to compile the TypeScript and TSX generated parsers and
  scanners. It performs no network access.
- Native boundary: generated C and Tree-sitter FFI remain third-party code.
  Complete source checksums, a closed vendor inventory, a handwritten-source
  capability audit, clean-room parser fixtures, locked Rust checks, and the
  affected indexing matrix cover that boundary.
- JavaScript tooling: the committed minimal regeneration lock is development
  input only. Regeneration installs it with lifecycle scripts disabled and
  invokes an exact CLI release asset only after platform-specific SHA-256
  verification. The lock currently audits with zero known vulnerabilities.
- Toolchain: the vendored Edition 2021 crate builds with the workspace's pinned
  Rust 1.97.1 toolchain. It does not change the workspace MSRV.
- Size: the vendored source adds about 18 MB, almost entirely generated C.
  Release binary size is measured after linking and recorded as validation
  evidence rather than treated as a portable constant.

## Alternatives considered

### Wait for an upstream release

This avoids local maintenance but leaves valid common syntax partially parsed
for an unbounded period. No release currently contains the generated fix.

### Use the broader optional-type-argument proposal

That [proposal](https://github.com/tree-sitter/tree-sitter-typescript/pull/342),
commit
`dc9481a6e1a5fd4a35e90f41790756c9ab7e0d08`, covers more syntax in principle,
but the confidential differential fixture regressed materially. Per-fixture
measurements are intentionally not retained. The proposal fails closed as an
upgrade candidate.

### Suppress the known recovery shape

This would make metrics look cleaner without improving the parse tree. It
would weaken evidence and coverage semantics, so raw errors remain visible.

### Replace Tree-sitter or add a second parser

A second TypeScript front end would add dependency, identity, performance, and
consistency costs far beyond the narrow Phase 0 defect.

## Consequences

### Positive

- The bounded valid syntax forms parse without recovery nodes in their
  applicable dialects.
- The patch is deterministic, checksum-pinned, licensed, and auditable.
- Parser semantics participate directly in artifact identity.
- Upstream replacement remains a bounded, testable dependency update.

### Negative and risks

- The repository carries about 17 MB of generated third-party C.
- Maintainers own regeneration, source review, checksum, and upstream-tracking
  work until an equivalent release is adopted.
- Generated files exceed normal human-readable module sizes and must not be
  hand-edited.
- The native parser boundary still requires dependency, compiler, sanitizer,
  and advisory review before release.

## Validation

- Clean-room TypeScript and TSX fixtures cover every local correction,
  expression and modifier guards, HTML entities, malformed syntax, stable
  facts, and raw/known count invariants.
- Vendored grammar checks verify exact source, provenance, and generated-parser
  checksums; reject symlinks and inventory drift; scan executable handwritten
  inputs for privileged capabilities; and reproduce generated runtime outputs
  byte for byte with a hash-verified CLI asset.
- Confidential external black-box differential probes report only generic
  pass/fail coverage in public records and leave the input unchanged.
- Cold, warm, one-file incremental, SQLite integrity, CLI/MCP contract,
  locked-build, Clippy, rustdoc, dependency, and benchmark gates remain green.

## Follow-up

- Accepted 2026-07-29 after provenance, inventory, checksum, regeneration,
  capability, parser-regression, language-matrix, dependency-policy, full CI,
  and clean release-platform benchmark checks passed.
- Track the upstream issue and replace the local patch only with a pinned
  release that passes the same regression and identity matrix.
- Re-measure release binary size after the local parser is linked.

## Supersession

This decision supersedes only ADR-0016's crates.io dependency-selection and
grammar-upgrade follow-up. ADR-0016's language scope, raw coverage, dialect
identity, and generation contracts remain accepted.
