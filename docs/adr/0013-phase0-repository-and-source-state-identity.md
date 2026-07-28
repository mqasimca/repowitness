# ADR-0013: Require explicit repository identity and canonical Git/worktree receipts

- Status: Accepted
- Date: 2026-07-25
- Owners: Project maintainers
- Scope: Phase 0 repository identity, Git state, worktree state, and snapshot
  stability

## Context

The implemented Phase 0 index can discover, read, analyze, persist, activate,
and retrieve Rust facts, but production CLI/MCP composition remains blocked on
constructing `RepositoryIdentityDigest`, `GitStateDigest`, and
`WorktreeStateDigest`.

Git exposes concrete object IDs, object format, shallow state, status, and
worktree layout, but no documented portable repository UUID. Deriving identity
from a path, remote, root commit, branch, or current commit would silently
merge or split repositories under ordinary operations. The
[source-identity research](../research/source-identity-2026-07-25.md) records
the primary-source findings and proposed encodings.

[ADR-0005](0005-git-dag-temporal-memory.md) already makes branch names selectors
rather than temporal identity. [ADR-0006](0006-immutable-index-generations.md)
requires exact snapshots and failure on concurrent source changes.
[ADR-0010](0010-repository-path-identity.md) forbids host paths from becoming
repository identity.

## Decision

### Repository identity is explicit

Phase 0 requires a caller/configuration-supplied opaque 32-byte repository ID.
Its canonical textual boundary is `rwi1:h:` plus exactly 64 uppercase RFC 4648
Base16 characters.

The application boundary validates and decodes that value before constructing
`RepositoryIdentityDigest`. CLI/MCP composition fails with an explicit missing
or invalid identity diagnostic when it is absent.

RepoWitness does not automatically derive repository identity from:

- an absolute, canonical, Git, or worktree path;
- a remote URL;
- a branch, ref, or current commit;
- a root-commit set; or
- filesystem metadata.

A future explicit `init` workflow may generate and store the ID in user-state
configuration. It does not silently modify the target repository. Linked
worktrees intended to represent the same logical repository use the same
configured ID.

### Git-state receipt is concrete and versioned

`GitStateDigest` version 1 is SHA-256 over a domain-separated canonical
encoding of:

- encoding version;
- Git object format;
- `HEAD` state as unborn or a concrete commit;
- the decoded raw commit object ID when present; and
- shallow-repository state.

Only explicitly supported object formats are admitted. Branch and symbolic-ref
names are excluded. A detached and symbolic `HEAD` resolving to the same commit
therefore have the same Git-state receipt.

Shallow state participates because ancestry coverage differs even when `HEAD`
is equal. Queries that need unavailable ancestry return indeterminate under
ADR-0005.

### Worktree receipt is scoped and canonical

`WorktreeStateDigest` version 1 is SHA-256 over:

- a domain and encoding version;
- a fixed status-profile version;
- validated, canonical, exact-path-ordered Git porcelain-v2 status records;
  and
- the exact Rust source-manifest digest.

The local adapter invokes sanitized Git without a shell using
`status --porcelain=v2 -z --untracked-files=all --ignore-submodules=none
--no-renames`. It does not request optional branch, ahead/behind, or stash
headers.

The parser validates record category, field count, status/mode/object fields,
and every repository path; retains ordinary, unmerged, and untracked states;
rejects duplicate paths and unknown mandatory records; and hashes a
length-prefixed structured encoding rather than raw subprocess bytes.

This receipt identifies the indexed Rust source/configuration scope, not every
byte in every ignored or non-indexed file. Exact source bytes come from the
manifest. Semantics-affecting policy/configuration remains in
`ConfigurationDigest`, and skipped scope remains explicit coverage.

Phase 0 fails closed on sparse-worktree or recursive-submodule scope until
their capture and coverage policy is accepted and implemented.

### Capture uses a stability fence

The local adapter captures Git/worktree receipts before discovery and again
after the existing final source path/content revalidation. Publication
requires:

