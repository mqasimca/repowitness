# Repository-path boundary-encoding research

- Status: Implemented and promoted
- Date: 2026-07-23
- Last updated: 2026-07-26
- Scope: Textual wire and Git-memory representation of `RepositoryPath`

## Question

RepoWitness preserves Git repository paths as arbitrary non-NUL bytes, while
MCP and human-reviewed Git-memory formats cross Unicode text boundaries. The
encoding must be lossless, canonical, versioned, bounded, deterministic across
platforms, strict under hostile input, and implementable without leaking a wire
DTO into the domain package.

## Primary-source findings

- JSON strings represent Unicode characters, and interoperable network JSON is
  UTF-8. Arbitrary Git path bytes therefore cannot be placed directly in a JSON
  string without a separate binary-to-text encoding.
- MCP specification version 2025-11-25 uses UTF-8 JSON-RPC. Its stdio transport
  also forbids embedded newlines in a message, favoring an ASCII encoding with
  no whitespace.
- RFC 4648 requires rejection of characters outside the selected alphabet
  unless a referring specification explicitly says otherwise. Its Base16
  alphabet uses uppercase `0-9A-F`, represents every octet with two
  characters, and needs no padding.
- Base16 preserves unsigned-byte lexicographic ordering when every value has
  the same prefix. Base64 and Base64url do not preserve that ordering under
  ordinary textual comparison.

## Options

| Encoding | Size | Canonicality and review | Decision |
|---|---:|---|---|
| Uppercase Base16 with a short versioned tag | `7 + 2n` bytes | One simple form, order-preserving, padding-free, easy to validate and inspect | Selected |
| Base64url | About `7 + 4n/3` bytes | Smaller, but padding and pad-bit rules are more complex and textual order differs from path order | Reject for version 1 |
| Percent-encoded bytes | Between `n` and `3n` bytes | Readable for some ASCII, but requires a larger canonical escape policy and special treatment of `%` and separators | Reject |
| UTF-8 when possible, binary encoding otherwise | Variable | Human-friendly common case, but creates two representations and requires a canonical-form predicate around controls and Unicode | Reject |
| JSON array of byte integers | Much larger | Lossless but verbose, awkward in schemas, and exposes many numeric parser edge cases | Reject |
| CBOR byte strings | Compact | Native bytes, but incompatible with the required MCP JSON-RPC text boundary and Git-reviewed YAML/JSON | Reject |

## Recommended version 1 profile

The canonical scalar is:

```text
rwp1:h:<UPPERCASE-BASE16>
```

`rwp1` identifies RepoWitness repository-path text version 1 and `h` identifies
Base16. The payload contains exactly two uppercase RFC 4648 Base16 characters
per repository-path byte. It contains no padding, separators, whitespace, or
lowercase characters. Decoders reject unknown tags, odd payloads, lowercase,
and every non-alphabet character rather than normalizing them.

Encoding accepts only an already validated `RepositoryPath`. Decoding:

1. enforces the encoded-input byte limit;
2. verifies the exact tag and an even, non-empty payload;
3. derives and enforces the decoded path-byte limit before allocation;
4. strictly decodes the canonical alphabet once into owned bytes; and
5. constructs `RepositoryPath`, applying its component and traversal rules.

The adapter-neutral canonical scalar belongs in `repowitness-application`
because both local Git-memory and MCP adapters need it. It is not a Serde,
MCP, database, or Git-memory DTO. Each adapter retains its own versioned DTO
and maps through this scalar to the domain type. SQLite continues to store
repository identity as a BLOB.

An adapter may add display text generated from the validated bytes. Display is
not canonical identity, is not decoded, and must not appear in default logs.

## Implementation follow-up

Accepted [ADR-0011](../adr/0011-repository-path-text-encoding.md) promotes this
profile. `repowitness-application` implements bounded canonical encode/decode
with golden vectors, every valid component byte, ordering equivalence, hostile
input, and redacted error coverage. CLI and MCP retrieval DTOs use the scalar;
SQLite continues to store exact path bytes.

## Initial corpus size

The initial clean Linux corpus was measured using the revisions recorded in
the [path-identity research](path-identity-2026-07-23.md). Counts below cover
only the canonical scalar, excluding surrounding JSON/YAML fields and optional
display text.

| Repository | Paths | Raw path bytes | Encoded scalar bytes | Longest scalar |
|---|---:|---:|---:|---:|
| `netwhy` | 48 | 906 | 2,148 | 81 |
| `nvctl` | 115 | 2,727 | 6,259 | 97 |

These samples support implementation testing, not final production defaults.
Hard encoded and decoded ceilings remain mandatory even after configurable
lower defaults are selected.

## Primary sources

- IETF [RFC 8259: JSON](https://www.rfc-editor.org/rfc/rfc8259.html),
  especially strings, UTF-8 interoperability, and parser limits
- IETF [RFC 4648: Base-N encodings](https://www.rfc-editor.org/rfc/rfc4648.html),
  especially non-alphabet rejection, canonical encoding, Base16, and test
  vectors
- MCP specification 2025-11-25
  [Transports](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports),
  requiring UTF-8 JSON-RPC and newline-free stdio messages
- RepoWitness [repository-path identity research](path-identity-2026-07-23.md)
