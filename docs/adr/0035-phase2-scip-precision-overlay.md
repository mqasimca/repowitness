# ADR-0035: Import SCIP as a bounded precision overlay

- Status: Accepted
- Date: 2026-07-31
- Owners: Project maintainers
- Scope: Phase 2 SCIP import, package-aware resolution, source/graph evidence,
  SQLite persistence, CLI, local stdio MCP, and context-provider inputs

## Context

Phase 1 publishes a complete, immutable, generation-pinned Rust syntax graph.
It deliberately abstains from package-aware resolution, compiler evidence,
macro expansion, dynamic dispatch, and cross-language edges. Syntax facts
remain valuable for every indexed source file, including malformed or
incompletely indexed worktrees, but they cannot establish compiler-grade
cross-file navigation.

The [SCIP Code Intelligence Protocol](https://github.com/scip-code/scip)
provides a language-neutral Protobuf interchange for definitions, references,
implementations, relationships, documents, and producer metadata. Its official
Rust bindings and schema support consuming producer-generated indexes; they do
not make an imported symbol a RepoWitness logical-symbol identity or guarantee
that the index covers every source file. An imported binary file, its paths,
positions, symbols, documentation, and relationships are hostile input.

Phase 2 needs a precision path without replacing syntax coverage, weakening the
immutable-generation contract, or silently treating an incomplete package index
as a complete repository truth. The corresponding package-aware result must be
explicitly attributed to its SCIP producer and exact source view.

## Decision

### SCIP is an optional immutable overlay

One successful import creates a new immutable SCIP overlay pinned to exactly
one completed connected-workspace view, source snapshot, active generation,
configuration digest, and source-slot set. It never mutates syntax artifacts,
syntax graph rows, an active view, or a previously published overlay. A failed,
cancelled, timed-out, superseded-source, or invalid import leaves the preceding
active syntax and SCIP overlays readable.

The imported overlay identity is a versioned domain-separated digest of:

```text
overlay format version
+ exact accepted SCIP protocol/schema identity
+ importer implementation identity
+ source view, snapshot, and generation identities
+ canonical producer metadata
+ canonical import policy and limits
+ exact SCIP input content digest
```

The source view and source snapshot must still be current at the final import
fence. A SCIP input is never reused when any field above differs. Equivalent
replays return the prior immutable overlay receipt instead of duplicating facts.

### Admission is explicit, contained, and bounded

The CLI accepts one explicit local SCIP file through the same capability-based,
no-follow, regular-file boundary used for other hostile local inputs. Local
stdio MCP remains read-only: it may expose results from a previously imported
overlay but cannot receive host paths or write one. Import never starts an
indexer, invokes a package manager, downloads dependencies, follows producer
paths, reads arbitrary referenced files, or executes embedded commands.

The importer has independent configured ceilings for input bytes, decoded
metadata bytes, document count, occurrence count, relationship count, symbol
bytes, documentation bytes, per-document output, database rows, CPU work,
wall-clock deadline, and captured diagnostics. It checks cancellation and the
monotonic deadline before every bounded decode/validation/write batch. A limit
or decode failure returns a categorical diagnostic and publishes no partial
overlay.

The production decoder must process documents in bounded batches. It must not
retain an unbounded decoded index, raw source text, raw symbols, or a full
untrusted diagnostic in memory or logs. Selecting a new Protobuf/SCIP
dependency is separately gated by the Phase 2 dependency review: license,
source, advisory, features, build scripts/proc macros, native code, binary
size, maintenance, and MSRV compatibility must be recorded before it enters
the workspace.

### Validate every claim against the pinned source view

Each imported document path is parsed as a repository-relative byte path and
must resolve beneath the admitted source root without symlink or reparse-point
escape. It must correspond to a member of the pinned generation with the exact
same content digest when the producer provides a document digest; a missing or
mismatched document is excluded with categorical coverage rather than
reinterpreted against current bytes.

Occurrence ranges are validated against the exact source bytes and the SCIP
position encoding declared by the accepted schema/profile. Invalid, reversed,
out-of-bounds, non-boundary, or unsupported-encoding ranges are rejected per
document. A valid SCIP symbol is an opaque, bounded producer-local identifier;
it is never rendered or used as a filesystem path, SQL fragment, command, or
automatic cross-history correspondence proof.

Package identity, project root, tool arguments, external symbols, hover text,
and relationship labels are all untrusted metadata. The importer stores only
the validated, bounded information required for evidence and preserves
path-free/redacted diagnostics by default.

### Evidence precedence is query-specific and non-destructive

Syntax and SCIP facts coexist. Every graph or context result exposes its
concrete generation, overlay identity when used, producer identity, evidence
class, categorical resolution, coverage, and unresolved/truncated work.

For an exact definition/reference/implementation query whose validated SCIP
overlay contains an applicable unambiguous claim, the result may prefer that
claim over a conflicting syntax-only resolution. It must still report syntax
coverage and must not erase syntax candidates, parser diagnostics, or an
incomplete SCIP-document state. When SCIP is absent, stale, incomplete,
ambiguous, invalid for the selected source view, or outside the requested
package scope, the result falls back to the existing syntax contract and says
why precise evidence was unavailable.

SCIP may establish package-aware cross-file edges only from validated imported
occurrences and relationships. It does not infer package-manager topology,
run builds, expand macros, claim dynamic dispatch completeness, resolve
cross-language edges, or promote symbols to durable logical identities. The
existing precision-first correspondence rules continue to require explicit
cross-revision evidence and abstain on ambiguity.

The initial package-aware read contract selects an overlay only by an exact
pinned view and source slot, then filters its document paths through the
caller-provided versioned `PackageScope` using repository-byte component
boundaries. It returns separately bounded occurrences and incoming/outgoing
relationships, with independent truncation and an output-byte ceiling. An
absent overlay, a scope-local no-match, and a truncated result remain distinct
outcomes; a scope filter never reads a package manager, producer project root,
or another source slot.

### Interface and persistence boundaries

The domain exposes validated SCIP evidence, producer, coverage, bounded range,
and categorical resolution values only; it has no Protobuf, filesystem,
SQLite, or wire-DTO dependency. The analysis crate translates immutable
validated document batches into provider-neutral overlay facts. The application
owns import/use-case coordination and precedence policy through narrow ports.
The local crate owns contained input, decoding, SQLite migration, and owned
writer batching. CLI and MCP remain thin adapters over the same application
reads.

Persisted SCIP overlays, documents, occurrences, relationships, coverage, and
receipts use a new versioned SQLite migration. Readers pin one generation plus
one selected immutable overlay. Activation/recovery, retention, backup, and
garbage collection treat each overlay as a generation root; no partially
written overlay is reader-visible.

## Alternatives considered

### Extend the Rust syntax resolver with package inference

This would keep one implementation path but would require reimplementing
compiler/package semantics while still failing to provide compiler evidence for
other supported languages. SCIP delegates that work to versioned producers and
keeps the compiler-derived evidence explicitly attributed.

### Replace syntax facts with SCIP

SCIP indexes can be absent, partial, stale, or invalid for a local worktree.
Replacing syntax would hide useful coverage and make an unavailable precision
provider look like no source knowledge exists.

### Accept an unpinned global SCIP cache

An index produced for another checkout or configuration can cite the wrong
bytes. Pinning it to a complete immutable source view is required for
evidence-backed answers.

### Run indexers from RepoWitness

Executing package-manager or producer commands expands the trust, network,
process, and reproducibility boundary. Phase 2 consumes explicitly supplied
local artifacts only; an opt-in supervised producer protocol needs a separate
decision and measurement.

## Consequences

### Positive

- Package-aware cross-file results can carry compiler/producer evidence.
- Syntax coverage remains available for files and claims SCIP does not cover.
- Precise results have an exact, reproducible source/producer/overlay identity.
- The importer adds no background command execution or remote dependency.

### Negative and risks

- A new binary format, migration, dependency review, and resource profile add
  meaningful implementation and maintenance cost.
- Producers have language- and version-specific coverage limits.
- Strict source/range validation can reject otherwise useful stale indexes.
- Evidence precedence and multi-provider context selection need dedicated
  evaluation to avoid reducing useful context or masking abstention.

## Validation

- Golden fixtures for accepted schema/protocol metadata, package identities,
  definitions, references, implementations, relationships, and complementary
  syntax evidence.
- Hostile fixtures for truncated/oversized payloads, malformed Protobuf,
  duplicate or excessive fields, invalid paths/ranges/encodings, stale source
  digests, ambiguous symbols, and unsafe metadata.
- Unit, property, and fuzz tests for streaming/bounded decode and validated
  range/path conversion.
- Real SQLite migration, crash/recovery, cancellation, active-overlay pinning,
  backup/restore, retention, and clean-versus-incremental tests.
- Cross-platform and public pinned-corpus evaluation proving improved precise
  navigation without hiding syntax coverage or increasing stale answers.

## Follow-up

- Complete the SCIP dependency and implementation research before selecting a
  decoder or adding a production dependency.
- Define the versioned package-aware graph/read contract and its MCP/CLI
  response schemas.
- Define a separate versioned Phase 2 context ranking/profile decision and
  evaluate it against lexical, graph-only, and supported incumbent baselines.

## Supersession

None. This adds an optional precision overlay while preserving the accepted
syntax and identity contracts in ADR-0004, ADR-0006, ADR-0019, ADR-0026, and
ADR-0027.
