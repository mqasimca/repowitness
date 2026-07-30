# Phase 0 design-partner evaluation protocol

- Status: Completed
- Protocol version: 1
- Last reviewed: 2026-07-30
- Scope: The completed Phase 0 product-outcome gate

## Goal

Determine whether RepoWitness evidence or recalled engineering memory changes
a useful decision on one real Rust engineering task. This is a product-outcome
evaluation, not another parser, indexing, or synthetic correctness test.

The task must come from active engineering work, a real incident, a prior
failed approach, or a maintained design constraint. A task created only to
demonstrate RepoWitness does not qualify.

## Required roles and inputs

- One design partner who owns or understands the task well enough to judge the
  decision and evidence.
- One concrete commit or exact worktree snapshot from a real Rust repository.
- One bounded task question that requires an engineering decision.
- Relevant source evidence.
- A pre-existing decision, failure, or constraint memory when memory is part of
  the expected improvement.
- One maintainer who records the protocol results and preserves the private
  evidence outside this public repository.

The design partner may also be a maintainer, but an isolated model run cannot
declare its own output useful or correct.

## Privacy boundary

The repository, source, and evaluation packet remain external. Do not add or
log repository names, organizations, local or remote paths, revisions, task
text, source, symbols, memory text, model transcripts, credentials, personal
data, or per-repository measurements in the public RepoWitness repository or
default logs. Local processing under the normal containment and redaction
policy is expected.

Use public labels such as `partner-01` and `task-01`. Do not derive those labels
from private values. The maintainer may keep the private mapping, exact inputs,
and full outputs in an access-controlled location.

The public outcome must state that its detailed evidence is confidential. It
must not present the result as independently reproducible from the public
record.

## Comparison method

1. Pin one exact source state and record it privately.
2. Confirm the task is real and record its origin category privately.
3. Prepare three bounded context variants from that same source state:
   - lexical or source-only evidence without engineering memory;
   - naive memory text without evidence or temporal validation; and
   - RepoWitness context with attributed source evidence and only eligible
     current memory.
4. Use separate, fresh agent sessions with the same model, question, limits,
   and runtime-tool policy. Do not let one session see another result.
5. Validate that every RepoWitness source citation resolves to its exact packet
   evidence and that no stale, conflicted, indeterminate, quarantined,
   superseded, contradicted, tombstoned, or review-needed memory enters the
   packet.
6. Have the design partner rate each decision for correctness and usefulness
   without treating model confidence as evidence.
7. Record whether RepoWitness evidence or recalled failure changed the useful
   engineering decision relative to both declared baselines.

If the task does not need memory, the outcome can still pass only when
RepoWitness evidence changes the useful decision relative to both baselines.
Do not invent memory to force a positive result.

## Pass criteria

The Phase 0 outcome passes only when all conditions hold:

- the task is real and the design partner confirms its relevance;
- the RepoWitness decision is correct and useful;
- the decision is grounded in exact RepoWitness evidence;
- RepoWitness changes the useful decision relative to the source-only and
  naive-memory baselines;
- every admitted memory item is current for the pinned source state;
- stale-memory uses equal zero;
- coverage gaps, abstentions, and unresolved work remain explicit;
- no confidential input appears in the public record; and
- the design partner confirms the recorded categorical outcome.

An equal decision can still be a useful observation, but it does not pass this
Phase 0 gate. A false confirmed claim, hidden truncation, mixed-generation
result, false automatic relink, unsupported certainty, or stale-memory use
fails the outcome.

## Public result record

Record only these fields in a dated follow-up document:

```text
protocol_version=1
partner_id=partner-NN
task_id=task-NN
language_scope=rust
source_state_kind=commit|worktree
task_origin=active_work|incident|failed_approach|design_constraint
source_only_decision=correct|incorrect|abstained
naive_memory_decision=correct|incorrect|abstained
repowitness_decision=correct|incorrect|abstained
repowitness_source_grounded=true|false
repowitness_current_memory_used=true|false
repowitness_stale_memory_uses=0
coverage_complete=true|false
decision_changed=true|false
partner_rating=useful|not_useful
partner_confirmed=true|false
outcome_pass=true|false
```

The result must also name the model and version, the bounded agent runtime
policy, the evaluation date, the number of isolated runs, an opaque
non-identifying custodian ID, and any categorical limitation that affected the
decision. Do not include private values while doing so.

## Completion action

After a passing result:

1. add the privacy-reviewed public outcome record;
2. decide ADR-0018 using the design-partner evidence plus its completed
   adversarial matrix;
3. decide ADR-0021 after ADR-0018; and
4. update the roadmap and product status to state that Phase 0 is complete.

If the outcome fails, keep both ADRs proposed, record the categorical failure
without confidential details, and fix or narrow the product claim before
running a new real task.
