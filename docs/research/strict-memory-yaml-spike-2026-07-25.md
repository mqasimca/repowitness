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

Primary sources:

- [serde-saphyr upstream](https://github.com/bourumir-wyngs/serde-saphyr)
- [granit-parser upstream](https://github.com/bourumir-wyngs/granit-parser)
- [serde_json_canonicalizer upstream](https://github.com/evik42/serde-json-canonicalizer)
- [RFC 8785](https://www.rfc-editor.org/rfc/rfc8785)
- [RFC 8785 verified errata](https://www.rfc-editor.org/errata/rfc8785)
