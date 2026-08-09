# Phase 2 local context evaluation

- Status: Phase 2 exit gate met
- Observed: 2026-07-31 UTC
- Profile: `phase2-evaluation-v1`
- Corpus: [`tokio-rs/mini-redis`](https://github.com/tokio-rs/mini-redis)
- Revision: `3d93b42bc363220f85af4fc9e1bebd35b588a4a3`

## Method

The retired evaluation harness required a clean checkout of the public pinned
corpus. It built the local release CLI, indexed into a disposable SQLite
database, and ran the lexical
selector, graph-only selector, supported Phase 0 context builder, and Phase 2
context builder for `run`. It performs five warm builds for each context
profile. The runner requires available syntax and reference providers, an
explicit unavailable SCIP-provider receipt for this corpus, and at least one
Phase 2 graph-relation item.

The RepoWitness-authored public synthetic test compares all four baselines for
two independent direct-call navigation tasks. For each task, Phase 2 supplies
the anchor plus its direct call target, while the supported Phase 0 context
supplies the anchor only. The test asserts that the two required source lines
have a higher relevant-lines-per-content-unit density than the one-line Phase
0 packet. The lexical and graph-only baselines remain selectors, not context
packs.

The pinned public-corpus density check covers three direct-navigation requests:
`run`, `handle_command`, and `subscribe`. Before either context profile runs,
the graph-only baseline traces each exact matched declaration one outbound hop
and retains only unique targets. The immutable target artifact digest and fact
ordinal form the task's relevant-source label set. The lexical and graph-only
baselines are selectors and therefore do not supply source lines. The Phase 0
and Phase 2 packets are then measured against the same labels, deduplicated by
source fact, over their complete emitted content budgets. Label construction
does not inspect a Phase 2 packet. This measures the intended direct-navigation
task, not general text-search relevance.

The SCIP CLI/MCP fixture imports one exact, source-verified overlay occurrence.
It requires the precise-overlay item and an available syntax-provider receipt
in the same Phase 2 result, proving precise navigation does not suppress syntax
coverage.

The history regression changes an approved, historically observed memory
record's source evidence and revalidates it. It proves neither the memory nor
history provider can emit the now-stale record. `--agent` is opt-in: it gives
the same two downstream tasks to an ephemeral read-only Codex session for both
the Phase 0 and Phase 2 packets. Tools, web, MCP, memories, goals, apps,
hooks, and plugins are disabled. Each answer must report
`memory_evidence=absent`; answers claiming support from a memory record fail
the runner. The Phase 2 shutdown task must also use listener/handler navigation
evidence. This makes the stale-answer comparison explicit: zero unsupported
memory uses for Phase 2 may not exceed the Phase 0 count.

## Result

The clean pinned-corpus run completed with all receipt checks passing.

| Measure | Result |
|---|---:|
| Lexical / graph-only / Phase 0 / Phase 2 baseline receipts | 4 / 4 |
| Warm Phase 0 context builds / p95 | 5 / 5 / 2.970 ms |
| Warm Phase 2 context builds / p95 | 5 / 5 / 77.930 ms |
| Two-task synthetic lexical / graph / incumbent / Phase 2 comparison | Passed |
| Exact SCIP navigation with syntax coverage still available | Passed |
| Stale memory or history items emitted after revalidation | 0 |
| Downstream Codex tasks, Phase 0 / Phase 2 | 2 / 2 |
| Stale-memory uses in downstream answers, Phase 0 / Phase 2 | 0 / 0 |
| Graph-labelled corpus navigation tasks | 3 |
| Relevant source lines / content units, Phase 0 | 0 / 16,543 = 0 |
| Relevant source lines / content units, Phase 2 | 135 / 22,120 = 0.006103 |

The public corpus has no imported SCIP overlay, so the receipt correctly
reports precise-overlay provider availability as `unavailable`; it does not
mistake that absence for budget omission. The CLI/MCP SCIP fixture separately
proves that an unambiguous exact overlay occurrence takes precise precedence
without removing syntax coverage.

Decision: the Phase 2 exit gate is met. Precise navigation retains syntax
coverage, the public synthetic and graph-labelled corpus tasks improve relevant
source lines per content unit, and Phase 2's downstream stale-answer count does
not exceed the Phase 0 baseline. The all-emitted-declaration proxy remains
lower for Phase 2, as expected for a broader unlabelled measure; it is not used
as a substitute for task relevance. This is local implementation evidence, not
a release attestation. Fresh macOS and Windows evaluator evidence remains
intentionally deferred by maintainer direction and is not included in this
decision.
