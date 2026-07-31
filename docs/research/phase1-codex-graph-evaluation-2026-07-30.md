# Phase 1 Codex graph evaluation

- Status: Completed, proposed-phase evidence only
- Observed: 2026-07-30 UTC
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
wrapped the successful results in the version-1 evaluator-only envelope from
proposed [ADR-0034](../adr/0034-phase1-codex-graph-evaluation.md).

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

## Scope and remaining gate

This demonstrates the declared `minimum_complete_runs` against the public
pinned corpus. It does not ratify proposed Phase 1 budgets, ADR-0034, a public
MCP-envelope contract, or the broader Phase 1 release gate. Maintainer review
and the remaining proposed-gate evidence remain required.
