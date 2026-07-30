# RepoWitness provenance record

This directory is an approved, narrowly scoped vendoring of the MIT-licensed
[`tree-sitter-typescript`](https://github.com/tree-sitter/tree-sitter-typescript)
grammar. The upstream `LICENSE` file is preserved.

## Source and version

- Upstream repository:
  `https://github.com/tree-sitter/tree-sitter-typescript`
- Relevant upstream typed-template issue:
  `https://github.com/tree-sitter/tree-sitter-typescript/issues/341`
- Relevant upstream import-type issue:
  `https://github.com/tree-sitter/tree-sitter-typescript/issues/322`
- Reviewed patch:
  `https://github.com/tree-sitter/tree-sitter-typescript/pull/332`
- Rejected competing patch:
  `https://github.com/tree-sitter/tree-sitter-typescript/pull/342`
- Released package version: `0.23.2`
- Upstream base commit:
  `75b3874edb2dc714fb1fd77a32013d0f8699989f`
- Reviewed patch commit:
  `4777a6b3605b5d6227ccc7d5349b52e0d91e53b3`
- Patch parent: the exact upstream base commit above
- Rejected competing patch commit:
  `dc9481a6e1a5fd4a35e90f41790756c9ab7e0d08`
- License: MIT
- Review date: 2026-07-28

## Approved transformation

The reviewed upstream patch adds `instantiation_expression` to the allowed
function expression for a tagged-template call. Local clean-room revision 2
also makes these bounded grammar corrections:

- preserve `override` as a contextual expression identifier while retaining
  its class-member modifier role;
- accept a raw ampersand as JSX text while preserving valid HTML character
  references;
- allow an import-member type as the element of an array type;
- accept `in` as a type-member property after an automatic separator without
  changing expression-level `in`; and
- disambiguate a generic call whose type argument starts with
  `typeof import(...)`.

TypeScript and TSX `grammar.json`, `node-types.json`, and `parser.c` were then
regenerated with `tree-sitter-cli` 0.24.4, Node.js 26.5.0, and the locked
upstream JavaScript grammar input. The regeneration checker downloads only an
allow-listed upstream CLI asset, verifies its platform-specific SHA-256 digest
before execution, installs JavaScript inputs with lifecycle scripts disabled,
removes each stale dialect output, regenerates from that dialect directory,
and compares every generated runtime output byte for byte. The committed
minimal `package.json` and `package-lock.json` pin only the JavaScript grammar
input required for regeneration; their current audit reports no known
vulnerabilities. The generated files are intentionally kept as generator
output and are exempt from first-party source-layout conventions.

The vendored Cargo package is labeled `0.23.2+repowitness.2` and marked
non-publishable.

The local copy contains only the Rust build binding, runtime grammar sources,
grammar-definition inputs, generation lockfile, queries, license, and upstream
readme. It does not contain upstream tests, fixtures, examples, prebuilt
binaries, package caches, or local diagnostic programs.

## Regeneration

From the repository root:

```text
./scripts/check-vendored-grammar-regeneration
```

The checker accepts any Node.js `v26.*` runtime and supports macOS and Linux
on ARM64 and x86-64. CI pins Node.js 26.5.0 to retain one reproducible
regeneration environment. The pinned CLI asset hashes are part of the checker.
It runs in an isolated temporary directory with bounded subprocess time and
output, a dedicated npm cache, the public npm registry, lifecycle scripts
disabled, and ambient Node injection variables removed.

Both grammar gates must pass without updating expected values unless the
parser change has received a new review and local patch version:

```text
./scripts/check-vendored-grammars
./scripts/check-vendored-grammar-regeneration
```

## Integrity

`../../scripts/check-vendored-grammars` verifies this provenance record, the
exact grammar definition, node schemas, generated grammar descriptions, and
generated C parsers. It also rejects symlinks, unexpected files, lifecycle
scripts, and process, network, direct file-I/O, dynamic-loading, constructor,
or inline-assembly capabilities in executable handwritten inputs. The
separate clean-regeneration checker verifies the downloaded generator before
execution and compares all generated runtime outputs. The generated parser
checksums are:

- TypeScript:
  `8b31686490169b91a23d738104d008b1c44029cb7e866a464447bfbe356abbd2`
- TSX:
  `42d1632397a132707b40f8503e421c3d3b6c8a88f542be3ababda226e75e9836`

The analyzer producer identity includes the dialect-specific generated parser
and scanner checksums, the shared scanner-header checksum, and the local patch
version. A checksum or version change therefore prevents reuse of artifacts
produced by different parser semantics.

## Rationale and review

The released Rust package does not contain these generated fixes, and selecting
the reviewed patch through a Git dependency alone would still use the stale
generated parser. Non-retained black-box differential validation found no
remaining recovery nodes in the supported TypeScript/TSX corpus after local
revision 2. No fixture identity, source, revision, path, or measurement is
retained in this public repository. Reproducible correctness claims use only
the committed clean-room fixtures and clean-regeneration gate.

The local patch remains a temporary maintenance obligation. Replace it with a
pinned, reviewed upstream release when that release contains an equivalent fix
and passes clean-versus-incremental, parser-error, identity, size, license, and
supply-chain gates.
