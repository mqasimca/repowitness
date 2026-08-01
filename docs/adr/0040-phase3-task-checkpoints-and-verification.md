# ADR-0040: Keep task checkpoints application-owned and project them through MCP Tasks

- Status: Accepted
- Date: 2026-07-31
- Owners: Project maintainers
- Scope: Durable work state, verification evidence, polling, and MCP task negotiation

## Context

Coding work needs resumable objectives, attempts, diagnostics, and verification
evidence. Raw conversations and terminal output are unsafe as durable state:
they can contain secrets, personal data, arbitrary untrusted text, and
unbounded payloads. The current MCP Tasks extension provides durable handles
only when both peers negotiate support, while many local stdio clients do not
support it.

## Decision

Task semantics are application-owned, bounded, and persisted in the local
SQLite store. A task has an opaque random ID, one repository identity and scope,
an objective, acceptance criteria, state, recorded timestamps, and append-only
checkpoint, attempt, and verification events. Checkpoints store only validated,
redacted structured summaries: active hypothesis, bounded source selectors,
attempt outcome, diagnostic classification/digest, unresolved questions, and a
next safe action. They do not store raw command output, source content,
credentials, environment variables, or conversation transcripts.

Verification is a distinct immutable receipt with an exact source target,
bounded command or check identity, exit/result category, tool producer, and
captured-output digest/byte count. It may prove an attempt or procedure only
when the check completed successfully at the cited target. A cancelled,
failed, missing, stale, or partial verification does not promote a procedure
and is explicitly retrievable as such.

The CLI and ordinary MCP tools expose synchronous create/checkpoint/attempt,
verification, and bounded polling reads. This is the mandatory fallback.
When a client explicitly negotiates MCP Tasks and invokes a task-capable
operation with task metadata, the local MCP server may return a durable task
handle and implements `tasks/get`, `tasks/list`, and `tasks/cancel` over the
same application task state. It never returns a task handle to a client that
did not opt in, and cancellation remains cooperative. Task TTL, poll interval,
result size, operation concurrency, deadlines, and durable-state cleanup are
fixed server policy, not caller-controlled values.

MCP task handles are transport projections, not engineering-work identities:
a reconnect, protocol downgrade, or expired MCP handle cannot delete or alter a
stored engineering task. Conversely, an engineering checkpoint does not claim
that a connected MCP client supports the extension.

## Alternatives considered

### Persist raw terminal sessions and chats

This has poor secret boundaries, unbounded storage, and weak semantic
verification.

### Require MCP Tasks for all asynchronous work

Client support is optional and version-dependent; it would make core local
work-state unavailable to ordinary CLI and stdio users.

### Let a completed command automatically verify a procedure

Exit status alone lacks source target, producer, scope, and retention evidence.

## Consequences

### Positive

- Work can resume safely without treating conversation text as truth.
- Verified procedures have evidence rather than a self-asserted success flag.
- MCP-capable clients get durable handles while all clients retain polling.

### Negative and risks

- Structured checkpoint entry is more deliberate than copying a terminal log.
- Durable task state requires bounded retention and schema migration coverage.
- MCP Tasks is an evolving extension, so its wire projection remains optional.

## Validation

- Task/attempt/verification immutability, bounds, cancellation, restart, and
  cleanup fixtures.
- Secret/poisoning/redaction tests for every persisted text field and result.
- Procedure-promotion tests for success, failure, cancellation, source change,
  and unavailable evidence.
- MCP negotiated and non-negotiated task, polling fallback, reconnect, cancel,
  TTL, and output-limit contract tests.

## Follow-up

- Add migration, application ports, local store, CLI, and MCP projections.
- Include task and verification state in the Phase 3 longitudinal evaluation.

## Supersession

None.
