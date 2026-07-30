# ADR-0027: Build a generation-resolved Rust syntax graph

- Status: Proposed
- Date: 2026-07-28
- Owners: Project maintainers
- Scope: Phase 1 Rust analysis, graph resolution, bounded trace and impact,
  context compilation, and SQLite persistence

## Context

Phase 0 persists immutable per-file declaration facts and publishes them through
atomic index generations. It does not extract imports, references, calls, macro
calls, or test markers, and it cannot answer graph trace or impact questions.
Phase 1 needs those relationships without weakening the existing artifact,
generation, evidence, ambiguity, and memory-revalidation contracts.

Rust syntax alone cannot prove every semantic relationship. Conditional
compilation changes which source exists, macros can generate or reinterpret
syntax, trait dispatch can have multiple implementations, and function values
or trait objects make calls dynamic. A name match is evidence, not proof.
Choosing one target when zero or many are supported would create false
precision.

Raw syntax also has a different lifetime from cross-file resolution. A site's
spelling and byte span depend only on one exact source artifact and the analyzer
profile. Its possible targets depend on the complete repository generation,
workspace source slots, configuration, and resolver profile. Mixing those
lifetimes would either duplicate immutable parsing work or reuse stale edges.

The proposed connected-workspace/source-slot decision in
[ADR-0026](0026-connected-workspace-source-slots-and-views.md) defines the
identity boundary on which generation resolution depends. Versioned local
configuration and monotonic policy are governed by
[ADR-0025](0025-versioned-local-configuration-and-policy.md).

## Decision

### Separate artifact-local syntax from generation-scoped resolution

The Rust graph has two explicit stages:

1. A pure Rust analyzer consumes one immutable source byte string. It emits
   declaration definitions using the existing Rust declaration profile and
   emits raw graph sites using a separate versioned graph-site profile.
2. A generation resolver consumes a complete immutable generation, its
   connected workspace/source-slot identities, artifact facts, and effective
   semantic configuration. It publishes resolution outcomes and edges scoped
   to that generation.

Raw sites never contain a generation ID or a resolved symbol ID. Resolved
outcomes never become part of a reusable per-file analysis artifact.
Generation publication remains atomic: a failure, cancellation, deadline, or
newer source epoch leaves the prior active generation readable and publishes no
partial graph.

The graph-site artifact key includes the exact content digest, language,
grammar/runtime identities, graph-site profile version, exact first-party
implementation fingerprint, and every semantics-affecting configuration input.
The resolver profile and connected source-slot set participate separately in
the generation projection identity.

### Exact occurrence identity

One raw site is identified within an artifact by:

```text
graph-site artifact identity
    + source-order ordinal
    + site kind
    + exact half-open occurrence byte span
    + exact half-open target byte span
```

The target span must be contained by the occurrence span and both must be
contained by the exact source blob. The stored raw target spelling must equal
the bytes at the target span. Line and column values are derived display data,
not identity.

Each site may carry an enclosing-definition descriptor containing the existing
declaration kind, exact name, deterministic syntax-qualified path, name span,
and declaration span. The descriptor is an artifact-local join aid, not a
logical-symbol identity and not correspondence proof.

### Raw site kinds and evidence

Profile 1 emits only these bounded source-ordered site kinds:

- `import`: one exact Rust `use` argument, retaining aliases, nested lists,
  `self`, `super`, `crate`, and globs as raw syntax;
- `reference`: a precision-first candidate identifier, type path, or field
  expression that is not a definition, binding, import target, call target,
  macro target, or attribute;
- `call`: the exact function expression of a Rust call expression;
- `macro_call`: the exact macro path of a macro invocation; and
- `test_marker`: a direct `#[test]`-style attribute or a conservative
  `cfg(...test...)` marker.

Every raw site labels its extraction evidence:

- `direct_syntax` means the pinned grammar directly identifies the construct;
- `syntax_heuristic` means bounded syntax inspection classifies a candidate,
  such as an identifier reference or a `test` token inside a configuration
  predicate.

Neither label claims that a target exists. Macro token-tree contents are not
reparsed as ordinary Rust, and generated definitions or sites are not
fabricated.

Malformed syntax remains visible through exact parser error/missing-node
coverage. A bounded Tree-sitter result may still be complete output for the
profile while reporting syntax errors. Source bytes, syntax nodes, depth, site
count, individual names, individual paths, and aggregate owned output text have
independent hard ceilings. Limit, cancellation, deadline, encoding, parser, or
invariant failures return no output.

