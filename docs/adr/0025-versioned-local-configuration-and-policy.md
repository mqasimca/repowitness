# ADR-0025: Resolve versioned local configuration with monotonic policy

- Status: Accepted
- Date: 2026-07-29
- Owners: Project maintainers
- Scope: Local configuration, policy resolution, diagnostics, CLI, and MCP startup

## Context

Phase 0 uses explicit CLI arguments and compiled resource limits. That keeps
the initial trust boundary small, but it cannot support repeatable workspace
configuration, named operating profiles, or an explanation of why an
effective value was selected.

Ordinary preferences and security policy have different precedence rules.
Last-write-wins is appropriate for presentation or local tuning. It is unsafe
for an untrusted repository to re-enable a denied capability, broaden an
allowed root or language set, or raise an administrator resource ceiling.

Configuration is hostile input. Misspelled keys, duplicate declarations,
oversized files, path aliases, secret values, and ambiguous layer provenance
must fail closed or remain explicit. Configuration that changes analysis or
retrieval semantics must also participate in the relevant identity digest.

## Decision

### Versioned strict schema

Use `repowitness.toml` schema version 1 for local configuration. Decode into a
wire DTO, reject unknown fields and duplicate TOML keys, validate every value,
and only then construct application configuration values.

Each input file is bounded before parsing. Text fields, collections, numeric
ranges, and the number of layers are bounded independently. Version 1 accepts
no inline credentials, tokens, commands, plugin paths, remote endpoints, or
executable hooks.

The initial schema contains only settings that the implementation can enforce:

- a named built-in profile;
- default query, context, graph, and watcher-polling limits;
- MCP tool profile selection;
- allowed source languages;
- maximum source-file bytes and source-file count;
- maximum query, context, graph-depth, and graph-result ceilings;
- an irreversible memory-write denial; and
- the no-follow symlink policy.

Unknown settings are errors. Future settings require a schema revision or a
documented optional-field rule; they are not silently ignored.

### Layer order and provenance

Ordinary preferences resolve in this order, with the last specified value
winning:

```text
built-in safe defaults
    -> selected named profile defaults
    -> user configuration
    -> workspace configuration
    -> repository configuration
    -> explicitly admitted environment references
    -> CLI flags
```

Every effective preference records the layer kind that supplied it. Host file
paths are not part of the public explanation and are not logged.

The profile is selected before profile defaults are applied. Version 1 permits
selection from the explicit CLI input or user layer only. Workspace and
repository files may tune supported preferences but cannot replace the
profile beneath higher-trust configuration.

### Monotonic policy merge

Policy does not use ordinary precedence:

- allowed language or capability sets are intersected;
- deny sets are unioned;
- numeric ceilings take the minimum;
- `deny_memory_writes = true` cannot be reversed by a later layer;
- `follow_symlinks` remains false in version 1; and
- compiled hard ceilings remain an upper safety bound even when every file
  requests a larger value.

Each effective policy value records every constraining layer, not only the
last layer that mentioned it. A layer that requests a broader value may be
accepted as a request, but the explanation shows the higher-trust constraint
that prevented it from becoming effective. Values outside the absolute schema
range are rejected rather than clamped.

Repository configuration can preserve or tighten policy. It cannot grant a
capability. Enabling MCP memory mutation remains a separate local startup
capability with a fixed validated actor and is still subject to any effective
deny.

### Canonical identity

Hash the complete effective semantic configuration using a domain-separated,
versioned, canonical encoding with fixed field order and explicit units. The
digest excludes provenance paths and presentation-only explanation text.

Indexing uses this digest as its `ConfigurationDigest`. Query and server
diagnostics return the digest and schema/profile versions. A change that
affects parsing, resolution, evidence, or retrieval invalidates only the
artifacts or projections whose versioned identities include that setting.

### CLI behavior

`repowitness config explain` is read-only. It returns:

- schema and resolver versions;
- selected profile and its provenance;
- every effective preference and supplying layer;
- every effective policy value and constraining layers;
- the canonical semantic digest; and
- warnings or unsupported settings without revealing host paths or secrets.

`repowitness doctor` is also read-only. Before indexing or serving, it checks
the resolved configuration, repository containment, database placement and
capabilities, compiled language adapters, requested tool profile, and
incompatible settings. It distinguishes errors from warnings and returns a
nonzero status when a required invariant fails.

Neither command creates a database, writes repository configuration, mutates
Git, or probes a secret value. A future `init` command requires a separate
decision.

## Alternatives considered

### Last-write-wins for every setting

This is familiar and simple, but lets a lower-trust repository broaden policy
or undo a denial.

### One configuration file

One file avoids merge semantics, but cannot express user defaults, workspace
coordination, repository tightening, or local operator capabilities without
copying sensitive host details into repositories.

### Silently ignore unknown keys

This improves forward compatibility but turns misspellings into unsafe or
surprising defaults. Explicit schema evolution gives a reviewable boundary.

### Put parsing and policy directly in the CLI

That would make MCP and future adapters resolve different effective policy.
The parser belongs to the local adapter and validated resolution belongs to
the application boundary.

### Persist source configuration text

It would retain comments, paths, and potentially sensitive values without
being needed for semantic identity. Persist only the validated canonical
digest and bounded non-sensitive diagnostic metadata.

## Consequences

### Positive

- Effective behavior is reproducible and explainable.
- Repository-owned input cannot weaken higher-level safety policy.
- Misspellings and unsupported capabilities fail visibly.
- CLI, MCP, indexing, and diagnostics share one resolved configuration.
- Semantic configuration changes participate in artifact and snapshot reuse.

### Negative and risks

- Two merge models are more complex than one precedence list.
- Profile selection must be resolved before ordinary layer application.
- Adding a setting requires deciding whether it is a preference, policy, or
  immutable safety invariant.
- Platform configuration locations and path aliases need cross-platform
  fixtures.
- Explanation output can itself leak host details unless its schema remains
  deliberately path-free.

## Validation

- Golden tests for every schema field, named profile, canonical encoding, and
  digest.
- Reject unknown keys, duplicates, unsupported versions, invalid UTF-8,
  oversized files, excessive arrays, invalid ranges, and inline secret-like
  fields.
- Property tests that policy ceilings never increase, allow sets never grow,
  deny sets never shrink, and layer ordering is deterministic.
- Pairwise and full-stack layer fixtures proving preference provenance and
  policy constraint provenance.
- Linux, macOS, and Windows fixtures for default locations, path aliases,
  missing files, and repository-local configuration.
- CLI golden tests for redacted `config explain` and `doctor` output.
- Tests proving both commands are read-only and leave absent databases and
  repositories byte-for-byte unchanged.
- Index/reopen tests proving semantic configuration changes invalidate reuse
  while presentation-only provenance does not.
- MCP startup tests proving a repository layer cannot enable memory writes or
  an unavailable compatibility profile.

## Follow-up

- This accepted contract is the stable Phase 1 local-configuration boundary.
- Keep the implemented strict parser, canonical resolver, path-free
  explanation, and read-only diagnostics aligned as the schema evolves.
- Preserve explicit resolved-configuration wiring through indexing, queries,
  diagnostics, and MCP startup as those surfaces evolve.
- Add automatic platform configuration discovery only through a separately
  reviewed scope decision; version 1 runtime inputs remain explicit.
- Maintain the focused dependency review for the selected TOML parser.

## Supersession

None.
