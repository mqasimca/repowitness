# Phase 2 SCIP decoder spike

- Status: Active
- Reviewed: 2026-07-31
- Scope: Bounded local import of explicitly supplied SCIP indexes under
  [ADR-0035](../adr/0035-phase2-scip-precision-overlay.md)

## Question

Can RepoWitness decode a hostile SCIP index in bounded document batches without
adding a decoder, generated binding, or source-provenance surface that violates
the local precision-overlay contract?

## Primary sources reviewed

- The official [SCIP repository](https://github.com/scip-code/scip), which
  publishes the language-neutral Protobuf schema and Rust bindings.
- The official [`scip.proto`](https://github.com/scip-code/scip/blob/main/scip.proto),
  reviewed 2026-07-31. It requires the singular `Index.metadata` field first
  for streaming, assigns documents to field 2, defines protocol version 0,
  separates on-disk `Metadata.text_document_encoding` from per-document range
  `PositionEncoding`, and recommends consuming an index one field at a time.
  The reviewed revision is `44d39fcfc95486d066a796e2cec8c7ec5d429aae`; the
  exact `scip.proto` SHA-256 is
  `b38021b65ef90cbbf6af9c829ff75192859ad9b5da05439ef154bea4ceb2bf03`.
- The official [`scip` 0.9.0 crate](https://docs.rs/scip/0.9.0/scip/), released
  from that repository under Apache-2.0.
- The official [`protobuf` 3.7.2 crate](https://docs.rs/protobuf/3.7.2/protobuf/),
  the exact runtime required by `scip` 0.9.0, under MIT.

## Candidate: `scip` 0.9.0

`scip` 0.9.0 is maintained in the official `scip-code/scip` repository. It has
an Apache-2.0 license and declared Rust 1.81 MSRV, both compatible with the
repository's allowed licenses and Rust 1.97.1 baseline. It has no build script,
native code, or feature selection surface of its own.

Its exact direct runtime dependency is `protobuf` 3.7.2. That package is MIT,
has no enabled optional feature in this candidate path, and introduces no
native code or build script. Its declared MSRV is absent, so compatibility must
be proven by the locked workspace build rather than assumed from metadata.

The candidate is **not selected**. Its generated `Index` type stores all
documents in one repeated `Vec<Document>`, and its ordinary `Message` decode
therefore materializes the complete index before RepoWitness can validate or
persist a document. That violates ADR-0035's independent document, occurrence,
relationship, decoded-metadata, and retained-memory bounds. A file-size ceiling
alone cannot make that behavior a bounded batch import.

## Current bounded-decoder result

`repowitness-analysis` now contains a dependency-free bounded outer-wire
scanner and a provider-neutral overlay adapter. The scanner enforces input,
metadata, document, ignored-field, document-count, occurrence, relationship,
symbol, deadline, and cancellation ceilings. It yields one borrowed document
at a time. The adapter requires immutable source bytes from an explicit lookup,
checks canonical repository paths and exact manifest content digests, validates
legacy and typed ranges against the pinned bytes, and emits bounded domain-only
occurrence and relationship batches. It never exposes Protobuf values through
the domain API.

The outer API requires the caller to stage document batches until outer framing
finishes successfully; malformed trailing data, cancellation, deadline, and
sink rejection therefore cannot produce a completed import result. It retains
neither a full decoded index nor raw diagnostic/document text. Repeated source
symbols in relationships are constrained by a per-document owned-symbol budget
so a small wire message cannot expand into an unbounded owned fact batch.

RepoWitness currently accepts only SCIP metadata declaring UTF-8 source files.
That is intentional: the pinned source adapters validate UTF-8 bytes, while
SCIP's metadata encoding describes on-disk text independently from document
range encoding. UTF-16 source metadata is rejected before any document batch
is emitted rather than being misinterpreted. UTF-8, UTF-16, and UTF-32
*position* units remain validated precisely against accepted UTF-8 source
bytes.

This is an integrated analysis boundary plus a canonical immutable overlay
identity, not yet the complete product import: it has no SQLite receipt or
migration, transactional activation, CLI command, MCP read response, or
package-aware query contract. It makes no coverage or precision claim by
itself.

The importer exposes version 1 plus the reviewed schema revision and digest as
explicit constants. The later immutable receipt must include these values with
the source/view/configuration/producer and input identities required by
ADR-0035.

## Required follow-up spike

The selected decoder must prove all of the following before it is introduced:

1. Parse only the fixed supported SCIP schema version and reject unsupported
   protocol/position encodings before facts are constructed.
2. Consume the repeated document field in bounded batches without retaining the
   complete decoded index or raw untrusted diagnostic text.
3. Enforce field, nesting, count, byte, deadline, and cancellation limits
   during wire decoding—not after materialization.
4. Produce validated provider-neutral document batches without leaking
   Protobuf types into domain or application APIs.
5. Pass malformed-wire, declared-length overflow, unknown-field, duplicate,
   excessively nested, and adversarial allocation fixtures under a fixed memory
   ceiling.

Possible approaches are a narrowly audited streaming reader over one reviewed
schema version, or generated bindings paired with a streaming wire layer. The
first needs a focused security and fuzz review; the second needs recorded
provenance, exact upstream revision, license compatibility, notices, and
maintainer approval before generated schema output is added. Neither approach
is selected by this spike.

## Dependency decision

Do not add `scip` 0.9.0 or `protobuf` 3.7.2 to the production dependency graph
yet. Preserve ADR-0035's streaming requirement. The current bounded decoder
uses no new dependency; any replacement still requires the recorded
supply-chain review before selection.

## Revisit conditions

Revisit when a maintained official binding exposes document-level streaming
with independently enforceable limits, or when the bounded-decoder prototype
has its security, fuzz, MSRV, license, and resource evidence.
