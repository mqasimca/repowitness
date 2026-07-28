# Phase 0 repository and source-state identity

- Status: Implemented and promoted
- Research date: 2026-07-25
- Last updated: 2026-07-26
- Reviewed baseline: Git 2.55.0 documentation
- Scope: repository identity, Git revision receipts, worktree receipts, and
  concurrent-mutation detection

## Conclusion

Git does not expose a portable, clone-stable repository UUID. RepoWitness
should therefore require an explicit opaque repository identity rather than
derive one from a host path, remote URL, branch, root commit, or current
commit.

For Phase 0, source-state identity should combine:

1. the explicit repository identity;
2. a canonical Git receipt containing object format, concrete `HEAD` state,
   and shallow-history state;
3. a canonical worktree receipt derived from validated, sorted porcelain-v2
   status records plus the exact Rust source-manifest digest; and
4. the existing configuration, producer, schema, and canonicalization
   identities.

Capture the Git and worktree receipts before discovery and again after final
source revalidation. Any difference fails the attempt as concurrently changed;
it never publishes a mixed generation.

[ADR-0013](../adr/0013-phase0-repository-and-source-state-identity.md)
accepts this contract as the Phase 0 implementation boundary.

## Primary findings

### Git metadata is not repository identity

The current experimental
[`git repo info`](https://git-scm.com/docs/git-repo.html) command reports
layout, shallow state, object format, and reference format. It is explicitly
documented as experimental and provides no repository UUID. Stable
[`git rev-parse`](https://git-scm.com/docs/git-rev-parse.html) queries likewise
report repository layout and object format rather than a portable project
identity.

The absence of a repository-ID key is an inference from the documented
metadata surfaces, not a claim that Git forbids another tool from maintaining
its own ID.

None of the common substitutes is a sound default:

- an absolute or canonical path is host-local, moves, leaks personal
  locations, and aliases linked worktrees incorrectly;
- a remote URL can be absent, mutable, credential-bearing, or different
  between equivalent clones;
- a root-commit set is incomplete in shallow history and intentionally shared
  by forks;
- the current commit changes during ordinary work and identifies a revision,
  not the repository; and
- a branch name is a moving selector, not durable identity.

RepoWitness should treat repository identity as caller/configuration input and
validate it before any domain or persistence construction.

### Git object and HEAD state are typed inputs

[`git rev-parse --show-object-format`](https://git-scm.com/docs/git-rev-parse.html)
reports the repository storage hash algorithm. RepoWitness must retain that
algorithm with the raw object ID rather than assume SHA-1.

`git rev-parse --verify --quiet --end-of-options HEAD^{commit}` distinguishes
an existing commit from an unborn `HEAD`. A detached and symbolic `HEAD` at the
same commit have the same concrete revision identity; branch names remain
selectors under [ADR-0005](../adr/0005-git-dag-temporal-memory.md).

Shallow repositories require an explicit receipt. Git documents that the
`shallow` file makes listed commits appear as traversal roots even though their
parents exist elsewhere. A shallow clone therefore cannot claim complete
ancestry coverage. See the
[`shallow` documentation](https://git-scm.com/docs/shallow).

The experimental `git repo info` command is not required. The stable
`rev-parse` equivalents remain the compatibility baseline.

### Porcelain v2 is the worktree-status boundary

[`git status --porcelain=v2 -z`](https://git-scm.com/docs/git-status.html)
provides a stable script format, repository-root-relative paths, unquoted path
bytes under `-z`, index and worktree modes and object IDs, conflict stages, and
submodule state. Unknown optional headers are not needed because the Phase 0
profile does not request branch or stash headers.

RepoWitness should still parse the output rather than hash raw subprocess
bytes:

- validate every path as `RepositoryPath`;
- reject unknown mandatory record tags and malformed field counts;
- disable rename detection for this receipt;
- retain ordinary, unmerged, and untracked record categories explicitly;
- sort records by exact repository-path bytes and then stable record tag; and
- reject duplicate identities before canonical hashing.

The status receipt is not a digest of every byte in every non-indexed file.
Exact indexed Rust content comes from the canonical source manifest.
Semantics-affecting configuration files belong in the configuration digest,
and skipped/non-indexed scope remains explicit coverage. Phase 0 should reject
sparse worktrees and recursive submodule scope until their policies are
implemented rather than claim complete support.

### Linked worktrees share repository state but not private state

Git documents that linked worktrees use a shared `$GIT_COMMON_DIR` for most
repository data and a private `$GIT_DIR` for worktree-specific `HEAD`, index,
and related state. It recommends using Git commands rather than assuming file
locations. See the
[`git worktree` documentation](https://git-scm.com/docs/git-worktree.html).

RepoWitness should never hash either host path into source identity. Linked
worktrees configured with the same explicit repository ID share repository
identity, while their concrete `HEAD`, status receipt, and source manifest
determine whether their source snapshots are equal.

### Snapshot capture needs a stability fence

A single Git query before file discovery does not prevent concurrent checkout,
index mutation, or worktree edits from mixing states. The local adapter should:

1. capture bounded Git and worktree receipts;
2. discover, open, hash, and analyze the selected source files;
3. perform the existing final path/content revalidation;
4. capture the receipts again; and
5. publish only if both captures are exactly equal and the source epoch is
   still current.

Every subprocess uses the existing sanitized, non-interactive, no-shell
profile with explicit output, deadline, and cancellation bounds.

## Recommended canonical inputs

### Repository identity text

Use `rwi1:h:` followed by exactly 64 uppercase Base16 characters encoding the
caller-assigned 32-byte identity. Decoding rejects lowercase, whitespace,
alternate prefixes, and all non-canonical forms.

The ID may be generated by a future `init` flow and stored in user-state
configuration or intentionally shared configuration. Phase 0 indexing should
require it explicitly until that flow exists. It must not silently write an ID
into the target repository.

### Git-state digest version 1

```text
SHA-256(
  "RepoWitness\0git-state\0" ||
  version:u32be ||
  object_format:u8 ||
  head_state:u8 ||
  head_oid_length:u8 ||
  head_oid_bytes ||
  shallow:u8
)
```

`object_format` admits only the explicitly supported Git algorithms.
`head_state` is `unborn` or `commit`; unborn state has a zero-length OID.
`shallow` is a canonical Boolean byte.

### Rust worktree-state digest version 1

```text
SHA-256(
  "RepoWitness\0rust-worktree-state\0" ||
  version:u32be ||
  status_profile_version:u32be ||
  status_record_count:u64be ||
  for each canonical path-ordered status record:
    record_tag:u8 ||
    record_field_encoding ||
    path_length:u64be ||
    path_bytes ||
  source_manifest_digest[32]
)
```

Every variable-width field is length-prefixed. Object IDs retain their object
format and exact decoded bytes. Record tags and mode/status encodings are
versioned rather than persisted as process enum discriminants.

## Implementation follow-up

Accepted [ADR-0013](../adr/0013-phase0-repository-and-source-state-identity.md)
promotes these identities into the production preparation and indexing
composition. The CLI requires the canonical explicit repository ID, captures
sanitized Git and worktree receipts before and after contained source reads,
and rejects a changed index, status, or `HEAD` instead of publishing a mixed
snapshot. SHA-1, SHA-256, unborn/detached `HEAD`, linked worktrees,
non-UTF-8/case-colliding paths, hostile configuration, and concurrent-mutation
fixtures pass. An actual nested-submodule fixture and concurrent sparse/gitlink
mode-transition fixtures also pass. Sparse worktrees and gitlinks remain
explicit fail-closed Phase 0 scope.

## Validation matrix

- explicit repository identity: canonical, lowercase, wrong-width, wrong-tag,
  empty, and overlong inputs;
- SHA-1 and SHA-256 repositories;
- symbolic, detached, and unborn `HEAD`;
- clean, staged, unstaged, untracked, conflicted, and renamed paths with rename
  detection disabled;
- non-UTF-8 Unix paths and case-colliding paths;
- linked worktrees at equal and different commits;
- shallow repository with missing ancestry;
- sparse worktree and gitlink/submodule fail-closed behavior;
- hostile repository configuration and environment;
- output/count/deadline/cancellation limits;
- checkout, index mutation, and atomic file replacement between the two
  receipt captures;
- clean and incremental construction of the same accepted snapshot produces
  identical digests; and
- default diagnostics and debug output expose no repository ID bytes, paths,
  remote URLs, status content, or subprocess stderr.

Run the real-repository probe read-only against ordinary and linked worktrees.
Differentially compare parsed status categories with the Git CLI oracle, while
keeping raw status bytes out of persisted identity.

## Revisit conditions

- Git standardizes a stable non-experimental repository identifier with
  documented clone/fork semantics.
- A team-memory schema selects a repository-tracked shared ID and conflict
  behavior.
- Phase 0 expands beyond one repository/worktree or admits sparse and recursive
  submodule indexing.
- Measurements show full porcelain-v2 status is the dominant indexing cost.
