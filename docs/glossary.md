# Glossary

- Status: Draft
- Last reviewed: 2026-07-26

## Core terms

### Claim

A statement RepoWitness can return or store, such as “symbol A calls symbol B” or “ledger writes must remain append-only.” A claim is evaluated with evidence, scope, time, and resolution status.

### Material result

A versioned envelope containing a claim, item-bounded attributed supporting or contradicting evidence, categorical resolution, one concrete source snapshot and active generation, item-bounded warnings or limitations, and coverage. Resolved claims require supporting evidence and cannot hide contradictory evidence; unresolved or indeterminate outcomes expose why they remain incomplete. Concrete components and boundary encodings carry their own byte limits.

### Evidence

An inspectable source supporting or contradicting a claim. Its source identity includes repository, concrete revision or worktree snapshot, normalized repository-relative path, content digest, and an explicit whole-file, half-open byte-span, or symbol-occurrence location. The evidence record attributes that source to a separate producer identity and version. Line numbers are display metadata.

### Evidence tier

The origin and expected precision of a code fact: compiler/SCIP, LSP, syntax, heuristic, runtime observation, or human assertion. A tier describes provenance, not a universal probability of correctness.

### Resolution status

A categorical outcome such as `confirmed`, `inferred`, `ambiguous`, `unresolved`, or `indeterminate`. RepoWitness does not expose probability-like confidence numbers until they can be calibrated on labeled data.

### Coverage receipt

Metadata describing what a request searched, skipped, could not resolve, or truncated. It prevents a partial answer from appearing exhaustive.

### Index generation

An immutable, internally consistent publication of derived code facts for one source snapshot and producer manifest. Writers build a staging generation and activate one pointer atomically; readers never observe half an update.

### Source snapshot

An exact canonical source manifest plus repository, complete Git, worktree/submodule, resolved configuration/policy, and analyzer/grammar/producer/schema identities. A dirty worktree snapshot is content identity, not a Git commit or a claim of filesystem-wide atomic capture.

### Source manifest

A file-count-bounded, canonical list of unique validated normalized paths, file types, and exact content digests. The domain contract sorts entries by the normalized path type's stable ordering and rejects duplicates; it does not itself choose path encoding, file policy, or digest algorithms.

### Repository-path text encoding

The canonical textual form of a validated repository path: `rwp1:h:` followed
by strict uppercase RFC 4648 Base16 for the exact path bytes. Encoded and
decoded sizes are bounded before allocation. Optional display text is
non-canonical and is never decoded into identity.

### Analysis artifact

Immutable per-file facts produced from one source blob and every semantics-affecting analyzer/configuration input. Generations reuse artifacts when their complete keys match.

### Analysis artifact key

The complete logical reuse identity: exact source digest, adapter/grammar/producer identity, resolved semantics-affecting configuration, extraction schema, and canonicalization version. Its persisted digest must use a versioned, domain-separated canonical encoding of every component.

### Query context

The request-scoped workspace, generation, source snapshot, policy/authorization, deadline, cancellation, and resource budgets used by every application query. It prevents hidden selection of changing global state.

### Logical symbol

A RepoWitness-assigned durable identity representing a code concept across supported changes. Names, paths, signatures, and fingerprints help match occurrences but are not themselves permanent identity.

### Symbol occurrence

A symbol at an exact repository, revision or worktree, file, range, and index generation.

### Correspondence

A versioned relationship connecting occurrences or logical symbols across revisions, such as same, moved, renamed, split, or merged. Correspondence carries evidence, method, and categorical assurance; ambiguity remains explicit.

### Memory record

A scoped engineering claim whose kind may be decision, failure, procedure, episode, preference, policy, or non-source-derivable fact. Records have immutable versions, provenance, lifecycle, validity, and audit history.

The strict Phase 0 record is still proposed in ADR-0014; the current YAML and
canonical-digest implementation is a test-only spike.

### Project-valid time

The revisions where a memory claim applies. RepoWitness interprets introduction and invalidation commits through Git ancestry rather than treating commit history as one linear interval.

### System-recorded time

When RepoWitness knew a particular record version. Immutable versions make historical “as known at” queries possible.

### Stale memory

A memory whose evidence or attached code changed enough that continued applicability has not been established. Stale is not the same as false; it means revalidation is required.

### Context pack

A deterministic, token-budgeted collection of source, relationships, tests, history, and eligible memory assembled for an intent. It includes omissions and a coverage receipt.

Context-pack compilation is not implemented in the current Phase 0 source
indexing and retrieval slice.

### Workspace

A query and storage boundary containing one or more related repositories. Related repositories belong in one workspace when cross-repository relationships must be queried together.

### Team memory

Git-tracked, reviewable memory stored under `.code-memory/` in the application repository for the initial product.

### Personal memory

User-scoped memory stored outside the repository in a local, optionally encrypted store.

### Canonical tool

A stable, compact RepoWitness MCP operation. The current local stdio server
implements `code_search` and `symbol_get`; `context_build` is planned for the
remaining Phase 0 loop.

### Compatibility alias

A bounded adapter exposing an incumbent tool name. Name, schema, and behavior compatibility are reported separately; an alias is not automatically a drop-in implementation.
