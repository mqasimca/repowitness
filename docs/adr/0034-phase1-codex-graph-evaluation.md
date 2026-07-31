# ADR-0034: Evaluate Phase 1 graph packets through a bounded evidence envelope

- Status: Accepted
- Date: 2026-07-30
- Owners: Project maintainers
- Scope: Phase 1 opt-in Codex evaluation, MCP graph evidence presentation, and
  benchmark validation only

## Context

The Phase 1 benchmark declares an opt-in Codex evaluation that must judge an
actual MCP graph packet, cite its evidence identifiers, use current memory at
the base revision, reject stale memory after the source change, and report the
packet useful. The implemented Phase 0 evaluator already isolates `codex exec`,
disables runtime tools, captures JSONL events, enforces bounded output and a
deadline, and validates citations. Its packet and validator, however, accept
only `context_build` source and memory items. They cannot establish that a
graph result, graph selector, generation, resolution outcome, or coverage was
actually supplied to the model.

ADR-0027 defines generation-scoped graph evidence and requires explicit
ambiguity, coverage, and bounds. Passing graph text through a context-only
validator would turn missing graph provenance into unsupported certainty.
Adding ad-hoc identifiers to ordinary MCP responses would instead broaden the
public graph contract without a versioned review.

## Decision

### Use a versioned evaluator-only envelope

The opt-in Phase 1 runner constructs one canonical JSON evaluation envelope
from bounded successful responses to the declared local MCP graph tools and
the pinned memory/context provider. The envelope is evaluator input only; it
does not change the MCP wire schema, SQLite schema, normal CLI output, or
accepted Phase 0 context profile.

The envelope contains:

- one exact graph workspace view and graph generation shared by its search and
  trace results, while source and memory items retain their existing context
  provider provenance;
- the canonical trace request, including its exact selector and declared
  bounds; the selector must match exactly one definition returned by the
  enclosed graph search. Query text and host paths stay out of normal receipts;
- graph source evidence, resolution category, edge evidence class, coverage,
  omissions, and truncation exactly as returned by MCP; and
- current or stale memory presentation with its existing effective-state and
  revalidation evidence.

The runner rejects mixed graph generations, mismatched graph selectors,
missing trace coverage or truncation fields, an incomplete search result, or
any response above the fixed envelope byte limit. It does not reconstruct graph
facts from source text or storage rows.

### Bind citations to supplied graph evidence

Each envelope item receives one wrapper-only `evaluation_evidence_id`. A graph
identifier's canonical input is the evaluator-envelope version, concrete graph
generation, item kind, and canonical JSON of that item's MCP evidence and
coverage fields. Source and memory retain their existing context-item identity.
The IDs are prefixed by their kind:

```text
graph/<sha256>
source/<sha256>
memory/<sha256>
```

The runner retains the canonical preimage only in its private bounded
temporary directory and emits neither host paths nor raw source outside the
already-supplied packet. A cited ID must identify exactly one supplied item.
The validator rejects duplicate IDs, absent IDs, IDs outside the envelope, and
memory whose effective state is not `current`.

At least one cited item must be graph or source evidence from the supplied
generation. A base-revision answer that claims current-memory use must cite
current memory; a changed-revision answer must cite no stale memory. Citations
never make an ambiguous or heuristic graph relation semantically confirmed.

### Keep the Codex process isolated and bounded

The runner uses non-interactive `codex exec` with an ephemeral session,
read-only sandbox, no approvals, empty MCP configuration, disabled web, apps,
shell, hooks, memories, goals, plugins, and multi-agent facilities, JSONL
events, and a versioned output schema. It supplies the envelope in the prompt
and validates the final structured answer plus every emitted event. A tool
event, unsupported event, timeout, output-limit breach, malformed JSON, or
schema failure fails the evaluation without a partial pass.

The runner is opt-in and never receives API credentials from GitHub Actions,
repository-controlled build steps, or the corpus checkout. It may use an
operator's local authenticated Codex CLI only after all corpus preparation and
RepoWitness work have completed. A future CI integration requires a separate
credential-isolation decision.

## Alternatives considered

### Reuse the Phase 0 context-only evaluator unchanged

It is implemented and familiar, but cannot prove that graph evidence or its
generation, ambiguity, and coverage were visible to Codex.

### Add evaluator identifiers to every normal MCP response

This would make a benchmark-only presentation field a public API commitment
and risks callers treating it as a durable graph identity.

### Let Codex call the local MCP server directly

This broadens runtime authority, allows tool use to hide missing supplied
evidence, and makes result identity depend on an uncontrolled interaction
sequence.

### Cite graph display strings or symbol names

Names and display spans are not stable generation evidence identities and can
be ambiguous, redacted, truncated, or reused across source slots.

## Consequences

### Positive

- The evaluation proves that the model saw concrete generation-pinned graph
  evidence without expanding the public MCP contract.
- Citation validation stays deterministic, bounded, and independent of model
  explanations.
- The evaluator preserves ADR-0027's ambiguity and coverage limits instead of
  flattening graph results into source-only certainty.

### Negative and risks

- The envelope is an additional versioned benchmark format and needs golden
  fixtures plus hostile-input tests.
- It measures one pinned corpus and declared task, not graph quality at scale.
- Local Codex authentication and model availability remain operator-dependent;
  a failed invocation is evidence of no result, not a product regression.

## Validation

- Golden/self-test envelopes cover exact generation pinning,
  graph/source/current-memory citation IDs, and stable canonical ordering.
- Adversarial fixtures reject mixed generations, altered evidence fields,
  ambiguous relations presented as confirmed, stale-memory citations, missing
  coverage, duplicate IDs, tool events, oversized output, timeout, and
  malformed structured answers.
- Three isolated successful base/changed Codex pairs satisfy the manifest's
  `minimum_complete_runs`, expected decisions, source grounding, memory-use,
  stale-memory exclusion, and usefulness requirements.
- The existing Phase 0 evaluator remains unchanged and keeps its own fixtures.

## Follow-up

- Completed: add the version-2 envelope schema, canonical golden fixture,
  hostile-input self-tests, runner, and manifest contract checks. The retained
  version-1 fixture is historical only: it lacked the trace-selector binding
  required for a trustworthy graph-evidence claim.
- Dated research reports record the clean repeated version-2 evaluation and
  release-platform attestation. They support this decision alongside the other
  Phase 1 evidence; they do not independently establish Phase 1 readiness.
- Revisit the envelope only through a new version when MCP graph evidence or
  coverage semantics change.

## Supersession

None.
