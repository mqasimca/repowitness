# Phase 1 Codex graph evaluation

- Status: Local proposed-phase evidence only; clean attestation pending
- Observed: 2026-07-30 and 2026-07-31 UTC
- Corpus: `tokio-rs/mini-redis`
- Base revision: `7295d727b82a0ef534b836b00807c15ef6c7f191`
- Changed revision: `3d93b42bc363220f85af4fc9e1bebd35b588a4a3`
- Response contract:
  [`../../benchmarks/phase1/codex-decision-v2.schema.json`](../../benchmarks/phase1/codex-decision-v2.schema.json)

## Method

The opt-in local evaluator built a fresh isolated clone and SQLite database for
each evaluation session. For both revisions it ran the existing comparison
probe, requested `context_build`, requested `graph_search` for `Frame::check`,
then traced the exact returned definition under fixed MCP limits. The runner
wrapped the successful results in the then-current version-1 evaluator-only
envelope from proposed [ADR-0034](../adr/0034-phase1-codex-graph-evaluation.md).

`codex exec` ran in an ephemeral read-only session with approval, web, MCP
servers, apps, shell access, hooks, memories, goals, plugins, and multi-agent
facilities disabled. The final answer and JSONL event stream had fixed byte
limits and a 600-second deadline. The validator rejected tool events,
out-of-envelope citations, missing graph evidence, invalid current-memory use,
stale-memory use, and decision or usefulness mismatches.

## Results

Three isolated base/changed pairs completed. Every base answer returned
`bug-present`, cited graph and source evidence plus current memory, and marked
the packet useful. Every changed answer returned `bug-fixed`, cited graph and
source evidence, cited no memory, and marked the packet useful. All six event
streams had zero tool events. The validator reported zero stale-memory uses and
one graph citation in each answer.

| Measure | Result |
|---|---:|
| Complete base/changed pairs | 3 / 3 |
| Validated decisions | 6 / 6 |
| Graph-cited answers | 6 / 6 |
| Source-cited answers | 6 / 6 |
| Base answers using current memory | 3 / 3 |
| Changed answers using memory | 0 / 3 |
| Tool events | 0 |
| Total input tokens | 147,420 |
| Total output tokens | 1,648 |
| Total tokens | 149,068 |

The graph trace explicitly reported its bounded coverage and truncation; it was
provided to Codex as a limitation rather than treated as complete graph proof.
The decisions are grounded in the supplied source and memory evidence, not in
ambiguous or heuristic trace relationships.

## Version-2 local repeat

On 2026-07-31, the current local evaluator repeated all three isolated
base/changed pairs with the version-2 envelope. Every base answer returned
`bug-present`, used source, graph, and current-memory evidence, and every
changed answer returned `bug-fixed`, used source and graph evidence, and cited
no memory. All six JSONL event streams contained zero tool events. The
validator reported all three complete runs, source grounding, current-memory
use at the base revision, stale-memory exclusion at the changed revision, and
usefulness as passing.

This repeat used the version-2 trace-request selector binding and its current
schema/golden/self-test contract. It was run from a dirty local working tree,
not a clean committed release revision. It is therefore implementation
evidence only; it does not replace the clean attested rerun required for
ADR-0034 or Phase 1 ratification.

## Scope and remaining gate

The historical result demonstrates the declared `minimum_complete_runs`
against the public pinned corpus for the version-1 envelope. Version 1 did not
retain the canonical trace request or bind its selector to a returned
definition, so it is not evidence for the current version-2 envelope. The
local version-2 repeat closes that implementation-validation gap but must be
rerun from a clean attested revision before it can support ADR-0034, budget,
public-contract, or broader Phase 1 release-gate decisions.
