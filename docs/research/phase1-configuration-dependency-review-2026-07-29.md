# Phase 1 local-boundary dependency review

- Status: Active
- Reviewed: 2026-07-29
- Scope: Strict bounded TOML configuration and manifest admission, stable
  file identity, and operating-system identity generation

## `toml` 1.1.4+spec-1.1.0

- Need: ADR-0025 requires standards-based TOML decoding into a strict wire DTO,
  including duplicate-key and type rejection, before application values are
  constructed. A custom parser would add avoidable ambiguity and hostile-input
  risk.
- Package: [`toml`](https://docs.rs/toml/1.1.4+spec-1.1.0/toml/), maintained in
  the official [`toml-rs`](https://github.com/toml-rs/toml) repository.
- License: MIT OR Apache-2.0.
- Declared MSRV: Rust 1.85, below RepoWitness's Rust 1.97.1 baseline.
- Pin: workspace requirement `=1.1.4`; the lockfile records the package's
  `1.1.4+spec-1.1.0` build metadata.
- Features: default features are disabled. Only `parse`, `serde`, and `std` are
  enabled. The `display`, `debug`, `preserve_order`, `fast_hash`, and
  `unbounded` features are disabled. `toml_writer` is consequently absent from
  the active `repowitness-local` dependency graph.
- Runtime shape: safe Rust parser and Serde adapter. The active direct
  transitive packages are `toml_parser`, `toml_datetime`, `serde_spanned`, and
  `winnow`; there is no native library, build script, proc macro, network I/O,
  or executable surface.
- Scope: `repowitness-local` only. TOML and Serde DTO types remain private to
  the local adapter. Application configuration and policy types have no TOML,
  Serde, filesystem, SQLite, Git, Tokio, or MCP dependency.
- Bounds: input is rejected above 65,536 bytes before UTF-8 validation or TOML
  parsing. Every scalar, collection, numeric value, enum, and layer is
  independently bounded. Parser errors are collapsed into stable redacted
  categories and never retained as an error source.
- Validation: focused tests cover the inclusive file boundary, invalid UTF-8,
  duplicate and unknown keys, unsupported versions, wrong types, every numeric
  range, excessive or duplicate arrays, unsupported sensitive fields,
  profile-selection trust, redacted errors, and conversion into the pure
  monotonic resolver.

The lockfile, `cargo tree -e features`, `cargo deny`, Clippy, and the locked test
matrix remain authoritative for the resolved package graph.

## `cap-std` and `cap-fs-ext` 4.0.2

- Need: explicitly selected configuration and connected-workspace manifest
  paths must be walked one component at a time without following symbolic
  links or reparse points. Retaining an opened parent directory capability
  also lets the connected-workspace coordinator revalidate the exact manifest
  authority before source access and at its final publication fence.
- Packages: [`cap-std`](https://docs.rs/cap-std/4.0.2/cap_std/) and
  [`cap-fs-ext`](https://docs.rs/cap-fs-ext/4.0.2/cap_fs_ext/), maintained by
  the Bytecode Alliance.
- License: Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT.
- Pin: both workspace requirements are exact `=4.0.2` pins.
- Features: `cap-std` disables its declared default features;
  `cap-fs-ext` disables its declared defaults and enables only `std`. The
  resolved extension feature activates the matching `cap-std` support needed
  by `DirExt`.
- Runtime shape: safe Rust capability wrappers over platform filesystem
  primitives. They add no network, process, parser, serialization, plugin, or
  executable surface.
- Scope: `repowitness-local` only. Capability objects remain adapter-private;
  domain and application types do not depend on host filesystem handles.
- Validation: exact-limit, one-over-limit, empty-file, option-shaped,
  absolute/relative, `.`/`..`, final and ancestor symlink, hard-link, FIFO,
  device, directory, in-place mutation, final replacement, ancestor
  replacement, and redacted-debug fixtures exercise the admitted-file seam.

## `same-file` 1.0.6

- Need: byte length and modification time cannot prove that a second path walk
  still names the opened file or directory. Stable platform file handles bind
  the admission and revalidation walks to the same object.
- Package: [`same-file`](https://docs.rs/same-file/1.0.6/same_file/), maintained
  in the upstream `BurntSushi/same-file` repository.
- License: Unlicense OR MIT.
- Pin: exact workspace requirement `=1.0.6`.
- Runtime shape: a small safe Rust platform abstraction over file identity.
  It has no network, parser, serialization, proc-macro, or executable surface.
- Scope and bounds: `repowitness-local` retains at most the fixed 256-component
  control-path identity chain. Handles and paths are never persisted or
  rendered in normal output.

## `getrandom` 0.3.4

- Need: callers need paste-ready repository, connected-workspace, and
  source-slot IDs without inventing entropy or exposing a seed interface.
- Package: [`getrandom`](https://docs.rs/getrandom/0.3.4/getrandom/), maintained
  in the upstream `rust-random/getrandom` repository.
- License: MIT OR Apache-2.0.
- Declared MSRV: Rust 1.63, below RepoWitness's Rust 1.97.1 baseline.
- Pin and features: exact workspace requirement `=0.3.4`, with default
  features disabled. No fallback PRNG or caller-selected backend is enabled.
- Runtime shape: one operating-system entropy call fills a fixed 32-byte
  buffer. The crate's build script selects supported platform configuration;
  it adds no proc macro, parser, serialization, network, plugin, or executable
  surface.
- Failure contract: incomplete or unavailable operating-system entropy returns
  one generic error and produces no identity. Injected deterministic tests
  prove every allow-listed tag, partial-failure behavior, and redacted debug
  output without making production randomness injectable from the CLI.
