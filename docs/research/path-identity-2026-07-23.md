# Repository path identity and filesystem authorization

- Status: Implemented and promoted
- Research date: 2026-07-23
- Last updated: 2026-07-28
- Reviewed baselines: Rust 1.97.1, Git 2.55.0, and `gix` 0.86.0 documentation
- Scope: repository-relative identity, host-path conversion, deterministic ordering,
  and filesystem authorization

## Conclusion

RepoWitness should not use `PathBuf`, UTF-8 text, a canonical absolute path, or
`OsStr::as_encoded_bytes()` as durable repository identity.

Use two separate concepts:

1. `RepositoryPath` is a bounded, validated, repository-root-relative Git path
   represented by exact bytes. It preserves case and Unicode bytes, uses `/` as
   its only separator, and has deterministic unsigned-byte ordering.
2. Filesystem authorization is an adapter operation that converts one validated
   repository path beneath an already authorized repository root and returns an
   opened or equivalently verified resource. A host path is not persisted as the
   repository identity.

This split retains Git fidelity on Unix, avoids Rust's intentionally unspecified
cross-platform `OsStr` byte encoding, prevents case or Unicode normalization from
merging distinct Git objects, and gives Windows and symlink behavior explicit
failure modes.

[ADR-0010](../adr/0010-repository-path-identity.md) subsequently accepted this
recommendation as the controlling repository-path decision.

## Primary findings

### Git defines the durable namespace

Git's index format specifies an entry path as relative to the repository top
level, without a leading slash, using `/` as the separator. The `.` , `..`, and
`.git` components, a trailing slash, and NUL are disallowed. Its character
encoding is otherwise undefined. Index entries are ordered by unsigned-byte
comparison without locale or separator special-casing.

That is a better durable identity contract than host path syntax:

- it preserves non-UTF-8 names that Git can represent;
- it provides one deterministic order on every operating system;
- it distinguishes names that a particular worktree may be unable to
  materialize; and
- it does not turn a worktree location into source identity.

Sparse-index directory entries are a storage optimization, not source-file
identities. Discovery must expand them through the selected Git adapter or
report explicit skipped coverage rather than place a trailing-slash placeholder
in a source manifest.

When the Git CLI is the adapter, `git ls-files -z --full-name` emits paths
verbatim, relative to the project root, and terminated by NUL. Without `-z`,
unusual names are quoted according to `core.quotePath` and are not suitable as
identity input.

### Rust host paths are platform-local

Rust documents `Path` and `PathBuf` as wrappers over `OsStr` and `OsString` that
use the local platform's path syntax. Windows recognizes both `\` and `/` as
separators, while Unix recognizes `/`. Several ordinary `Path` operations also
perform lexical normalization such as ignoring repeated separators and
non-leading `.` components.

`OsStr::as_encoded_bytes()` does not solve persistence. Its encoding is
unspecified and platform-specific; non-UTF-8 output is only comparable within
the same Rust version built for the same target. Rust explicitly warns that
storing or transferring those bytes will likely be incompatible.

The specified native conversions are different:

- Unix `OsStrExt::as_bytes()` exposes the underlying byte view.
- Windows `OsStrExt::encode_wide()` losslessly exposes potentially ill-formed
  UTF-16 code units.

Neither representation is a portable Git repository-path encoding. Host
conversion must therefore remain a fallible adapter concern.

### Case and Unicode are reconciliation inputs, not identity normalization

Git's `core.ignoreCase` enables workarounds for case-insensitive filesystems
while Git continues to remember the index spelling. `core.precomposeUnicode`
controls macOS worktree conversion. `core.protectHFS` and `core.protectNTFS`
reject names that are unsafe or alias `.git` on those filesystems.

RepoWitness should preserve exact repository bytes and record every relevant
Git/path-policy input in the source-snapshot configuration identity. It must
not lowercase, Unicode-normalize, or silently select one side of a host
collision. If two repository paths cannot coexist on the current host, both
logical identities remain distinct and materialization returns an explicit
collision or unsupported-path diagnostic.

### A backslash is not a portable separator

Git reserves `/` as the repository separator. A backslash can be an ordinary
filename byte on Unix, but Windows path parsing treats it as a separator.
Rejecting it from the repository namespace would lose valid Git identity;
passing it through `PathBuf::from()` on Windows could change the path's
structure.

