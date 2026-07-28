# Phase 0 production dependency review

- Status: Active
- Last reviewed: 2026-07-26

This review records why the first production dependencies beyond workspace
packages are present. Versions are exact workspace pins. Cargo's lockfile,
`cargo deny`, feature inspection, and full CI remain authoritative for the
resolved transitive graph.

## `sha2` 0.11.0

- Need: standard SHA-256 source identities and domain-separated canonical
  artifact-key digests.
- Package: [`sha2`](https://docs.rs/sha2/0.11.0/sha2/), maintained by
  [RustCrypto](https://github.com/RustCrypto/hashes).
- License: MIT OR Apache-2.0.
- Declared MSRV: Rust 1.85, below RepoWitness's Rust 1.97.1 baseline.
- Features: default features disabled; no allocation, object-identifier, or
  zeroization feature is requested.
- Code/build shape: pure Rust library; no build script, proc macro, or native
  library. Architecture-specific implementations are upstream internals behind
  the safe digest API.
- Scope: `repowitness-analysis` hashes exact and name-elided declaration
  fingerprints; `repowitness-application` hashes canonical source, snapshot,
  and artifact identities; `repowitness-local` hashes exact migration and
  memory-presentation bytes. Domain digest values remain dependency free and do
  not expose RustCrypto types.
- Lockfile impact: the exact `sha2` line was already present through the
  dev-only `gix` oracle; RepoWitness now uses it in the production application
  graph.

## `rusqlite` 0.40.1 and bundled SQLite 3.53.2

- Need: the accepted local persistence contract requires SQLite WAL,
  transactions, FTS5, progress callbacks, online backup, and explicit
  connection ownership.
- Package:
  [`rusqlite`](https://docs.rs/rusqlite/0.40.1/rusqlite/), maintained in
  [`rusqlite/rusqlite`](https://github.com/rusqlite/rusqlite).
- License: MIT.
- Declared MSRV: Rust 1.85, below the workspace baseline.
- Features: default features are disabled; only `bundled`, `backup`, and
  `hooks` are enabled. The hook feature is required solely for SQLite's
  progress callback. Loadable extensions, SQLCipher, serialization, sessions,
  user-defined functions, URL/time/UUID adapters, and generic virtual-table
  APIs are excluded.
- Code/build shape: `libsqlite3-sys` compiles the pinned upstream SQLite C
  amalgamation. Unsafe FFI remains inside upstream crates; all first-party
  crates continue to forbid unsafe code. The runtime validates SQLite
  3.51.3-or-newer and `ENABLE_FTS5` before serving.
- Scope: `repowitness-local` only. Connections remain private to dedicated
  writer, reader, and backup owner threads. No SQLite, SQL, or rusqlite type
  crosses into domain or analysis APIs.
- Validation: exact migration identity, malformed file rejection, immutable
  content, bounded staging, crash recovery, atomic activation, pinned reads,
  progress cancellation, hostile literal search, result-byte limits,
  checkpoint contention, online backup/restore, and real-repository
  persistence/retrieval all have focused tests.

## `tree-sitter` 0.26.11

- Need: bounded direct-syntax parsing for the Phase 0 Rust adapter, including
  cooperative progress callbacks.
- Package: [`tree-sitter`](https://docs.rs/tree-sitter/0.26.11/tree_sitter/),
  from the official
  [`tree-sitter/tree-sitter`](https://github.com/tree-sitter/tree-sitter)
  repository.
- License: MIT.
- Declared MSRV: Rust 1.77, below the workspace baseline.
- Features: default features disabled; only `std` is enabled. Wasm and bindgen
  are excluded.
- Code/build shape: safe Rust bindings over Tree-sitter's bundled C runtime.
  Its build script uses `cc` and build-time `serde_json`; upstream owns the FFI
  and unsafe code. RepoWitness adds no first-party unsafe boundary.
- Transitive runtime shape: regex, regex-syntax, streaming-iterator, and the
  small language ABI package. Build-only dependencies include `cc`,
  `serde_json`, and their resolved support packages.
- Scope: `repowitness-analysis` only. The adapter accepts immutable bytes and
  performs no filesystem, database, Tokio, or protocol I/O.

## `tree-sitter-rust` 0.24.2

- Need: the official Rust grammar paired with the Tree-sitter runtime.
- Package:
  [`tree-sitter-rust`](https://docs.rs/tree-sitter-rust/0.24.2/tree_sitter_rust/),
  from the official
  [`tree-sitter/tree-sitter-rust`](https://github.com/tree-sitter/tree-sitter-rust)
  repository.
- License: MIT.
- Declared MSRV: not published. The exact package builds and tests with the
  pinned workspace toolchain; an MSRV claim is therefore not inferred.
- Features: none.
- Code/build shape: the package build script compiles the generated Rust parser
  C source through `cc`. The generated grammar and native code remain upstream
  dependencies; no source is copied into RepoWitness.
- Scope: `repowitness-analysis` only. Grammar version changes alter the
  producer manifest and invalidate artifact reuse.

## `tree-sitter-python` 0.25.0

- Need: bounded syntax-only declaration extraction for Python and Python stub
  files without executing repository code.
- Package:
  [`tree-sitter-python`](https://docs.rs/tree-sitter-python/0.25.0/tree_sitter_python/),
  maintained in the official
  [`tree-sitter/tree-sitter-python`](https://github.com/tree-sitter/tree-sitter-python)
  repository.
- License: MIT.
- Declared MSRV: not published. The exact package builds and tests with the
  pinned workspace toolchain; no lower support claim is inferred.
- Features: none.
- Code/build shape: the package build script compiles the generated Python
  parser C source through `cc`. Runtime exposure is the small
  `tree-sitter-language` interface plus the embedded node schema. Upstream owns
  the generated grammar, FFI, native code, and build script; RepoWitness adds
  no first-party unsafe boundary.
- Maintenance and integrity: version 0.25.0 is published by the official
  grammar owners and its repository, license, and node schema agree with the
  registry package inspected on 2026-07-27. The exact version is lockfile
  pinned and covered by the existing source, license, advisory, and build
  policy.
- Scope: `repowitness-analysis` only. Grammar and node-schema bytes enter the
  Python producer manifest, so any version change invalidates Python artifact
  reuse without changing other language identities.

## `cap-std` and `cap-fs-ext` 4.0.2

- Need: capability-relative source opening that cannot escape the explicitly
  authorized repository root, plus no-follow opens for every intermediate
  directory and the final file.
- Packages: [`cap-std`](https://docs.rs/cap-std/4.0.2/cap_std/) and
  [`cap-fs-ext`](https://docs.rs/cap-fs-ext/4.0.2/cap_fs_ext/), maintained in
  the Bytecode Alliance
  [`cap-std`](https://github.com/bytecodealliance/cap-std) repository.
- License: Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT. The transitive
  `winx` 0.36.4 package declares only Apache-2.0 WITH LLVM-exception, so
  `deny.toml` admits that exact package-expression pair rather than broadening
  the baseline allowlist.
- Declared MSRV: not published. The exact packages build and test with the
  pinned workspace toolchain; an MSRV claim is not inferred.
- Features: `cap-std` has no default features; `cap-fs-ext` enables only its
  `std` integration. UTF-8 wrapper, Camino, and ARF-string features are
  excluded.
- Code/build shape: no build script or proc macro. The capability
  implementation is platform-specific safe Rust over `rustix` and Windows
  system bindings. Its normal transitive graph includes `cap-primitives`,
  `io-extras`, `io-lifetimes`, `fs-set-times`, and target-specific Windows
  packages.
- Scope: `repowitness-local` only. The ambient authority is used once to open
  the user-selected root; every repository path afterward is resolved through
  the directory capability. Symlinks and reparse points are not followed.

## `rustix` 1.1.4

- Need: Unix `O_NONBLOCK` for the final no-follow open, so a hostile FIFO or
  other special file cannot stall the synchronous adapter before handle-based
  file-type rejection.
- Package: [`rustix`](https://docs.rs/rustix/1.1.4/rustix/), maintained in the
  Bytecode Alliance [`rustix`](https://github.com/bytecodealliance/rustix)
  repository.
- License: Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT.
- Declared MSRV: Rust 1.63, below the workspace baseline.
- Features: target-specific Unix dependency with default features disabled;
  only `fs` and `std` are enabled.
- Code/build shape: no build script or proc macro in the selected package. It
  was already resolved by `cap-std`; the direct edge exposes only the typed
  `O_NONBLOCK` constant.
- Scope: `repowitness-local` on Unix. RepoWitness performs no direct syscall
  and adds no first-party unsafe code.

## `same-file` 1.0.6

- Need: safe cross-platform handle identity for rejecting an existing SQLite
  database hard-linked into the indexed worktree. Canonical paths cannot
  distinguish hard-link aliases.
- Package: [`same-file`](https://docs.rs/same-file/1.0.6/same_file/), maintained
  in [`BurntSushi/same-file`](https://github.com/BurntSushi/same-file).
- License: Unlicense OR MIT.
- Declared MSRV: not published. The exact package builds and tests with the
  pinned workspace toolchain; an MSRV claim is therefore not inferred.
- Features: none.
- Code/build shape: no build script or proc macro. Unix compares device and
  inode metadata; Windows uses its existing `winapi-util` dependency. Unsafe
  platform calls remain in upstream crates, and first-party unsafe remains
  forbidden.
- Scope: `repowitness-local` only. The database handle is compared with
  capability-opened repository paths before source capture; raw paths or file
  identities do not enter domain, persistence, or wire APIs.
- Lockfile impact: the exact package was already resolved through the
  development-only `gix` oracle. This change promotes it to one direct
  production edge without selecting a new package version.

## `rmcp` 2.2.0

- Need: released Rust SDK types and service machinery for the local stdio MCP
  boundary.
- Package: [`rmcp`](https://docs.rs/rmcp/2.2.0/rmcp/), from the official
  [Model Context Protocol Rust SDK](https://github.com/modelcontextprotocol/rust-sdk).
- License: Apache-2.0.
- Declared MSRV: not published. The exact package builds and tests with the
  pinned Rust 1.97.1 toolchain; no lower support claim is inferred.
- Protocol: the server explicitly advertises stable MCP `2025-11-25`.
  Pre-release specification or SDK lines are not selected.
- Features: default features disabled; only `server` and `transport-io` are
  enabled in production. Client support is enabled only for MCP crate tests.
  Macro helpers, HTTP transports, TLS, OAuth, elicitation, task support, and
  in-memory transport features are excluded.
- Code/build shape: pure Rust; selected transitive packages include the
  futures family, Tokio utilities, Chrono, Serde, Schemars, tracing,
  `async-trait`, and `pastey`. Proc macros remain upstream implementation
  details. No MCP SDK type crosses into application, analysis, or domain APIs.
- Scope: `repowitness-mcp` only. RepoWitness supplies its own bounded line
  reader before the SDK transport so a peer cannot make stdio buffer an
  unbounded JSON-RPC line. Tool inputs and encoded result envelopes are checked
  against independent limits.

## `tokio` 1.53.1

- Need: MCP stdio I/O, protocol task supervision, bounded admission, deadline
  timers, cancellation, and ownership of synchronous repository work.
- Package: [`tokio`](https://docs.rs/tokio/1.53.1/tokio/), maintained by
  [Tokio](https://github.com/tokio-rs/tokio).
- License: MIT.
- Declared MSRV: Rust 1.71, below the workspace baseline.
- Features: default features disabled; only `io-std`, `io-util`,
  `rt-multi-thread`, `sync`, and `time` are enabled. Network, filesystem,
  process, signal, macros, and full-feature bundles are excluded.
- Scope: the MCP transport and CLI composition root. The CLI constructs two
  asynchronous worker threads and permits at most six blocking threads; the
  MCP service separately admits at most four concurrent repository
  operations. Synchronous local work uses `spawn_blocking`, receives the
  remaining request deadline and one cooperative cancellation flag, and is
  awaited during cancellation so it is not detached. Tokio does not enter
  domain or analysis APIs.

## `serde` 1.0.229, `serde_json` 1.0.151, and `schemars` 1.2.1

- Need: strict versioned MCP wire DTO serialization, JSON-RPC value handling,
  and exact JSON Schema generation.
- Packages: [`serde`](https://docs.rs/serde/1.0.229/serde/),
  [`serde_json`](https://docs.rs/serde_json/1.0.151/serde_json/), and
  [`schemars`](https://docs.rs/schemars/1.2.1/schemars/).
- Licenses: Serde and `serde_json` are MIT OR Apache-2.0; Schemars is MIT.
- Declared MSRVs: Serde 1.56, `serde_json` 1.71, and Schemars 1.74, all below
  the workspace baseline.
- Features: default features disabled. Serde enables `std` and `derive`;
  `serde_json` enables only `std`; Schemars enables `std` and `derive`.
  Derive support adds the upstream Serde and Schemars proc macros.
- Scope: boundary DTOs in `repowitness-mcp` and JSON handling in the CLI
  installed-binary contract test. Inputs reject unknown fields and are
  revalidated into application/domain boundary values before repository I/O.
  Persisted and domain aggregates remain free of MCP wire derives and schema
  types.

## Verification

The initial introduction passed:

- focused unit tests and Clippy with warnings denied;
- deterministic repeated parser reuse;
- cancellation, deadline, source, node, depth, fact, and name limits;
- syntax-error coverage without hiding valid declarations;
- a real-workspace integration over every tracked Rust source;
- capability-contained source reads covering exact limits, cancellation,
  deadlines, non-UTF-8 Unix names, symlink escape attempts, special files,
  path replacement, and redacted diagnostics;
- `cargo tree` normal/build and feature review;
- `cargo deny --locked check licenses bans advisories sources`.

Release work still needs binary-size and parse-resource measurements on the
pinned Phase 0 corpus, source-vetting ratification, and a grammar-update
procedure tied to artifact invalidation.

## Test-only strict-memory candidates

The strict-memory spike uses exact development-only candidates. Serde is also
an approved production MCP dependency under the review above; the
YAML-specific parser and canonicalizer stack remains test-only and is not
approved for the production dependency graph.

- [`serde`](https://docs.rs/serde/1.0.229/serde/) 1.0.229 supplies the same
  reviewed `std` and `derive` feature set to the test DTO; this does not promote
  any YAML-specific package into production.
- [`serde-saphyr`](https://docs.rs/serde-saphyr/0.0.29/serde_saphyr/) 0.0.29
  supplies only `deserialize`; serialization, includes, filesystem includes,
  properties, validation frameworks, and robotics extensions are disabled.
  The package declares `MIT OR Apache-2.0` but no MSRV. Its deserialization
  graph includes `granit-parser`, `encoding_rs_io`, `annotate-snippets`,
  `num-traits`, `smallvec`, and `ahash`.
- [`granit-parser`](https://docs.rs/granit-parser/0.0.7/granit_parser/) 0.0.7
  is used directly for the raw bounded event preflight because typed
  `serde-saphyr` decoding accepted a custom tag on a string. It is pure Rust,
  forbids unsafe code, has no selected features or build script, declares
  Rust 1.81, and is `MIT OR Apache-2.0`.
- [`serde_json_canonicalizer`](https://docs.rs/serde_json_canonicalizer/0.3.2/serde_json_canonicalizer/)
  0.3.2 supplies RFC 8785 output for the golden test. It is MIT licensed,
  declares no MSRV, and depends on `serde_json` and `ryu-js`.

`encoding_rs` 0.8.35 is transitively required by the test-only YAML decoder
and uses `(Apache-2.0 OR MIT) AND BSD-3-Clause`. The cargo-deny exception
admits `BSD-3-Clause` only for that exact package. Focused tests, Clippy with
warnings denied, the locked feature tree, and cargo-deny advisory, license,
ban, and source checks pass.

The detailed behavior and the reasons not to promote this stack yet are in
the
[strict-memory report](strict-memory-yaml-spike-2026-07-25.md).

## Standalone fuzz dependency

The opt-in fuzz workspace pins
[`libfuzzer-sys`](https://github.com/rust-fuzz/libfuzzer/tree/0.4.13)
0.4.13 to supply the LLVM libFuzzer runtime used by `cargo-fuzz`. It is outside
the shipped workspace, enables its default `link_libfuzzer` feature, depends on
`arbitrary`, and compiles the bundled native runtime through `cc` in a build
script. Its declared license is `(MIT OR Apache-2.0) AND NCSA`; `deny.toml`
therefore admits NCSA only for this exact version.

The fuzz workspace has a separate committed lockfile. Required CI performs a
stable locked compile of its target and applies the advisory, license, ban, and
source policy to that graph. Running a coverage-guided campaign remains an
explicit nightly, opt-in check.
