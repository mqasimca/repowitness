# Strict memory YAML and canonical-digest spike

- Status: Recommendation
- Observed: 2026-07-25
- Scope: test-only candidate for the ADR-0007 memory-file boundary

## Question

Can a maintained Rust stack satisfy the accepted
[Git-memory synchronization contract](../adr/0007-git-memory-synchronization.md)
without hashing presentation YAML or admitting YAML features that change
meaning invisibly?

The previously common
[`serde_yaml`](https://docs.rs/serde_yaml/latest/serde_yaml/) package is
deprecated. This spike therefore evaluates `serde-saphyr` 0.0.29 with
`granit-parser` 0.0.7, Serde 1.0.229, and
`serde_json_canonicalizer` 0.3.2. All are exact, development-only workspace
pins. The candidate DTO is intentionally incomplete and is not a production
memory schema.

## Candidate boundary

The test boundary applies these steps in order:

1. Reject input over 64 KiB and invalid UTF-8 before YAML parsing.
2. Stream raw YAML events with limits of 4,096 events, 2,048 nodes, depth 8,
   and one document. Reject every tag, anchor, and alias event.
3. Decode a `deny_unknown_fields` DTO with duplicate-key errors, merge-key
   errors, strict booleans, no implicit number-to-string conversion, zero
   anchors/aliases, and explicit parser scalar/comment budgets.
4. Validate schema version, record ID, string sizes, parent count, lowercase
   SHA-256 parent encodings, and duplicate parents. Sort parents because their
   order is not semantic in the candidate.
5. Serialize validated semantics with
   [RFC 8785 JSON Canonicalization Scheme](https://www.rfc-editor.org/rfc/rfc8785),
   excluding the display revision as explicit presentation metadata.
6. Hash a versioned, length-framed canonical byte string with SHA-256 under
   the `RepoWitness\0memory-record\0` domain and return the domain
   `CanonicalMemoryDigest` type.

The raw-event pass is necessary. A focused regression proved that typed
`serde-saphyr` deserialization alone accepted `!secret` on a string and lost
the tag before DTO validation. The extra pass catches that case without a
lexical substring scan, while the small fixed input limit bounds the cost of
parsing twice.

## Results

The six focused tests prove:

- a stable canonical JSON and digest golden vector;
- key order, comments, parent order, and display revision do not change the
  digest;
- record ID, kind, title, body, tombstone, and parent changes do change it;
- duplicate keys, anchors, aliases, custom tags, merge keys, implicit floats
  in string fields, unknown fields, and multiple documents fail closed;
- input, schema, path-like ID, and collection bounds fail closed;
- parse and validation diagnostics do not expose memory content.

Focused tests and Clippy pass with warnings denied. `cargo deny --locked check`
passes all advisory, license, ban, and source policies. The exact dependency
review is recorded in the
[Phase 0 dependency report](phase0-dependency-review-2026-07-25.md).

## Recommendation

Keep the candidate test-only until the complete memory schema defines every
field, encoding, optionality rule, transport-only field, and canonical JSON
integer profile. Promotion also requires:

- an ADR or focused schema that settles the remaining ADR-0007 questions;
- fuzzing both raw-event and typed passes, including directives, malformed
  UTF-8, deep nesting, scalar bombs, and unusual tags;
- resource measurements over realistic memory histories;
- a maintenance decision for a stack whose reviewed stable YAML release is
  still pre-1.0 and whose current upstream line also has a 1.0 release
  candidate;
- production dependency and source-vetting approval.

Do not use `serde_yaml`, hash YAML bytes, or remove the raw-event tag check
merely because the target DTO appears strict.

## 2026-07-26 upstream review

The focused [Phase 0 memory-record ADR](../adr/0014-phase0-engineering-memory-record.md)
now proposes the missing complete schema and production boundary. It remains
proposed and therefore does not yet promote these dependencies.

Current upstream package metadata still reports `serde-saphyr` 0.0.29 and
`granit-parser` 0.0.7 as the latest stable releases, with 1.0.0-rc.1 available
for both. The serde-saphyr project documents configurable parser budgets and a
panic-free, unsafe-free goal; granit-parser exposes the raw event and tag spans
needed by the independent preflight. The canonicalizer remains
`serde_json_canonicalizer` 0.3.2 and documents RFC 8785 compatibility.

The recommendation is therefore to keep the exact reviewed stable pins, retain
the raw-event pass, and evaluate a stable 1.0 release only after it exists and
passes the complete hostile-input, golden, fuzz, MSRV, dependency, and resource
suite. RFC 8785's verified negative-zero erratum is another reason for version
1 to admit no floating-point values.

## 2026-07-27 pre-acceptance contract review

The proposed ADR was reviewed against ADR-0004, ADR-0005, ADR-0007, ADR-0011,
and ADR-0013. It remains proposed, but now fixes ambiguities that would have
made a released version 1 unsafe to reinterpret:

- record IDs are uniformly random opaque 128-bit values with an exact
  Crockford Base32 bit mapping and no time input;
- repository-authored assurance and lifecycle are separated from effective
  local approval and retrieval eligibility;
- trusted recorded-time/audit metadata is explicitly outside repository YAML;
- every digest encoding, integer/collection/output bound, tagged union, symbol
  kind, relationship, and tombstone rule is explicit; and
- the SHA-256 input frame now fixes its version and length widths and byte
  order.

The proposed
[Phase 0 memory schema](../schemas/phase0-memory-v1.md) records the exact
candidate profile. A separate full-shape, test-only harness verifies commit and
worktree/relationship generated YAML, RFC 8785 canonical JSON, framed digests,
record-ID vectors, every mutable semantic component, set/evidence ordering,
presentation invariance, hostile YAML rejection, deterministic mutation smoke
coverage, independent input/canonical bounds, cross-field validation, and
redacted diagnostics. The original small spike remains useful as an independent
regression for the parser stack.

This evidence does not accept the ADR or promote the dependencies. Explicit
maintainer acceptance plus the roadmap's fuzz, resource, dependency, and
production implementation gates still remain.

## 2026-07-27 synthetic resource measurements

The test-only full-shape harness includes two ignored release-mode probes. They
were built once and then run as the isolated test binary three times per
scenario on:

- hardware model `Mac16,7`, Apple arm64, 24 GiB RAM, and 14 logical CPUs;
- macOS 26.5.2 build 25F84; and
- Rust/Cargo 1.97.1 with the locked release dependency graph.

The first draft probe canonicalized each record twice and XORed an even number
of alternating digest prefixes, yielding a misleading zero checksum. That
measurement was discarded. The corrected probe performs exactly one strict
parse/validation, canonical serialization, and framed SHA-256 operation per
record and accumulates a wrapping 64-bit digest-prefix checksum.

| Scenario | Work per run | Isolated real time | Maximum resident set | Peak memory footprint |
|---|---:|---:|---:|---:|
| Alternating complete commit/worktree vectors | 10,000 records; 15,190,000 input bytes; 14,850,000 canonical bytes | 0.87–0.88 s | 4,358,144–4,374,528 bytes | 2,343,224–2,359,608 bytes |
| Exact 64 KiB input bound, padded only with YAML whitespace | 1,000 records; 65,536,000 input bytes; 1,343,000 canonical bytes | 0.39–0.57 s | 4,194,304–4,276,224 bytes | 2,163,000–2,244,920 bytes |

All six isolated runs completed with zero swaps, page faults, or block I/O.
Their deterministic checksums were `360585392808813880` and
`3558501863875213256` respectively.

These are preliminary adapter measurements, not release budgets. The ordinary
vectors are representative small records, while the maximum-input case is
mostly whitespace rather than maximum semantic content. Cargo, Git-history
import, file admission, audit projection, concurrency, adversarial
canonical-output expansion, and realistic divergent histories are not
measured. Ratified memory-history resource budgets and continuous fuzzing
therefore remain open production gates.

Primary sources:

- [serde-saphyr upstream](https://github.com/bourumir-wyngs/serde-saphyr)
- [granit-parser upstream](https://github.com/bourumir-wyngs/granit-parser)
- [serde_json_canonicalizer upstream](https://github.com/evik42/serde-json-canonicalizer)
- [RFC 8785](https://www.rfc-editor.org/rfc/rfc8785)
- [RFC 8785 verified errata](https://www.rfc-editor.org/errata/rfc8785)