The safe rule is:

- preserve `\` in `RepositoryPath`;
- split only on `/`;
- append validated components individually during host conversion; and
- reject a component that the target filesystem cannot materialize
  unambiguously.

The same rule prevents a repository component such as `C:` or a UNC-looking
byte sequence from becoming a Windows prefix. Repository bytes are never
parsed wholesale as a host path.

### Authorization must cover the open, not only a string

Lexical validation rejects absolute paths, empty components, `.` and `..`
before filesystem access. That is necessary but not sufficient: a symlink,
mount, reparse point, or concurrent rename can change where a later open lands.

Discovery should not follow symlinks by default. If policy permits following
them, the filesystem adapter must resolve and authorize the actual target under
an allowed root. Prefer directory-handle-relative or capability-style access
whose containment applies to the open itself. On Linux, `openat2` exposes
`RESOLVE_BENEATH` and `RESOLVE_NO_SYMLINKS`; on Windows, reparse points require
handle-level policy. The `cap-std` project demonstrates a Rust API based on an
open directory capability, but dependency selection requires its own review.

Calling `canonicalize()`, checking a prefix, discarding the result, and later
opening the original path is not an adequate security boundary because the
filesystem may change between those operations.

## Recommended contract

### `RepositoryPath`

The domain value should:

- own a non-empty byte sequence;
- accept explicit maximum byte and component counts before allocating or
  cloning untrusted input;
- require repository-root-relative Git form;
- use ASCII `/` as the only component separator;
- reject NUL, a leading or trailing `/`, empty components, and the exact
  components `.`, `..`, and `.git`;
- preserve every other byte exactly, including case, Unicode form, control
  characters, and `\`;
- order values by unsigned-byte lexicographic comparison; and
- expose raw bytes for canonical hashing while keeping escaped display
  separate.

The type describes a source-file identity, so it does not admit the
trailing-slash sparse-index directory placeholder.

SQLite may store this identity as a BLOB. Textual wire and Git-memory schemas
must use a versioned, tagged, lossless byte encoding plus optional display text;
lossy display text is never decoded back into identity. The exact textual
encoding belongs to the boundary-schema decision.

### Capture

- Tracked paths come from byte-preserving index/tree APIs or NUL-delimited Git
  output and are validated before domain construction.
- Git-aware discovery of untracked paths should prefer the same byte-preserving
  boundary, for example NUL-delimited `git ls-files --others`.
- Direct Unix enumeration may convert components with
  `std::os::unix::ffi::OsStrExt::as_bytes()`.
- Phase 0 Windows materialization should accept valid UTF-8 repository
  components that also pass Windows and Git protection checks. Other byte
  identities remain representable but return a typed unsupported-encoding
  diagnostic until a Git-compatible byte-to-wide conversion is proven by
  differential fixtures.
- Git subprocess calls never interpolate a path into a shell. Operations that
  accept caller-provided path sets use a literal NUL-delimited pathspec input
  when the command supports it.

### Filesystem access

The local adapter should receive:

- an already authorized root handle or capability;
- a validated `RepositoryPath`;
- explicit symlink/reparse, mount, file-type, and size policy; and
- deadline/cancellation state.

It should return opened content or an owned verified handle with file metadata,
not an unchecked absolute path for a later caller to reopen. Diagnostics must
distinguish invalid identity, unsupported encoding, host collision, denied
symlink/reparse traversal, scope escape, changed-during-read, unsupported file
type, and resource limit.

## Adversarial fixture matrix

| Case | Repository identity | Host behavior |
|---|---|---|
| `src/lib.rs` | Accept | Materialize beneath the authorized root |
| empty, leading `/`, or trailing `/` | Reject | No filesystem access |
| `a//b`, `a/./b`, or `a/../b` | Reject | No filesystem access |
| `.git/config` or `a/.git/config` | Reject | No filesystem access |
| NUL in any component | Reject | No filesystem access |
| newline or tab in a component | Preserve | Escape only for display; use NUL-delimited Git I/O |
| invalid UTF-8 bytes | Preserve | Unix round-trips; Phase 0 Windows reports unsupported encoding |
| `A.rs` and `a.rs` | Preserve as distinct | Report host collision where both cannot coexist |
| NFC and NFD spellings | Preserve as distinct | Report host collision or adapter mapping explicitly |
| backslash in a component | Preserve | Unix may materialize; Windows rejects ambiguous conversion |
| `C:` or UNC-looking component | Preserve as bytes | Never parse as a prefix; reject if the host forbids it |
| symlink at any component | Path remains valid | Default deny; follow only through explicit contained-open policy |
| sparse-index directory entry | Not a source-file path | Expand or report skipped coverage |
| path over byte/component budget | Reject with limit diagnostic | No filesystem access |
| rename during discovery/read | Identity remains unchanged | Retry within budget or report changed input |

