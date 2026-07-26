# ADR-0011: Encode repository paths as tagged uppercase Base16 at text boundaries

- Status: Accepted
- Date: 2026-07-23
- Owners: Project maintainers
- Scope: MCP, Git-memory, configuration, export, and other textual repository-path fields

## Context

Accepted [ADR-0010](0010-repository-path-identity.md) represents repository
paths as exact Git bytes. MCP JSON-RPC and Git-reviewed JSON/YAML are Unicode
text boundaries and cannot carry every valid Git path byte directly.

The encoding must have one canonical form, preserve exact bytes and ordering,
reject malformed hostile input, enforce limits before allocation, remain
independent of target-local host paths, and avoid introducing protocol types
or serialization dependencies into `repowitness-domain`.

The dated
[boundary-encoding research](../research/repository-path-boundary-encoding-2026-07-23.md)
compares the primary specifications and candidate encodings.

## Decision

Repository-path text encoding version 1 is the ASCII scalar:

```text
rwp1:h:<UPPERCASE-BASE16>
```

Its grammar is:

```text
repository-path-text-v1 = "rwp1:h:" 1*(HEXDIG-UPPER HEXDIG-UPPER)
HEXDIG-UPPER = DIGIT / %x41-46
```

The payload is RFC 4648 Base16 using uppercase `0-9A-F`, with exactly two
characters per repository-path byte and no padding, whitespace, separators, or
ignored characters. The prefix identifies RepoWitness repository-path text
version 1 and its Base16 payload.

Encoders accept only a validated `RepositoryPath`. Decoders:

1. enforce an explicit encoded byte limit before inspecting the payload;
2. require the exact prefix and an even non-empty payload;
3. derive and enforce the decoded repository-path byte limit before
   allocation;
4. strictly decode the canonical uppercase alphabet once into owned bytes; and
5. construct `RepositoryPath`, which enforces component count, traversal, NUL,
   `.git`, and other identity rules.

Unknown versions or encoding tags are errors. Decoders do not accept lowercase,
strip whitespace, ignore non-alphabet characters, or translate an alternative
form. This prevents multiple textual values from representing one identity.

Canonical text equality and ordering use the encoded ASCII bytes. Because every
value has the same prefix and Base16 digit order follows nibble order, textual
ordering matches `RepositoryPath` unsigned-byte ordering.

The shared, serialization-independent scalar and codec live in
`repowitness-application`, the existing multi-adapter boundary. MCP,
Git-memory, export, and configuration layers keep separate versioned DTOs and
map through it. The domain package does not gain Serde, MCP, database, or text
encoding types. SQLite continues to persist path identity as a BLOB.

An adapter may include separately bounded display text generated from the
validated path. Display text is never decoded into identity, is excluded from
canonical equality and hashing, and is not logged by default.

## Alternatives considered

### Base64url

It reduces expansion from two characters per byte to roughly four per three
bytes. It adds padding or omitted-padding rules, canonical pad-bit validation,
and a textual ordering different from repository-path ordering. Version 1
prefers the simpler dependency-free decoder; revisit only if measured boundary
size is material.

### Percent-encoded bytes

It keeps some ASCII readable but needs a larger canonical escaping policy,
expands hostile or non-ASCII paths to three characters per byte, and makes `%`
and `/` handling easier to implement inconsistently.

### UTF-8 text with a binary fallback

It improves common-case display but creates two representations and needs a
stable predicate for controls, separators, Unicode, and display safety.
Optional non-canonical display text provides readability without weakening
identity.

### JSON byte arrays

They are lossless but substantially larger, awkward in schemas and reviews,
and expose more numeric parsing surface.

### CBOR byte strings

They represent bytes directly but do not satisfy MCP's required UTF-8 JSON-RPC
boundary or Git-reviewed text formats.

### Untagged Base16

It cannot evolve without out-of-band schema knowledge and makes an encoding
mistake harder to diagnose.

## Consequences

### Positive

- Every valid repository identity has one portable text representation.
- Strict decoding has small, auditable, dependency-free code.
- Encoded ordering remains identical to repository-path ordering.
- The alphabet is safe inside JSON, YAML, command output, and newline-delimited
  transports without additional binary escaping.
- Version and encoding are visible in every scalar.

### Negative and risks

- Each path byte expands to two payload bytes, plus the seven-byte prefix.
- Hex is less readable than a normal path; optional display fields must remain
  clearly non-canonical.
- Every boundary must enforce both encoded and decoded limits.
- Independent DTO implementations must share golden conformance fixtures.
- A future encoding change requires a new tag and migration, never silent
  decoder widening.

## Validation

- Golden vectors for ASCII, control bytes, non-UTF-8 bytes, case, Unicode, and
  Windows-looking repository components.
- Exhaustive byte round trips through valid adversarial path components.
- Rejection tests for empty/odd payloads, lowercase, whitespace, padding,
  non-alphabet characters, unknown tags, and invalid decoded paths.
- Exact-limit and one-over-limit tests for encoding and decoding, proving the
  decoded byte limit is checked before allocation.
- Ordering tests comparing encoded and domain sorting.
- Redacted debug and error tests that never expose path or payload content.
- Real-repository encode/decode round trips for the pinned sibling corpus.
- Fuzzing and cross-language golden fixtures before a public wire format ships.

## Revisit conditions

- Measured path payload size materially harms MCP latency, Git-memory review,
  or storage after compression and batching are evaluated.
- A required standard protocol field adopts a different canonical binary
  representation.
- Cross-language conformance finds ambiguity in the version 1 grammar.

## Implementation status

Implemented in `repowitness-application` and used by CLI and MCP retrieval
DTOs. Golden, exhaustive valid-byte, hostile-input, exact-limit, ordering, and
redaction tests pass; SQLite retains raw repository-path bytes.

## Supersession

None.
