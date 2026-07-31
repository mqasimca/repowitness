# Phase 2 local context evaluation

- Status: Completed local evidence
- Observed: 2026-07-31 UTC
- Profile: `phase2-evaluation-v1`
- Corpus: [`tokio-rs/mini-redis`](https://github.com/tokio-rs/mini-redis)
- Revision: `3d93b42bc363220f85af4fc9e1bebd35b588a4a3`

## Method

[`../../scripts/run-phase2-evaluation`](../../scripts/run-phase2-evaluation)
requires a clean checkout of the public pinned corpus. It builds the local
release CLI, indexes into a disposable SQLite database, and executes five warm
`phase2-context-build` requests for `run`. The runner requires available
syntax and reference providers, requires an explicit unavailable SCIP provider
receipt for this corpus, and requires at least one graph-relation item.

It also runs the RepoWitness-authored public synthetic direct-call-chain test.
That test compares lexical selector retrieval, graph-only selector retrieval,
the supported Phase 0 context, and Phase 2. Phase 2 supplies the anchor plus
its direct call target, whereas the incumbent supplies the anchor only; its
two required source lines occupy fewer units per relevant line than the
incumbent's single line.

The history regression changes an approved, historically observed memory
record's source evidence and revalidates it. It proves neither the memory nor
history provider can emit the now-stale record. `--agent` is opt-in: it gives
one packet to an ephemeral read-only Codex session with tools, web, MCP,
memories, goals, apps, hooks, and plugins disabled. The consumer must identify
the listener/handler path and receives no memory evidence.

Run locally:

```text
./scripts/run-phase2-evaluation --agent /path/to/mini-redis
```

## Result

The local run completed with all receipt checks passing.

| Measure | Result |
|---|---:|
| Warm Phase 2 context builds | 5 / 5 |
| Warm p95 | 77.116 ms |
| Synthetic lexical / graph / incumbent / Phase 2 comparison | Passed |
| Stale memory or history items emitted after revalidation | 0 |
| Downstream Codex consumer runs | 1 / 1 |
| Stale memory items available to that consumer | 0 |

The public corpus has no imported SCIP overlay, so the receipt correctly
reports precise-overlay provider availability as `unavailable`; it does not
mistake that absence for budget omission. The CLI/MCP SCIP fixture separately
proves that an unambiguous exact overlay occurrence takes precise precedence
without removing syntax evidence.

This is local implementation evidence from a dirty working tree. It is not a
clean-commit release attestation and does not replace the separately deferred
fresh macOS or Windows evidence.