Property tests should generate arbitrary byte components, separators, limits,
case variants, and Unicode forms. Cross-platform integration tests should use
real Git repositories and worktrees rather than assuming that a mocked
`PathBuf` reproduces filesystem behavior.

## Initial external-worktree exercise

An opt-in local-adapter probe enumerated cached and untracked non-ignored paths
with NUL-delimited `git ls-files` output and validated each record through
`RepositoryPath`. It used a bounded deadline, captured-output limit, path-count
limit, path-byte limit, and component-count limit, and did not log path
contents.

Clean locally configured external worktrees passed exact-byte round-trip
validation. Their identities, paths, revisions, and per-repository measurements
are intentionally omitted from this public research record. This private smoke
test is not evidence for final default limits, arbitrary-byte filenames,
Windows host conversion, contained filesystem opens, sparse indexes,
submodules, or a production `gix` versus Git CLI choice.

The bounded sanitized-Git implementation was subsequently moved from the
test-only probe into the local adapter and exposed through the explicitly
non-indexing `repowitness inspect-paths` diagnostic. Unit tests cover canonical
ordering, aggregate counts, malformed and duplicate output, byte and path
bounds, subprocess and parse cancellation/deadlines, inherited stdout writers,
nested-worktree resolution, hostile `core.worktree` pinning, redacted failures,
static symlink/special-file worktree-marker rejection, and the command's
critical argument, repository-discovery, configuration, and trace-environment
sanitization. Worktree-marker inspection uses Rust
[`symlink_metadata`](https://doc.rust-lang.org/stable/std/fs/fn.symlink_metadata.html)
so a static marker symlink is classified without following it. Black-box CLI
tests require
unsupported `index` invocations to return a nonzero unavailable status instead
of silently succeeding. This promotion improves spike reuse and test fidelity;
it does not select Git CLI over `gix`, authorize source-file opens, or satisfy
the remaining production-adapter fixtures below.

## `gix` versus sanitized Git CLI differential spike

On 2026-07-23, RepoWitness added an exact-pinned, dev-only `gix` 0.86.0
differential oracle with default features disabled and only the `index`,
`sha1`, and `sha256` features enabled. It opens repositories with isolated
options and refuses untrusted repositories. The production-shaped Git CLI path
continues to disable interactive behavior and ambient system/global
configuration, clear ambient repository-discovery controls, pin the containing
worktree with `--work-tree`, avoid a shell, bound time and output, and validate
every NUL-delimited result into `RepositoryPath`.

The reproducible fixture matrix produced these results:

| Fixture | Result |
|---|---|
| ordinary tracked, untracked, and ignored paths | `gix` index paths equal the CLI cached scope; CLI inclusion of untracked non-ignored paths is explicit |
| non-UTF-8 Unix path | exact index bytes agree |
| SHA-256 repository | paths agree when `gix` SHA-256 support is explicitly enabled |
| three-stage conflicted index | raw index stages repeat the path; CLI `--deduplicate` and domain sorting/deduplication produce one identity |
| sparse index | raw `gix` exposes a trailing-slash sparse-directory placeholder; CLI without `--sparse` expands it to logical file paths |
| gitlink/submodule entry | path bytes agree and `gix` reports the gitlink mode; a path-only CLI result cannot classify it as non-regular |
| linked worktree | cached path bytes agree when opening through the linked worktree |
| nested path inside a worktree | discovery resolves the containing worktree and returns the complete root-relative path set |
| case-colliding index entries | `Case.rs` and `case.rs` remain two exact identities without requiring host materialization |
| hostile included config with an `fsmonitor` command | neither isolated `gix` nor the sanitized CLI executes the command |
| hostile local `core.worktree` | the CLI pins the requested containing worktree and does not enumerate the configured outside directory |
| hostile local `core.excludesFile` | the CLI overrides the outside excludes path and retains the requested untracked path set |
| symlinked or special-file `.git` marker | discovery fails before Git can read a redirected index; ordinary `.git` directories and linked-worktree `.git` files remain supported |

The sparse-index result is a contract boundary: raw index entries cannot be
copied directly into source manifests. A `gix` production adapter would need
bounded sparse expansion or explicit unresolved coverage. The CLI adapter must
retain `--deduplicate`, because an unresolved merge can otherwise emit the same
repository identity once per index stage. Both adapters must also return entry
type or mode with each path: the path-only diagnostic sees a gitlink, but a
source manifest must not treat that submodule boundary as a regular file.

The ignored real-repository differential test also passed against clean,
locally configured external worktrees. Their identities and measurements
remain local; the smoke result does not replace reproducible adversarial
fixtures.

The minimal umbrella `gix` feature set still resolves 145 packages in
all-target Cargo metadata. Its graph includes object-database and
protocol/transport infrastructure that this index-only spike does not call,
plus proc macros, build scripts, memory mapping, temporary-file support, and a
pure-Rust zlib implementation. `gix` 0.86.0 declares Rust 1.85 and
MIT-or-Apache-2.0 licensing, so it fits the pinned workspace toolchain and
project license policy. Because it is dev-only, it has no shipped binary-size
impact; it does increase test-build and supply-chain surface.

The dependency remains an exact-pinned dev-only differential oracle.
`cargo-deny` checks development/build dependency licenses and development
duplicate versions. Exact exceptions cover the Zlib license in `foldhash`
0.2.0 and `zlib-rs` 0.6.6 and the Unicode-3.0 term in `unicode-ident` 1.0.24.
Those exceptions must be reviewed or removed whenever `gix` is upgraded,
promoted to production, or removed.

### Active cancellation and performance follow-up

The Phase 0 production-adapter comparison completed on 2026-07-28. The pinned
`gix` 0.86.0 index path opens the repository and calls
`Repository::index_or_empty`, which reaches `gix_index::File::at`. These index
open and decode calls do not accept a caller-owned cancellation flag or
deadline. A regression sentinel sets `gix`'s process-global interrupt before
index loading and confirms that this path still completes. The global flag is
therefore not an active-work cancellation boundary for index discovery.

Running the operation on an abandonable thread would let the caller return but
would leave CPU, memory, mappings, and file handles live without a completion
bound. Running `gix` in a killable helper process would restore a hard
cancellation boundary but also reproduce the subprocess ownership model
already provided by the sanitized Git adapter.

An opt-in release probe built a deterministic synthetic SHA-1 index with
50,000 sorted cached paths and compared exact path results over 20 samples.
Three complete probe runs on the development workstation produced:

| Adapter state | First observation | Median range | p95 range |
|---|---:|---:|---:|
| fresh isolated `gix` repository per query | 7.829–9.454 ms | 7.068–8.358 ms | 7.132–8.845 ms |
| retained isolated `gix` repository | 6.931–7.818 ms | 0.598–0.672 ms | 0.613–0.687 ms |
| fresh sanitized Git CLI process | 35.131–43.235 ms | 28.616–42.826 ms | 34.930–49.168 ms |

All 50,000 exact cached paths agreed in every sample. The probe excludes build
time, does not clear operating-system page caches, and measures cached index
enumeration rather than untracked discovery, sparse expansion, source reads,
or complete indexing. The values are comparative evidence, not ratified
resource budgets.

`gix` is materially faster for this narrow workload, especially with a
retained repository. It is not promoted because active work cannot meet
RepoWitness's per-operation cancellation and deadline contract, its raw sparse
and gitlink semantics still need adaptation, and its 145-package all-target
dependency graph remains much larger than the invoked index surface.
Sanitized Git remains the Phase 0 production discovery adapter; exact-pinned
`gix` remains a development differential oracle.

Reproduce the synthetic probe with:

```text
cargo test --release -p repowitness-local \
  gix_and_sanitized_git_report_cold_and_warm_performance \
  --locked -- --ignored --nocapture
```

The fail-closed policy is now covered by an actual nested-submodule fixture and
by sparse and gitlink index-mode transitions between the two source-state
captures. A mode present at the initial capture retains its specific
unsupported-scope diagnostic; a mode introduced during indexing reports a
concurrent source change and cannot reach publication. Retain these
regressions, and complete Windows conversion and contained-open fixtures,
before broadening the supported scope or reconsidering the adapter.

## Alternatives rejected

### UTF-8-only durable paths everywhere

This is convenient for JSON and YAML but loses valid Unix/Git identities and
encourages silent lossy conversion. A supported Windows materialization subset
does not require narrowing the durable repository namespace.

### `PathBuf` or `OsString` as the domain identity

These values have target-local parsing and representation. They cannot define
one portable ordering or canonical persisted encoding.

### `OsStr::as_encoded_bytes()` persistence

Rust explicitly limits non-UTF-8 comparability to the same toolchain and target,
so the result is unsuitable for database, wire, or Git-memory identity.

### Case folding or Unicode normalization

This hides repository distinctions and can merge evidence from different Git
objects. Host aliases must be diagnosed, not silently relinked.

### Canonical absolute paths as identity

Absolute paths leak personal host details, change when a worktree moves, and
resolve symlinks rather than preserving repository spelling.

### Canonicalize-and-prefix-check before a later open

This leaves a time-of-check/time-of-use race. Authorization must be coupled to
the actual open or verified handle.

## Implementation follow-up and remaining spikes

Accepted [ADR-0010](../adr/0010-repository-path-identity.md) and
[ADR-0011](../adr/0011-repository-path-text-encoding.md) now govern the
implemented byte identity and `rwp1:h:` boundary encoding. On Unix, production
source reads use `cap-std` directory capabilities, no-follow opens, regular
file checks, exact byte limits, and final path/content revalidation.
Adversarial tests cover non-UTF-8 names, symlinks, special files, path
replacement, concurrent mutation, cancellation, deadlines, and redacted
diagnostics.

1. Completed for Phase 0 on 2026-07-28: the active-work cancellation and
   synthetic 50,000-path performance comparison retains sanitized Git in
   production and exact-pinned `gix` as a development oracle.
2. Completed on 2026-07-28: an actual two-level submodule hierarchy fails
   closed at each gitlink boundary without recursive indexing, and concurrent
   sparse/gitlink index-mode changes fail as concurrent source changes.
3. Prove the supported Windows Git-byte-to-wide conversion against Git for
   valid UTF-8, reserved names, trailing dots/spaces, device names, reparse
   points, and case/Unicode aliases.
4. Decide and test the Windows contained-open adapter. Retain the same
   opened-resource authorization and final revalidation semantics as the Unix
   `cap-std` implementation.
5. Ratify configurable path byte/component defaults from the Phase 0 corpus
   and adversarial fixtures; retain hard ceilings regardless of configuration.

## Primary sources

- Rust 1.97.1 [`std::path`](https://doc.rust-lang.org/std/path/) and
  [`Component`](https://doc.rust-lang.org/std/path/enum.Component.html)
- Rust 1.97.1
  [`OsStr::as_encoded_bytes`](https://doc.rust-lang.org/std/ffi/os_str/struct.OsStr.html#method.as_encoded_bytes)
- Rust 1.97.1 Unix
  [`OsStrExt`](https://doc.rust-lang.org/std/os/unix/ffi/trait.OsStrExt.html)
  and Windows
  [`OsStrExt`](https://doc.rust-lang.org/std/os/windows/ffi/trait.OsStrExt.html)
- Git 2.55.0 [index format](https://git-scm.com/docs/gitformat-index),
  [`git ls-files`](https://git-scm.com/docs/git-ls-files),
  [`git config`](https://git-scm.com/docs/git-config), and
  [`git checkout`](https://git-scm.com/docs/git-checkout)
- `gix` 0.86.0
  [crate documentation](https://docs.rs/gix/0.86.0/gix/) and
  [`RelativePath`](https://docs.rs/gix/0.86.0/gix/path/struct.RelativePath.html)
- Linux [`openat2(2)`](https://man7.org/linux/man-pages/man2/openat2.2.html)
- Microsoft
  [`CreateFileW`](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew)
- `cap-std` 4.0.2
  [crate documentation](https://docs.rs/cap-std/4.0.2/cap_std/)
