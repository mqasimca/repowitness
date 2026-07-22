# RepoWitness

**A temporal, evidence-backed code-intelligence and engineering-memory engine.**

RepoWitness is a planning-stage, local-first MCP server and CLI intended to help coding agents understand a repository, retain verified engineering experience, and recognize when that knowledge no longer applies.

Its defining promise is:

> Every retrieved fact explains where it came from, how precise it is, when it was true, and what could invalidate it.

## Intended differentiators

- proof-carrying code retrieval with explicit coverage and limitations;
- engineering memory connected to source evidence and Git history;
- refactor-aware memory correspondence and staleness detection;
- token-budgeted context compilation;
- Git-reviewable team memory with local personal memory;
- SQLite-first local operation, with a demand-gated PostgreSQL server mode later;
- a Rust core designed for deterministic, bounded, crash-safe indexing.

## Current status

RepoWitness is currently in product and architecture planning. There is no usable implementation yet.

The complete research-backed proposal, architecture, delivery phases, evaluation strategy, and initial backlog are in [plan.md](plan.md).

The first implementation milestone is deliberately narrow: index one language, retrieve evidence, attach a verified decision or failure, change the associated code, revalidate the memory, and compile an updated context pack.

## License

A project license has not been selected yet. Until a license is added, do not assume permission to copy, modify, or redistribute the contents of this repository.
