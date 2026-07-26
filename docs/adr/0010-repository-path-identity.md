# ADR-0010: Separate repository path identity from filesystem authorization

- Status: Accepted
- Date: 2026-07-23
- Owners: Project maintainers
- Scope: Source manifests, evidence paths, persistence, host conversion, and
  filesystem access

## Context

RepoWitness must identify the same Git path deterministically across Unix,
Windows, databases, wire protocols, and Git-tracked memory. It must also read
host files without path traversal, symlink/reparse escape, case collision, or
lossy encoding.

Rust `Path` and `OsStr` intentionally follow target-local syntax and encoding.
Git instead stores repository-root-relative path bytes with `/` separators and
unsigned-byte ordering. Conflating those namespaces would make snapshot
identity platform-dependent and could turn a valid Git filename into a
different host path.

The dated
[path-identity research](../research/path-identity-2026-07-23.md) records the
primary-source findings and adversarial matrix.

## Decision

RepoWitness uses separate repository identity and filesystem authorization
contracts.

### Repository identity

`RepositoryPath` is a domain newtype over owned exact bytes.

Construction:

- requires explicit byte and component limits and rejects over-limit input
  before cloning or allocating from it;
- requires a non-empty repository-root-relative Git path;
- uses ASCII `/` as the only separator;
- rejects NUL, leading or trailing `/`, empty components, and exact `.`, `..`,
  or `.git` components; and
- preserves all other bytes exactly, including case, Unicode form, control
  characters, and backslash.

Equality and hashing use the exact bytes. Ordering is unsigned-byte
lexicographic order, matching Git index name ordering. The type represents a
source-file path and does not accept sparse-index trailing-slash directory
placeholders.

Case folding, Unicode normalization, lossy conversion, and host
canonicalization never change this identity. Host aliases and unmaterializable
names are explicit diagnostics.

SQLite persists the value as bytes. Wire and Git-memory DTOs use a versioned,
tagged, lossless encoding with optional display text; display text never
reconstructs identity. The boundary-schema decision selects the exact textual
encoding.

### Capture and host conversion

Tracked paths enter through byte-preserving Git index/tree APIs or
NUL-delimited Git output. Git-aware untracked discovery uses the same kind of
boundary where possible.

Direct Unix conversion uses the specified Unix `OsStr` byte APIs. RepoWitness
does not persist `OsStr::as_encoded_bytes()`.

Phase 0 Windows materialization supports valid UTF-8 repository components that
pass Windows and Git protection checks. Other repository bytes remain valid
logical identities but return a typed unsupported-path-encoding diagnostic.
The local adapter:

1. splits only on `/`;
2. validates and appends each component individually beneath an authorized
   root;
3. never parses the complete repository byte string as a Windows path; and
4. reports reserved-name, reparse, case, Unicode, or other host collisions
   without merging identities.

Relevant Git and path-policy settings, including case and Unicode behavior,
belong in source-snapshot configuration identity.

Git subprocess calls never use a shell. Commands that consume untrusted path
sets use literal NUL-delimited pathspec input where supported, together with
the existing sanitized environment, deadline, cancellation, and output bounds.

### Filesystem authorization

Repository-path validation does not itself authorize filesystem access.

The local adapter starts from an already authorized repository-root handle or
capability and returns opened content or an owned verified handle. It does not
return an unchecked absolute path for another layer to reopen.

Symlink and reparse following is disabled by default. When enabled, the actual
open must remain beneath an allowed root under the resolved policy. Prefer
directory-handle-relative, capability-style, or equivalent platform operations
that bind containment to the open. A canonicalize-and-prefix-check followed by
a later open of the original path is insufficient.

The adapter reports invalid identity, unsupported encoding, host collision,
denied link/reparse traversal, scope escape, changed-during-read, unsupported
file type, cancellation, deadline, and resource limits as distinct outcomes.
Skipped or unsupported paths contribute to source-snapshot coverage.

## Alternatives considered

### UTF-8 strings for every path

Simplifies text schemas but cannot represent all Unix/Git paths. Silent lossy
conversion would corrupt evidence identity.

### `PathBuf` or `OsString` in domain values

Both use target-local syntax and representation, so persistence and ordering
would vary by operating system and toolchain.

### Persist `OsStr::as_encoded_bytes()`

Rust documents the non-UTF-8 encoding as unspecified and only comparable within
the same Rust version and target.

### Fold case or normalize Unicode

