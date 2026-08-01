# Phase 3 durable-memory longitudinal benchmark

- Status: Proposed; not ratified
- Manifest: [`manifest.json`](manifest.json)

This contract reuses only the public, externally referenced `mini-redis`
corpus and historical revisions already declared by the Phase 0 and Phase 1
manifests. It vendors no corpus material and records no maintainer-local
repository information.

Each case runs the scoped, revalidated-memory candidate and both declared
baselines against the same revision, question, and bounded source-evidence
budget. Five fresh disposable-state paired runs are required for every base and
changed snapshot. The runner records aggregate counts only and rejects any
personal-to-team, cross-repository, or sensitive-scope-output leakage.

The opt-in runner is available locally, but it is not an attestation and is not
run in ordinary CI:

```text
./scripts/run-phase3-longitudinal <envelope-root> <receipt.json> [model]
```

It requires an installed, authenticated Codex CLI and exact input envelopes
materialized by the candidate and baseline harnesses. No envelope is supplied
in this repository because it would either embed an outcome or copy benchmark
corpus content. The required bounded input layout is:

```text
<envelope-root>/<comparison-id>/<base|changed>/run-<1..5>/<variant>.json
```

where `variant` is `candidate`, `source-only`, or `naive-memory-text`. Each
envelope conforms to [`codex-envelope-v1.schema.json`](codex-envelope-v1.schema.json),
and its packet is consumed only by a disposable, read-only Codex process. The
runner verifies that every pair has the same revision, question, budget, and
canonical source-evidence sequence. Packets declare one bounded source-evidence
list plus a variant-constrained memory mode: none for `source-only`, only
unvalidated entries for `naive-memory-text`, and scoped revalidated entries for
the candidate (including current memory at the base snapshot). Every run uses a
distinct disposable-state identifier. It deletes detailed prompts, events,
answers, and stderr before it returns. The only publishable output is the aggregate receipt validated by
[`phase3-longitudinal-receipt-v1.schema.json`](phase3-longitudinal-receipt-v1.schema.json)
and `scripts/check-phase3-longitudinal-receipt`.

Before packets are materialized, validate the public pinned source identities
without printing or retaining corpus content:

```text
./scripts/check-phase3-public-scenarios /path/to/public-corpus
```

`source_discriminator` is the authoritative fixture admission check. The
historical `lexical_signals` fields remain descriptive compatibility metadata;
they are not sufficient to admit a Phase 3 packet on their own.

The runner refuses to write a receipt unless all 30 required Codex executions
complete and the measured aggregate satisfies the proposed strict baseline and
zero-leakage relations. A valid receipt is explicitly `not-attested`; it is
evidence for review, not a public-beta claim. `attestation.status` remains
`not-collected` until a maintainer records independently reviewed longitudinal
evidence.