### Resolution outcomes preserve zero, one, or many targets

For each raw site, the generation resolver records exactly one categorical
outcome:

- `unresolved` with zero targets and a bounded reason;
- `unique` with one target; or
- `ambiguous` with two or more deterministically ordered candidate targets.

An ambiguous outcome is not represented by several independently confident
edges. Candidate count, truncation, evidence class, resolver profile, concrete
generation, and relevant coverage are retained with the outcome.

Resolution evidence is categorical and attributed:

- direct compiler or future SCIP identity may provide semantic evidence;
- Rust module/import and lexical scope rules implemented by the versioned local
  resolver provide syntax evidence; and
- exact-name, receiver-shape, or other incomplete matching remains heuristic
  evidence.

Heuristic evidence cannot be upgraded merely because only one candidate was
found. Deterministic ordering is not a confidence signal.

Definitions remain the existing exact declaration occurrences. The first
resolver may connect imports and conservative lexical references. Call edges
are published only with their actual evidence class; a syntactic call site is
not by itself a resolved-call claim.

### Rust limitations remain explicit

Profile 1 does not evaluate Cargo features, target triples, build scripts, or
arbitrary `cfg` predicates. Sites under conditional source remain syntactic
candidates, and test-marker presence does not prove that a test is compiled or
executed.

Macro invocations are visible, but token trees and expanded output are not
treated as resolved ordinary source. Procedural macros, declarative macro
expansion, hygiene, and generated files require separately attributed compiler
or expansion evidence.

Trait methods, blanket implementations, associated items, UFCS, deref
coercions, function pointers, closures, trait objects, and other dynamic
dispatch may legitimately resolve to zero, one, or many candidates. The local
syntax resolver must abstain or retain ambiguity rather than emulate the Rust
compiler incompletely.

Coverage reports count syntax errors, skipped candidate classes, macro
boundaries, conditional markers, unresolved sites, ambiguous sites, candidate
truncation, and unsupported semantic evidence. Missing coverage is never
converted into confidence.

### Trace and impact are deterministic bounded projections

`graph_trace` and `impact_analyze` operate only on one pinned immutable
generation. A request admits:

- an exact starting occurrence or definition;
- an allow-list of edge kinds and one explicit direction;
- positive maximum depth, visited-node, visited-edge, frontier, path, result,
  and encoded-output limits;
- one monotonic deadline and cooperative cancellation signal; and
- effective policy no broader than ADR-0025's configured ceilings.

Traversal uses a stable ordering by depth, edge-kind rank, evidence rank,
repository/source-slot identity, normalized source path, occurrence span, and
stable target identity. Cycles are detected by generation-local identity.
Results expose the visited counts, returned counts, maximum completed depth,
coverage, and whether each bound truncated the frontier. A cancellation,
deadline, generation change, corrupt row, or invariant failure returns no
partial success envelope.

Impact is a conservative graph projection, not a build-break prediction.
Inbound unique call/reference/import edges may support `directly_connected`;
ambiguous or heuristic paths remain `possible`; unsupported dynamic,
conditional, and macro-generated behavior is explicitly `unknown`. Trace and
impact do not expose a general graph-query language.

### Memory remains independent

Graph resolution does not edit, relink, approve, invalidate, or otherwise
mutate engineering memory. Memory revalidation continues through the accepted
correspondence, Git-validity, approval, and review contracts. Graph evidence
may later be presented as an attributed review candidate only through a
separate accepted decision. No edge, including a unique high-confidence edge,
silently changes a memory attachment.

### Context compilation uses a new profile

The accepted Phase 0 context profile remains unchanged. Graph candidates enter
context compilation only through a separately versioned profile 2 with its own
provider coverage, fusion behavior, candidate ceilings, expansion depth,
budget allocation, omission taxonomy, and evaluation gate. Enabling profile 2
must be explicit; it cannot silently change profile-1 ordering or output.

Graph-expanded context items retain the concrete generation, originating site,
resolved outcome, edge evidence, complete source selector, provider-local rank,
and truncation state. Raw scores from unrelated providers are not compared.

### Persistence is a migration-3 responsibility

Migration 3 implements the reviewed relational persistence contract for raw
sites, generation-scoped resolution outcomes, and typed edges.

Under ADR-0026's provisional migration-3 assembly, graph tables replace only
the graph fragment's assigned responsibility. Workspace identity remains owned
by `0003_phase1_workspace.sql`, and retention remains owned by
`0003_phase1_retention.sql`. The fixed-order fragments preserve
[ADR-0022](0022-squash-pre-release-sqlite-schema.md)'s pattern: they execute as
one ordered transaction and record one migration-3 ledger row with one exact
checksum. The baseline and migration 2 remain byte-for-byte unchanged.

