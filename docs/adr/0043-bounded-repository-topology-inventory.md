# ADR-0043: Bounded repository topology inventory

- Status: Proposed
- Date: 2026-08-01
- Owners: Project maintainers
- Scope: sanitized Git discovery, local topology capture, immutable generation
  publication, SQLite reads, CLI, and local stdio MCP

## Context

Coding agents need to orient themselves using more than supported source files:
documentation, agent instructions, build/package descriptors, configuration,
and CI workflow paths often identify the relevant code boundary. RepoWitness
currently indexes only Rust, Go, TypeScript, TSX, and Python source. Treating
Markdown, TOML, YAML, or arbitrary files as another source language would
weaken source artifact, parser, reuse, secret-handling, and coverage contracts.

The cached tracked Git index and final source-state fence can establish an exact
bounded set of repository-relative paths without returning host paths or
reading file contents. Untracked and deleted paths must not enter the receipt.
That is sufficient for an initial, privacy-preserving topology inventory.

## Decision

Add `repository_topology`, a separate version-1 path-only inventory pinned to
one active generation and a matching topology receipt.

- It classifies only an allow-listed path into `documentation`,
  `agent_instruction`, `workflow_descriptor`, `build_descriptor`,
  `package_descriptor`, `configuration_descriptor`, or `other_tracked_file`.
- It returns canonical repository-relative paths, deterministic aggregate
  category totals, the active source snapshot/generation, a separate topology
  digest, coverage, omissions, and explicit truncation. It never returns file
  contents, labels, configuration values, raw URLs, host paths, or content
  hashes for non-source assets.
- Topology capture uses only bounded cached tracked Git-index paths, excludes
  untracked and deleted paths, performs no non-source content read, and has
  cancellation/deadline checks plus the authoritative final source-state fence.
  It publishes atomically beside the matching source generation and survives
  recovery, retention, and backup.
- Readers reject a mismatched topology profile or a recomputed full-inventory
  digest mismatch before applying the response bound.
- A topology-only change creates a new topology receipt but does not invalidate
  reusable supported-language analysis artifacts.
- The response limitation is fixed to
  `inventory_only_no_semantic_relationship_inference`. A file category is not
  a package boundary, ownership, dependency, build, deployment, or runtime
  claim.

Markdown local-link extraction and descriptor-specific facts are not included
in version 1. They each require an independently versioned parser/reader,
content-read policy, evidence tier, secret review, and relationship meaning.

## Alternatives considered

### Add documentation/configuration as source languages

Rejected. These files have different content sensitivity and do not produce the
same semantic source facts as supported programming languages.

### Parse workflows and descriptors lexically

Rejected. A `uses`, `run`, dependency, or link-looking string does not prove
build, deployment, package, ownership, or code relationships.

### Expose all discovered files through `architecture_map`

Rejected. It would make a source-artifact map contain non-source receipts with
incompatible evidence and reuse semantics.

## Consequences

### Positive

- Agents can find relevant repository paths without source expansion or
  arbitrary content ingestion.
- The inventory is exact, bounded, snapshot-pinned, and privacy-aware.
- Source analysis remains limited to the accepted five-language slice.

### Negative and risks

- Path presence does not explain semantic relationships.
- Every allow-list expansion needs a review of its privacy and classification
  behavior.
- A separate receipt, tables, publication, recovery, and retention paths add
  implementation complexity.

## Validation

- Synthetic fixtures for every asset category, unsupported paths, canonical
  ordering, limits, truncation, cancellation, deadline, non-UTF-8 paths,
  symlink/special-file rejection, final-fence races, and topology-only changes.
- Migration, staging, activation, recovery, retention, backup, corruption, and
  stale-generation isolation tests.
- CLI/MCP schema, tool-list, output-bound, stdout-purity, privacy-canary, and
  installed-binary contracts.
- Sibling smoke coverage reports only aggregate inventory-schema outcomes and
  never emits external repository paths or individual measurements.

## Follow-up

Evaluate versioned Markdown local-link topology first. Evaluate descriptor
parsers one format at a time; a TOML claim must be limited to fully understood
syntax, and workflow semantics require a separately reviewed parser.

## Supersession

None.
