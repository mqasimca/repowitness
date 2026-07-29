# Phase 0 controlled comparative evaluation

- Status: Provisional measurement
- Observed: 2026-07-28 UTC
- Corpus: `tokio-rs/mini-redis`
- Base revision: `7295d727b82a0ef534b836b00807c15ef6c7f191`
- Changed revision: `3d93b42bc363220f85af4fc9e1bebd35b588a4a3`
- Benchmark manifest:
  [`../../benchmarks/phase0/manifest.json`](../../benchmarks/phase0/manifest.json)

## Question

The controlled task asks whether `Frame::check` still accepts negative RESP
bulk lengths other than `-1`.

At the base revision, the implementation accepts that input. The changed
revision rejects it and adds a regression test. The evaluation records one
approved memory claim about the base behavior, then compares three strategies
after moving to the fixed revision:

- bounded lexical source search without an index or memory;
- naive memory text without evidence or temporal validation; and
- RepoWitness source evidence plus revalidated memory.

This is a deterministic public correctness evaluation. It is not a substitute
for the separate real design-partner outcome gate.

## Method

The runner creates two disposable clones. The existing product-loop clone
measures the full current-revision lifecycle. The comparison clone starts at
the base revision and uses a separate database.

The comparison performs these steps:

1. Index the base revision.
2. Scan every discovered Rust source file for the two bounded lexical signals
   declared by the evaluation.
3. Write and locally approve the base-behavior memory with exact
   `Frame::check` evidence.
4. Revalidate it as current and prove that context includes it.
5. Move the disposable clone to the changed revision.
6. Incrementally index the exact changed files.
7. Repeat the bounded lexical scan.
8. Revalidate the memory as stale and prove that context excludes it.
9. Record that the naive baseline still exposes the same unlabeled text.

The lexical baseline shares only RepoWitness's sanitized Git discovery and
capability-contained file-reading boundary. It does not use the index,
extracted facts, graph relationships, context compiler, or memory.

Every scan has explicit path, output, file, aggregate-byte, and deadline
bounds. The benchmark sample count is restricted to 2 through 100 before any
clone, allocation, or build starts.

## Environment

| Field | Observed value |
|---|---|
| RepoWitness revision | `f4f1c05e1081ca9e12797849267feabb7a2e5a08`, dirty working tree |
| Toolchain | `rustc 1.97.1`, release profile, locked dependency graph |
| Operating system | Darwin 25.5.0 arm64 |
| CPU | Apple M4 Pro |
| Logical CPUs | 14 |
| Memory | 24,576 MiB |
| Filesystem | APFS |
| Complete repeated runs | 5 |

The implementation remained uncommitted as required. These results are
development evidence, not a clean-revision release attestation.

## Results

All five complete runs passed.

| Observation | Base revision | Changed revision |
|---|---:|---:|
| Rust files scanned | 27 | 28 |
| Source bytes scanned | 148,783 | 149,820 |
| Old-behavior lexical matches | 1 | 0 |
| Fix-evidence lexical matches | 0 | 1 |
| Lexical evidence relation | `supports` | `contradicts` |
| RepoWitness memory state | `current` | `stale` |
| RepoWitness memory included in context | yes | no |
| Naive memory claim exposed | yes | yes |
| Incremental files analyzed | 27 | 2 |
| Incremental files reused | 0 | 26 |

After the fix, the naive baseline exposed one obsolete, temporally unlabeled
claim. RepoWitness exposed zero stale claims and reported the omission from
context explicitly. The automated decision classification changed from
`bug-present` to `bug-fixed`.

Median timings from the five complete runs were:

| Operation | Median |
|---|---:|
| Current-revision full product cold index | 394.751 ms |
| Current-revision unchanged warm index | 297.394 ms |
| Comparison base index | 383.809 ms |
| Comparison changed incremental index | 307.060 ms |
| Base lexical scan | 35.119 ms |
| Changed lexical scan | 35.494 ms |

The lexical timing is not a quality comparison. It performs two fixed literal
searches and returns no context pack, memory, temporal state, provenance, or
coverage beyond the scan. RepoWitness performs indexing, persistence,
evidence binding, revalidation, and context compilation.

## Interpretation

The controlled evaluation establishes three points:

- lexical source search can detect both versions when the correct signals are
  known, but it retains no engineering decision and supplies no temporal
  validity;
- naive text memory retains the decision, but cannot recognize that the source
  now contradicts it; and
- RepoWitness retains the decision while it is supported, marks it stale when
  its exact evidence changes, and excludes it from the next context pack.

This closes the automated comparative-harness gap. Phase 0 still requires a
separate real design-partner task that measures whether this behavior improves
a useful engineering decision. A separate
[Codex utility evaluation](phase0-codex-utility-evaluation-2026-07-28.md)
passes the same controlled before/after decision using actual structured MCP
context, but it is not a real design-partner outcome.

## Reproduction

Use a clean external checkout at the manifest revision:

```text
./scripts/run-phase0-benchmark /path/to/mini-redis
```

The runner validates the supplied revision and worktree, creates disposable
clones, runs both comparisons, fails on any categorical mismatch, and removes
the disposable source, memory, and database state when it exits.
