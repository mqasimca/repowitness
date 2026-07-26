# ADR-0008: Start as a layered modular monolith

- Status: Accepted
- Date: 2026-07-22
- Owners: Project maintainers
- Scope: Rust packages, dependency direction, process boundaries, and application ports

## Context

RepoWitness combines source analysis, Git history, SQLite persistence, temporal memory, retrieval, CLI commands, and MCP transport. A single undifferentiated crate would make it easy for protocol, database, async, and filesystem details to become domain dependencies. Starting with microservices or a generic plugin kernel would instead create distribution and compatibility contracts before the differentiating local loop is proven.

The architecture needs enforceable dependency direction without a crate per feature or an abstraction for every function.

## Decision

Build one local process as a six-package Rust workspace:

| Package | Responsibility |
|---|---|
| `repowitness-domain` | Pure identities, snapshots, evidence, coverage, memory lifecycle, temporal states, and invariants |
| `repowitness-analysis` | Content-to-facts analysis, resolution, correspondence, retrieval, and context-selection algorithms |
| `repowitness-application` | Use cases, request context, policy enforcement, task supervision, and narrow port traits |
| `repowitness-local` | SQLite, Git, filesystem/VFS, watcher reconciliation, local configuration, and bounded execution |
| `repowitness-mcp` | MCP transport, wire DTOs, negotiation, and protocol error mapping |
| `repowitness-cli` | Binary, commands, composition root, and human-facing reports |

Dependencies point inward:

```text
cli -> mcp -> application -> analysis -> domain
cli -> local -> application
             -> analysis
             -> domain
```

Additional rules:

- Domain aggregates do not expose `rusqlite`, Tokio, Tree-sitter, Git, Serde wire, or MCP SDK types.
- Protocol and persisted DTOs are mapped to validated domain values at their boundaries.
- Analysis accepts immutable content/snapshot inputs and does no direct filesystem or database I/O.
- Application ports are introduced only at ownership, I/O, security, or demonstrated multi-adapter boundaries. Do not create one generic repository trait or mock every internal function.
- The CLI is the composition root. The MCP adapter calls the same application use cases as CLI commands.
- Add a language package, FFI package, server process, or extension SDK only when an actual dependency, safety, ownership, distribution, or scaling requirement justifies it.
- Initial packages remain private workspace packages unless a supported external API is deliberately designed.

## Alternatives considered

### One crate with modules

It minimizes bootstrap work, but compiler-enforced dependency direction disappears and infrastructure types can leak into durable domain APIs. It is acceptable only as a short-lived spike scaffold.

### Crate per feature or language immediately

It creates many manifests, feature matrices, and public-looking boundaries before ownership and reuse are known. Languages begin as modules and split when a real boundary appears.

### Microservices

Independent scaling is not a Phase 0 requirement. Services would add remote consistency, authentication, deployment, telemetry, and failure modes to a local-first product.

### Plugin/microkernel architecture

A stable plugin contract would freeze immature evidence, snapshot, and lifecycle semantics. Declarative packs and interchange formats cover the first extension needs more safely.

## Consequences

### Positive

- Cargo enforces the important dependency direction.
- Domain and analysis tests can run without MCP, Git, or a database service.
- CLI and MCP share behavior rather than reimplementing it.
- A future PostgreSQL/server composition can reuse application/domain semantics without pretending local and server operations are identical.
- The process remains easy to install, debug, profile, and recover.

### Negative and risks

- Six packages add some manifest and compile-graph overhead.
- `repowitness-application` and `repowitness-local` can become dumping grounds without ownership checks.
- Excessive port traits can recreate a framework rather than a product.
- Moving types across package boundaries later may be disruptive if packages are published prematurely.

## Validation

- Keep the automated workspace dependency-policy check in required CI.
- Prove domain and analysis tests run without Tokio runtime, SQLite file, Git executable/repository, or MCP client.
- Prove CLI and MCP fixture calls produce the same domain result envelope.
- Review package dependency direction and public exports at every milestone.
- Track build time and split/merge packages only from measured evidence.

## Open questions

- Whether SQLite deserves a separate infrastructure package after Phase 1, and whether language packs should split from `repowitness-analysis` when dependency weight or distribution justifies it.
- Which application ports deserve behavior suites versus concrete integration tests.
- Whether any crate is useful and stable enough to publish independently.

## Implementation status

Implemented. The workspace contains the six accepted packages, the root
manifest expresses the inward dependency graph, and
`scripts/check-workspace-deps` rejects missing, unexpected, or disallowed
first-party edges. Domain and analysis remain free of transport and storage
dependencies; CLI and MCP retrieval share application use cases.

## Supersession

None.
