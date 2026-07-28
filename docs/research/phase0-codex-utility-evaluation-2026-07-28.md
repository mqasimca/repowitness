# Phase 0 Codex utility evaluation

- Status: Provisional measurement
- Observed: 2026-07-28 UTC
- Consumer: Codex CLI 0.145.0 using `gpt-5.6-sol`
- Corpus: `tokio-rs/mini-redis`
- Base revision: `7295d727b82a0ef534b836b00807c15ef6c7f191`
- Changed revision: `3d93b42bc363220f85af4fc9e1bebd35b588a4a3`
- Benchmark manifest:
  [`../../benchmarks/phase0/manifest.json`](../../benchmarks/phase0/manifest.json)

## Question

The evaluation asks:

> At the current source snapshot, does `Frame::check` accept RESP bulk lengths
> below `-1`?

The base revision accepts those invalid lengths. The changed revision rejects
them and adds a regression test. RepoWitness has one approved memory claim
about the old behavior. That memory is current at the base revision and stale
at the changed revision.

The test measures whether a coding agent can use the returned packet to make
the correct temporal decision, ground it in exact source, use current memory,
and avoid stale memory.

## Method

The opt-in runner:

1. validates a clean public corpus checkout at the exact manifest revision;
2. creates disposable base and changed worktrees;
3. runs the controlled comparison to establish the source and memory oracle;
4. obtains a structured `context_build` result through the installed local
   stdio MCP server at each revision;
5. starts a fresh ephemeral Codex process in an empty read-only workspace with
   repository instructions and user configuration disabled;
6. supplies only the question, an explicit untrusted-evidence warning, and the
   MCP packet;
7. disables shell, shell snapshots, web search, apps, MCP servers, hooks,
   memories, goals, remote plugins, and collaboration tools;
8. captures Codex JSONL events and fails the run on any command, file-change,
   MCP, web-search, collaboration, or otherwise unsupported item; and
9. validates the answer against the versioned
   [`codex-decision-v2`](../../benchmarks/phase0/codex-decision-v2.schema.json)
   JSON Schema, then verifies every returned evidence identifier against the
   exact source or current-memory item in the supplied packet.

The response contract records the decision, exact packet evidence identifiers,
source grounding, current-memory use, stale-memory use, packet usefulness, and
limitations. A run passes only when both revisions are correct, both decisions
cite at least one supplied source item, the base decision cites current memory,
the changed decision does not cite memory, every cited memory item is current,
no stale memory is used, and the packet is rated useful.

All source, memory, database, and Codex state created by the runner is
disposable. The supplied corpus remains unchanged.

## Results

Three complete paired runs passed.

| Observation | Base revision | Changed revision |
|---|---:|---:|
| Supplied packet size | 4,601 bytes | 3,698 bytes |
| Correct Codex decision | 3/3 `bug-present` | 3/3 `bug-fixed` |
| Expected decision | `bug-present` | `bug-fixed` |
| Source grounded | 3/3 | 3/3 |
| Exact supplied source citation | 3/3 | 3/3 |
| Current memory used | 3/3 | 0/3 |
| Exact supplied memory citation | 3/3 | 0/3 |
| Stale memory used | 0/3 | 0/3 |
| Runtime tool events | 0/3 | 0/3 |
| Packet usefulness | 3/3 `useful` | 3/3 `useful` |
| JSONL input-plus-output token range | 11,277–11,311 | 10,756–10,838 |

In every base-revision run, Codex used the current memory together with the
exact `Frame::check` declaration. In every changed-revision run, RepoWitness
excluded the stale memory and Codex relied on the changed declaration and
regression evidence. The validator matched every cited identifier to the
supplied packet and observed no tool events.

## Agent-output defect found

The first trial exposed exact declarations as lowercase hexadecimal even when
the source was valid UTF-8. Codex still made the correct changed-revision
decision, but the representation was harder to inspect and unnecessarily
large:

| Changed-revision packet | Size |
|---|---:|
| Original CLI packet with hexadecimal declaration | 5,733 bytes |
| Revised CLI packet with exact UTF-8 declaration | 4,275 bytes |
| Revised structured MCP result | 3,597 bytes |
| Hardened evaluation packet with evidence identifiers | 3,698 bytes |

The revised CLI packet is 25.4% smaller than the original packet. Exact source
is now emitted as UTF-8 when valid and display-safe, and as labeled lowercase
hexadecimal for invalid or display-unsafe UTF-8. MCP keeps the representation in separate
`declaration_encoding` and `declaration` fields. CLI output uses one
JSON-escaped data field so untrusted newlines and control characters cannot
forge report fields.

The hardened evaluator now records input, cached-input, output, and reasoning
counts from the JSONL completion event. Those values are observational and are
not directly comparable to the older formatted CLI total. They do not
establish a causal token-performance improvement because the response contract
and model execution are nondeterministic.

## Interpretation

This controlled public task shows that the Phase 0 packet is useful to Codex
for one evidence-backed before/after decision:

- exact source was sufficient to identify both behaviors;
- current memory added usable historical context at the base revision;
- stale-memory exclusion prevented obsolete guidance at the changed revision;
  and
- labeled, directly readable source improved packet compactness and human/agent
  inspectability.

The runs also demonstrate why agent evaluation belongs in the benchmark loop:
correct storage and retrieval contracts did not reveal the hexadecimal
presentation cost.

## Limitations

- This is one public task, one model, and three complete paired runs.
- Packet usefulness is a schema-constrained model judgment, not an independent
  human design-partner score.
- Raw model token totals are not a stable benchmark budget.
- The task does not exercise references, structural expansion, call graphs, or
  history retrieval, which Phase 0 explicitly omits.
- The tool and event gate prevents runtime filesystem, MCP, app, or web
  retrieval from helping. It cannot remove prior knowledge encoded in model
  weights or eliminate model nondeterminism.
- This result does not satisfy the separate real design-partner outcome or
  clean-revision release-attestation gates.

## Reproduction

Use a clean external checkout at the manifest revision:

```text
./scripts/run-phase0-codex-evaluation /path/to/mini-redis 3
```

An optional third argument selects a bounded, validated Codex model name:

```text
./scripts/run-phase0-codex-evaluation /path/to/mini-redis 3 gpt-5.6-sol
```

The runner requires an installed authenticated `codex` command with the
documented feature-disable and JSONL flags. It validates all categorical
outcomes, rejects tool events, bounds captured output, and removes its
disposable worktrees on exit.
