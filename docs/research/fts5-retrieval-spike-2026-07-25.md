# Phase 0 FTS5 retrieval spike

- Status: Implemented and promoted
- Observed: 2026-07-25
- Last updated: 2026-07-26
- Scope: lexical retrieval over generation-scoped facts

## Question

Can bundled SQLite FTS5 provide the bounded deterministic lexical candidate
stage required by Phase 0 without mixing immutable generations or exposing
FTS query syntax directly to an untrusted caller?

SQLite documents FTS5 as a virtual-table full-text engine, notes that queries
without an explicit order are arbitrary, and defines a query language with
boolean, prefix, phrase, NEAR, and column operators. It also specifies that
embedded double quotes inside a quoted FTS string are escaped by doubling
them. See the official
[FTS5 documentation](https://sqlite.org/fts5.html).

## Candidate

The test-only fixture uses:

- a `STRICT` generation-fact table with fixed path and text bounds;
- a disposable FTS5 projection keyed by the fact row ID;
- a reader-resolved active generation joined into every search;
- `unicode61` with diacritic removal disabled and underscore treated as a
  token character;
- weighted `bm25()` ordering followed by exact path and fact ordinal
  tie-breakers;
- a result limit in `1..=100`;
- a 256-byte, eight-term literal query profile with 64 bytes per term;
- every caller term double-quoted and every embedded quote doubled before the
  complete FTS expression is bound as a SQL parameter.

The literal profile deliberately does not expose FTS boolean, prefix, NEAR,
column-filter, or raw phrase syntax. Rich syntax, if ever supported, needs a
separate versioned parser and policy rather than interpolation.

## Results

Focused tests prove:

- a query against active generation 2 cannot return matching generation-1
  rows;
- repeated searches have identical order and result limits are inclusive;
- quote, boolean, and prefix-looking hostile input remains literal and cannot
  alter SQL or FTS expression structure;
- deleting and rebuilding one generation's FTS projection preserves logical
  results;
- empty, oversized, over-term, over-term-byte, and invalid-result-limit inputs
  fail closed with stable redacted diagnostics.

The exact bundled SQLite candidate already passes the runtime-version and
`ENABLE_FTS5` compile-option checks in the
[generation spike](sqlite-generation-spike-2026-07-23.md). The focused test
and Clippy run pass with warnings denied.

## Production promotion and remaining evaluation

ADR-0012 is accepted and the
[Phase 0 SQLite v2 schema](../schemas/phase0-sqlite-v2.md) now promotes the
validated design through production owned readers. The implementation pins
one active generation per read transaction, installs a deadline/cancellation
progress callback, enforces row and encoded-output limits, and returns content
and artifact identities plus byte spans. The shared application use case now
adds canonical query admission and identity, exact pre-limit counts,
syntax-producer attribution, categorical resolution, a lexical-only
limitation, and explicit index/query coverage. The installed CLI exercises the
same use case through the SQLite port and emits canonical path text rather than
lossy platform paths. The local stdio MCP adapter exposes that application use
case through bounded versioned DTOs; installed-binary tests cover
initialization, literal admission, exact selectors, cancellation,
backpressure, output limits, stale generations, and real-repository
round-trips.

The remaining evaluation gate is pinned-corpus relevance, P50/P95/P99
latency, encoded result size, database/WAL size, and projection-rebuild
measurement. That gate ratifies product budgets; it does not leave the bounded
retrieval path unimplemented.