Artifact-local site rows reference immutable analysis artifacts. Resolved
outcomes and typed edges reference one immutable generation and cannot point
across generations. Activation exposes source search and graph projections
together or neither. Foreign keys, triggers, uniqueness constraints, encoded
fixed-width counts, stable categorical spellings, and hostile-row decoding
enforce the same invariants as application construction.

## Alternatives considered

### Persist resolved targets inside analysis artifacts

This makes a file artifact depend on every other file and source-slot choice.
It prevents safe content reuse and can carry stale edges into a new generation.

### Resolve every call to the first exact-name candidate

This produces convenient-looking graphs but hides overload-like ambiguity,
traits, imports, methods, macros, and dynamic dispatch behind arbitrary stable
ordering.

### Run compiler expansion as part of the built-in analyzer

Compiler evidence could improve precision, but it adds toolchain, build-script,
feature, target, dependency, time, and hostile-execution boundaries. It belongs
in a separately supervised and attributed evidence provider.

### Extend the accepted context profile in place

This would change established ordering and budgets for existing callers without
an explicit compatibility choice or evaluation baseline.

### Store graph relationships in engineering-memory records

Source-derived graph facts change with generations and are reproducible.
Engineering memory is an append-only human/project knowledge system with
separate trust and temporal semantics.

## Consequences

### Positive

- Immutable syntax work is reusable while cross-file resolution remains fresh.
- Exact spans and raw spellings keep every edge inspectable.
- Ambiguity and unsupported Rust semantics remain visible.
- Trace, impact, and graph-expanded context have explicit resource contracts.
- Graph evidence cannot silently corrupt trusted memory attachments.

### Negative and risks

- The two-stage model requires separate identities, coverage, and persistence.
- Syntax-only reference and call recall is intentionally incomplete.
- Conditional compilation and macros can produce both inactive candidates and
  missing generated sites until stronger evidence is available.
- Deterministic ambiguity may return more data than a guessed single target and
  needs strict candidate ceilings.
- Migration 3 and context profile 2 require separate compatibility reviews
  before the end-to-end feature is available.

## Validation

- Golden tests cover exact source-order ordinals, occurrence and target spans,
  enclosing descriptors, imports with aliases/nested lists/globs and
  `self`/`super`/`crate`, references, free/method/UFCS/dynamic calls, macro
  calls, direct tests, and conditional test markers.
- Adversarial tests cover shadowing, duplicate names, trait ambiguity, malformed
  syntax, invalid UTF-8, macro token trees, cancellation, elapsed deadlines,
  parser reuse after interruption, conditional-marker token classification, and
  inclusive source/node/depth/site/name/path/aggregate-output boundaries.
- Property and differential fixtures prove deterministic output, span
  containment, exact raw spelling, source ordering, no duplicate identity, and
  no result on failure.
- Resolver tests cover zero/one/many targets, stable candidate ordering,
  conditional and macro limitations, cross-slot identity, stale-generation
  rejection, cancellation, and atomic publication.
- Migration-3 tests cover exact checksum/ledger identity, baseline and
  migration-2 upgrades, schema introspection, immutable artifact rows,
  generation isolation, atomic activation, recovery, backup, corrupt rows, and
  clean-versus-incremental graph equivalence.
- Trace and impact tests cover cycles, every bound independently, explicit
  frontier truncation, evidence propagation, stable ordering, and
  mixed-generation rejection.
- Context-profile-2 tests compare lexical/source-only, graph-only, and fused
  packs without changing accepted profile-1 golden output.

## Follow-up

- Implemented locally under this proposed contract: the bounded Rust graph-site
  analyzer, conservative generation resolver, migration-3 persistence and
  atomic publication, categorical evidence and coverage, architecture, trace,
  and impact use cases, and thin CLI and MCP adapters.
- Maintainer ratification of this ADR, the connected-workspace identity
  contract, migration 3, and the associated resource budgets remains a Phase 1
  gate. The implementation does not make a proposed decision accepted.
- Context profile 2 remains separate from the existing context compiler and
  requires its own proposal and evaluation without changing profile 1.
- Package-aware resolution, macro expansion, compiler or SCIP evidence,
  dynamic dispatch proof, and cross-language graph edges remain explicitly
  deferred. Add any such evidence only through a separately versioned,
  attributed provider.

## Supersession

None.