- both Git receipts are equal;
- both worktree receipts are equal;
- the prepared source manifest matches the final receipt;
- cancellation and deadline remain active; and
- the compare-and-set source epoch remains current.

Any mismatch returns a stable concurrent-source-change diagnostic and produces
no generation activation. The adapter may retry only within an explicit
bounded policy.

Every subprocess uses the existing sanitized non-interactive environment,
bounded output, absolute deadline, cancellation, and redacted diagnostics.

## Alternatives considered

### Hash the canonical repository path

Easy to compute locally, but moves change identity, aliases can split one
repository, linked worktrees are misclassified, and host locations may leak.

### Hash a remote URL

Remote URLs can be missing, renamed, credential-bearing, transport-specific,
or intentionally different between equivalent clones. Fork and mirror
semantics remain ambiguous.

### Hash root commits

Forks intentionally share roots, history rewrites change them, multiple roots
exist, and shallow clones make the set incomplete.

### Use the current commit as repository identity

A commit is the concrete Git-state component. Treating it as repository
identity changes the repository ID on every commit and cannot represent unborn
or dirty worktrees correctly.

### Generate a hidden local ID automatically

This minimizes initial user input but silently assigns different identities to
separate clones and requires a secure, portable, conflict-aware sharing
contract that the current configuration and memory schemas do not yet define.
An explicit `init` workflow may add this later.

### Hash raw porcelain output

It avoids parser work but makes subprocess formatting accidental identity,
cannot canonicalize order or duplicates, and weakens validation of hostile
paths and fields.

### Use only the source manifest

The manifest proves indexed content but omits concrete revision/object format,
shallow-history capability, conflict/index state, and conservative worktree
scope required by evidence and temporal-memory decisions.

## Consequences

### Positive

- Repository identity never depends on a personal path, mutable remote, or
  moving Git selector.
- SHA-1, SHA-256, unborn, detached, shallow, and linked-worktree states are
  explicit.
- Exact source bytes remain the correctness anchor.
- Concurrent checkout/index/worktree changes fail before activation.
- Status and subprocess data are validated and canonically hashed rather than
  persisted or logged.
- The contract is independently testable without SQLite, CLI, or MCP.

### Negative and risks

- Phase 0 users or integrations must supply a repository ID until `init`
  exists.
- Separate clones do not share identity unless configured intentionally.
- Porcelain-v2 parsing and a second receipt capture add implementation and
  indexing cost.
- Conservative status scope can invalidate Rust indexing after unrelated
  worktree changes.
- Sparse and recursive-submodule worktrees remain unavailable initially.
- The repository-ID text tag becomes a compatibility boundary.

## Validation

- Golden vectors and all-component invalidation tests for both digest
  encodings.
- Canonical repository-ID text round trips and hostile input tests.
- SHA-1, SHA-256, symbolic, detached, unborn, shallow, conflict, untracked,
  non-UTF-8, case-collision, linked-worktree, sparse, and submodule fixtures.
- Differential status-category tests against sanitized Git.
- Concurrent checkout, index update, and atomic file-replacement tests between
  receipt captures.
- Bounds, cancellation, deadline, command failure, and queue saturation tests.
- Clean-versus-incremental equivalence for one exact source state.
- Default debug/error output contains no identity bytes, repository paths,
  remotes, raw status, or subprocess stderr.
- Real read-only probes on the pinned corpus and neighboring repositories.

## Follow-up

- The canonical repository-ID text boundary is implemented in
  `repowitness-application`.
- Bounded sanitized Git/worktree receipt capture and stability fencing are
  implemented in `repowitness-local`.
- The local index request/report facade and behavior tests compose source-state
  capture with the shared publication use case.
- Explicit `--repository-id` and `--database` CLI arguments are implemented,
  and `index` atomically activates production supported-language generations.
- Add the equivalent validated MCP request/response boundary before enabling
  MCP indexing.
- Specify `init`, shared repository-ID storage, and conflict behavior in a
  later focused configuration/memory decision.
- Revisit sparse and recursive-submodule support only after accepting an
  explicit capture and coverage contract.

## Supersession

None.
