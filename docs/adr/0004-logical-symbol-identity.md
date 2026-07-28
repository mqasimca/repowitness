# ADR-0004: Separate logical symbols, occurrences, and correspondence

- Status: Accepted
- Date: 2026-07-22
- Last reviewed: 2026-07-28
- Owners: Project maintainers
- Scope: Symbol identity, refactor tracking, memory attachment, and impact analysis

## Context

RepoWitness needs memories and historical facts to remain connected through supported renames and moves without attaching trusted knowledge to the wrong code. No property derived from one syntax tree is universally stable:

- qualified names and paths change during renames and moves;
- signatures change during compatible edits and overload evolution;
- semantic fingerprints change when implementations change and may collide for similar code;
- Git diff similarity operates on text/files rather than language meaning;
- compiler and SCIP identities have producer-specific limits.

False automatic relinks are more dangerous than missed relinks because they can make stale knowledge appear current.

## Decision

Model three independent concepts:

1. `LogicalSymbol`: an opaque RepoWitness-assigned durable ID plus current descriptors.
2. `SymbolOccurrence`: one definition/reference at an exact repository, revision/worktree, file, range, generation, and evidence producer.
3. `Correspondence`: a versioned same/moved/renamed/split/merged relationship between occurrences or logical symbols, with evidence, method, categorical assurance, and audit history.

Names, paths, signatures, container structure, content digests, and semantic fingerprints are matching signals, not durable IDs.

Automatic correspondence uses the strongest available evidence in this order:

1. compiler or SCIP identity whose producer documents cross-revision behavior;
2. Git-aware, language-structural matching;
3. exact semantic fingerprint plus compatible package/container evidence;
4. language-specific heuristics.

Only configured high-assurance rules can relink active, high-trust memory automatically. Ambiguous results create candidate correspondences and mark affected memory `needs_review`. A human can approve, reject, or manually establish correspondence; the action and actor are audited.

Structural/AST differencing is a useful correspondence producer, not proof of identity. Its output remains attributed to an algorithm/version and is calibrated per language/fixture. Thresholds and feature weights belong to a versioned correspondence profile; a score is never exposed as a universal probability.

Splits and merges create explicit many-to-many correspondence. Active memory is not copied or reactivated across a split/merge unless a reviewed policy and evidence support that action.

## Alternatives considered

### Qualified name as identity

Simple and readable, but fails exactly when symbols are renamed, moved, re-exported, overloaded, or placed in a new package.

### Semantic fingerprint as identity

Useful as a signal but unstable under meaningful edits and vulnerable to collisions among generated or repetitive code.

### Git rename detection only

Useful for file moves but insufficient for symbols, splits/merges, copy detection, and uncommitted worktrees.

### Always create new IDs

Safe from false links but prevents memory from surviving routine supported refactors and degrades historical navigation.

## Consequences

### Positive

- Ambiguity and uncertainty become explicit domain states.
- Evidence tiers can improve identity without changing the stable API concept.
- Manual correction becomes auditable training/evaluation data.
- Split/merge behavior is represented rather than hidden in one-to-one assumptions.

### Negative and risks

- Correspondence storage, ranking, review, and lifecycle logic adds substantial complexity.
- Different language adapters provide different guarantees.
- Opaque IDs require tooling for diagnosis and export.
- Thresholds can be overfit to small fixtures.
- AST mapping algorithms can return plausible but incorrect mappings; adding more structural signals does not eliminate the need to abstain.

## Validation

Every language adapter publishes identity guarantees and runs fixtures covering:

- file and symbol rename/move;
- signature and body-only edits;
- overloads, duplicate/generated code, and nested symbols;
- copy versus move;
- split and merge;
- delete and reintroduce;
- branch divergence and merge;
- ambiguous and missing evidence.

Report false automatic relinks, missed relinks, ambiguity/abstention, manual-review rate, and resulting stale-memory outcomes separately by language and evidence tier. The Phase 0 release corpus permits no known false automatic relink for an active decision/failure fixture.

Evaluate identity features individually and as an ensemble. A change to an algorithm, grammar, compiler/SCIP producer, or threshold profile invalidates the corresponding calibration result and artifact key.

## Open questions

- Opaque ID encoding and namespacing.
- Exact structural fingerprint inputs for the Rust adapter.
- How manual correspondence is exported with Git memory.
- Which evidence, if any, can safely propagate memory across splits or merges.

## Implementation status

The current Rust slice implements exact revision-specific symbol occurrences,
source/content/artifact identity, attributed syntax evidence, exact occurrence
retrieval, versioned correspondence fingerprints, precision-first
rename/exact-move matching, immutable correspondence audit, manual review, and
memory relinking. Durable logical-symbol assignment and automatic
split/merge/container-move correspondence remain unimplemented; those cases
abstain or require explicit review.

## Supersession

None.
