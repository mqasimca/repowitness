# ADR-0032: Admit connected workspaces through an explicit manifest

- Status: Proposed
- Date: 2026-07-29
- Owners: Project maintainers
- Scope: Connected-workspace input, hostile manifest parsing, CLI composition,
  configuration authority, and privacy

## Context

[ADR-0026](0026-connected-workspace-source-slots-and-views.md) defines the
persisted source-slot and immutable-view model.
[ADR-0031](0031-source-slot-selectors-and-package-scopes.md) defines the
validated selector, caller-provided worktree, and package-scope inputs. Neither
decision defines how a local caller supplies a complete multi-source request.

Inferring neighboring repositories from the filesystem would broaden
authorization, make behavior depend on host layout, and risk persisting or
logging private paths. Encoding an entire source tuple in repeated command-line
arguments would preserve operating-system path bytes, but would be difficult
to review, quote, reproduce, and validate atomically. Reusing the layered
`repowitness.toml` policy document would mix two trust concerns: monotonic
runtime policy and explicit authorization to read particular worktrees.

## Decision

### Separate, explicitly selected document

Add a strict version-1 connected-workspace manifest. The CLI accepts it only
through:

```text
repowitness workspace index --manifest <path> --database <path>
```

There is no ambient discovery, parent-directory search, default filename, or
environment-variable fallback for the manifest. `--manifest` and `--database`
are each accepted exactly once and support the existing `--` positional
separator rules where applicable. Help and invalid arguments perform no file,
Git, or database I/O.

The CLI reads at most 1 MiB from an explicitly named regular file using a
non-blocking, no-follow open where the platform supports it, then validates
the opened-file metadata. Symbolic links, directories, devices, sockets,
pipes, replacement during admission, invalid UTF-8, duplicate keys, unknown
fields, and unsupported versions fail closed with path-free errors.

### Manifest schema

The TOML document contains:

- `schema_version = 1`;
- one canonical `cwi1:h:` connected-workspace ID; and
- one to 256 `[[source]]` tables.

Each source table contains:

- one canonical `ssi1:h:` source-slot ID;
- one canonical logical repository identity;
- one non-empty UTF-8 worktree-root string of at most 4,096 bytes;
- exactly one structured selector:
  `worktree-head`, `exact-revision`, or `full-ref`; and
- exactly one structured package scope: `whole-repository` or one to 64
  canonical `rwp1:h:` repository-path values.

The exact version-1 TOML spelling is:

- top-level `schema_version` and `connected_workspace_id`;
- repeated `[[source]]` tables containing `source_slot_id`,
  `repository_identity`, and `worktree_root`;
- `[source.selector]` with `kind`; `exact-revision` and `full-ref` require
  exactly one `value`, while `worktree-head` forbids it; and
- `[source.scope]` with `kind`; `explicit-roots` requires `roots`, while
  `whole-repository` forbids it.

The canonical repository-path text boundary preserves non-UTF-8 package-root
bytes. Version 1 worktree-root text is UTF-8 because TOML cannot portably
represent arbitrary host-path bytes. The existing single-repository CLI
remains available for an authorized non-UTF-8 worktree root. A future manifest
version may add a platform-tagged host-path encoding only after a portable
authorization contract exists.

Relative worktree roots resolve against the admitted manifest's containing
directory. Absolute roots remain explicit caller authority. Normalized,
canonical, or resolved host roots are never persisted, returned, or included
in default diagnostics. Manifest order is not semantic: source slots are
validated as unique and canonicalized by exact source-slot bytes before the
coordinator runs. Multiple slots may intentionally name the same logical
repository or worktree.

### Configuration and identity

The ordinary version-1 configuration layers remain separate inputs. One
resolved configuration is supplied to every source slot in manifest version
1; repository or workspace files are not implicitly discovered from each
listed worktree. Explicit higher-trust CLI or user layers may select the MCP
profile, while workspace and repository policy cannot grant authority under
[ADR-0025](0025-versioned-local-configuration-and-policy.md).

Each slot's package-scope digest is domain-separated into its effective
semantics identity. Therefore equal source bytes under different scopes cannot
reuse or publish a mismatched snapshot. Selector text and host roots do not
become durable identity; concrete source-state receipts, package-scope
identity, opaque workspace and slot IDs, and immutable generations do.

### Publication and output

The command invokes one bounded connected-workspace coordinator. It succeeds
only after every source slot has an exact completion receipt and one immutable
workspace view is atomically active. Failure, cancellation, a stale selector,
or a stale epoch keeps the previous active view readable.

Default output contains only the manifest schema, opaque version tags,
configuration and view digests, aggregate source/generation counts, explicit
coverage and omission counts, and outcome. It never includes manifest
contents, host roots, raw selectors, source text, memory text, credentials, or
environment values.

## Alternatives considered

### Discover repositories adjacent to the current directory

This is convenient but silently widens filesystem authority, depends on host
layout, and creates a privacy hazard. Explicit source roots are required.

### Put sources in the layered policy file

Policy files merge by provenance and monotonic restriction. Source membership
is an atomic authorization set with immutable identity. Combining them would
make merge semantics ambiguous and could let a less-trusted layer add a root.

### Use only repeated CLI source tuples

This can preserve arbitrary host-path bytes, but long tuples are difficult to
review and reproduce and are easy to associate incorrectly. The existing
single-source command covers the non-UTF-8-root exception.

### Persist the manifest

Persisting it would retain host paths and moving selector text. Only validated
opaque identities and concrete receipts cross the storage boundary.

## Consequences

### Positive

- Multi-source authorization is explicit, reviewable, bounded, and
  reproducible.
- Host layout and neighboring repositories cannot silently enter a workspace.
- Package roots preserve repository-path bytes through a canonical text
  boundary.
- Policy authority remains separate from source admission.
- Storage and normal diagnostics remain path- and selector-free.

### Negative and risks

- Version 1 cannot describe a non-UTF-8 worktree root.
- Callers must manage opaque workspace and slot IDs.
- One shared resolved configuration is less flexible than per-slot policy.
- A manifest is another hostile file boundary that needs cross-platform
  no-follow and replacement tests.

## Validation

- Golden one-source, two-repository, and same-repository/two-slot manifests.
- Zero, 256, and 257 sources; duplicate and reordered source slots; repeated
  logical repositories; and maximum-length scalar boundaries.
- Unknown and duplicate keys, type confusion, deep nesting, malformed TOML,
  invalid UTF-8, oversize input, and one-byte-over-limit input.
- Canonical workspace, slot, repository, and repository-path text vectors,
  including non-UTF-8 package roots and invalid lowercase or odd-length text.
- Relative and absolute roots, option-shaped names, empty paths, missing
  paths, path replacement, case distinctions, and platform separators.
- Symbolic-link, hard-link alias, FIFO, device, directory, and concurrent
  replacement admission tests where supported.
- Selector and package-scope error propagation without content disclosure.
- Cancellation and deadline at every source boundary, stale epoch, moving
  ref, database alias, process termination, restart, and prior-view recovery.
- Equivalent source order produces the same request and view identity.
- Persistence, output, error, debug, and privacy scans contain no manifest
  path, host root, raw selector, source text, or credential canary.

## Follow-up

- Add an explicit ID-generation command backed by operating-system secure
  randomness before recommending the manifest to new users.
- Add per-slot configuration only through a new manifest version with
  monotonic authority tests.
- Revisit a portable host-path encoding only after Windows and Unix
  authorization semantics can be represented without lossy conversion.

## Supersession

None. This supplies the local input boundary required by ADR-0026 and ADR-0031
without changing their identity or publication contracts.
