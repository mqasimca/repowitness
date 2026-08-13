# ADR-0064: Auto-import Go SCIP during explicit onboarding

- Status: Proposed
- Date: 2026-08-12
- Owners: Project maintainers
- Scope: onboarding and the existing local Go SCIP producer/import boundary

## Context

Go source indexing provides bounded syntax facts, while exact caller, callee,
implementation, and test relationships come from the existing local
`scip-go-import` command. Requiring users to run a second command after every
`onboard` makes repository setup easy to get wrong.

## Decision

`onboard` runs the normal atomic source index first. When that generation
contains Go files, the repository has a regular root `go.mod`, and `scip-go`
is available, onboarding runs the existing bounded producer/import flow with
implementation and test relationships enabled.

- `--no-scip` keeps onboarding source-only.
- `--scip-go <path>` selects a producer other than the default `scip-go`.
- A missing producer or unsupported root-module layout is reported as a
  categorical skip; producer/import failure is reported as a categorical
  failure. Either way, the completed source generation remains usable.
- Ordinary `index`, `watch`, and MCP startup remain producer-free.
- RepoWitness never downloads a producer or enables network/package-driver
  execution. Existing SCIP environment restrictions and deadlines apply.
- The current boundary is one regular `go.mod` at the repository root;
  nested-module-only repositories remain an explicit unsupported case.

## Alternatives considered

### Keep SCIP as a second mandatory command

Rejected. It makes successful onboarding incomplete from the user's point of
view and leaves exact Go relationships absent unless users know the follow-up
command.

### Run SCIP during every index or MCP startup

Rejected. It would make routine indexing and server startup depend on the Go
toolchain and module state, increasing latency and broadening execution over
hostile worktrees.

### Download or auto-install `scip-go`

Rejected. Installation and network policy belong to the user environment, not
to an indexing command.

## Consequences

Onboarding a supported Go module becomes a complete source-plus-relationship
setup with one command. Non-Go repositories and environments without a local
producer remain fast and explicit through the report. Go onboarding can take
up to the existing bounded producer and import deadlines.

## Validation

- CLI and parser tests cover the new opt-out and producer override.
- Existing source-generation and import atomicity tests remain applicable.
- A live onboarding smoke test uses an installed `scip-go` on a temporary
  private state directory and verifies an imported overlay; the public test
  record contains only aggregate pass/fail coverage.

## Supersession

This clarifies the onboarding exception to ADR-0053; it does not change the
explicit producer boundary for ordinary indexing, watch mode, or MCP startup.