Makes some host lookups easier but can collapse distinct Git objects and attach
evidence or memory to the wrong file.

### Reject backslash from repository identity

Would discard valid Unix Git filenames. It is preserved logically and rejected
only when a host conversion cannot represent it safely.

### Use canonical absolute paths as identity

Leaks host-specific locations, changes when worktrees move, and confuses
symlink targets with repository spelling.

### Canonicalize, check a prefix, then reopen by name

Leaves a time-of-check/time-of-use race around symlink, reparse, mount, or rename
changes.

## Consequences

### Positive

- Snapshot and evidence identity is stable across supported hosts.
- Non-UTF-8 Git paths remain representable without lossy display conversion.
- Deterministic sorting matches Git's documented unsigned-byte order.
- Windows, case, and Unicode limitations fail explicitly rather than
  contaminating identity.
- Traversal validation and filesystem authorization become independently
  testable.
- Absolute personal paths stay out of persisted identities and default logs.

### Negative and risks

- Boundary DTOs need an explicit binary-path representation.
- Some repositories valid on Unix cannot be fully materialized on Windows.
- Case and Unicode collisions require diagnostics and coverage accounting.
- Race-resistant contained opens need platform integration and may justify a
  reviewed dependency or a narrow audited boundary.
- Limits must be threaded through constructors, discovery, persistence, and
  decoding.
- Byte-oriented paths are less convenient for human-readable configuration and
  error messages.

## Validation

- Unit tests for every accepted and rejected component form, exact-byte
  equality, Git-compatible ordering, bounds, and escaped display.
- Property tests over arbitrary bytes, separators, component counts, case, and
  Unicode forms; successful construction must round-trip exact bytes.
- Golden persistence and wire round trips for UTF-8, control-character, and
  non-UTF-8 identities.
- Unix integration tests for arbitrary non-NUL bytes and symlinks at every
  component.
- Windows integration tests for drive/UNC-looking bytes, separators, device and
  reserved names, trailing dots/spaces, case collisions, Unicode aliases,
  reparse points, and unsupported encodings.
- Git differential tests for index/tree paths, `ls-files -z --full-name`,
  tracked and untracked files, sparse index, worktrees, submodules, and
  `core.ignoreCase`, `core.precomposeUnicode`, `core.protectHFS`, and
  `core.protectNTFS`.
- Adversarial rename/symlink-swap tests must prove a read cannot escape the
  authorized root or otherwise return an explicit unsupported-platform
  diagnostic.
- Clean and incremental manifests must contain the same path bytes and order on
  the same source snapshot.
- Fuzz domain decoding, textual boundary decoding, Git output parsing, and host
  conversion. Retain every minimized regression input.

## Implementation sequence

1. Completed: implement and property-test the pure `RepositoryPath` domain type without
   filesystem or Git dependencies.
2. Completed: select the tagged textual boundary encoding and add golden DTO fixtures.
3. Implemented for Phase 0 Linux: use sanitized Git in production and retain
   `gix` as a differential oracle; active-work cancellation, performance, and
   recursive-submodule comparisons remain.
4. Completed on Unix: capability-contained no-follow reads authorize the
   opened file and revalidate identity/content. Windows conversion and
   reparse-point containment remain before Windows production support.
5. Implemented for the current fail-closed scope: integrate path diagnostics,
   explicit skipped coverage, and source-state fencing into the Phase 0
   clean-versus-incremental fixture.

The domain type, canonical text encoding, sanitized Git discovery, Unix host
conversion/containment, SQLite byte persistence, CLI/MCP DTOs, and
real-repository probes are implemented. Sparse worktrees, gitlinks, and
recursive submodules fail closed until broader scope is explicitly accepted.

## Open questions

- Which reviewed Windows adapter provides the contained-open contract matching
  the implemented Unix `cap-std` boundary?
- What hard byte/component ceilings and lower default limits pass the Phase 0
  corpus and adversarial benchmarks?
- Does a later Windows adapter need to support Git identities that cannot be
  decoded as valid UTF-8, and which Git-compatible conversion proves that
  behavior?

## Follow-up decisions

[ADR-0011](0011-repository-path-text-encoding.md) selects the tagged lossless
text encoding used by textual repository-path boundaries.

## Implementation status

Implemented for supported Unix Phase 0 repository discovery, source reads,
SQLite persistence, CLI output, and MCP retrieval. Windows production support
and broader sparse/submodule scope remain explicit follow-up work.

## Supersession

None.
