# ADR-0053: Produce and import one Go SCIP overlay through explicit local execution

- Status: Proposed
- Date: 2026-08-03
- Owners: Project maintainers
- Scope: direct CLI, local Go SCIP production, and existing SCIP-overlay import

## Context

The current source index supports Go syntax facts, but exact Go symbol
resolution requires an admitted SCIP precision overlay. The normal source index
must not execute a compiler, package manager, or external producer because that
would change its predictable hostile-workspace boundary.

Upstream `scip-go`, checked on 2026-08-03, is the maintained Go SCIP producer.
Its documented standard flow runs at a `go.mod` project root and emits an
`index.scip` artifact. It invokes the Go toolchain and supports custom package
drivers, both of which must be controlled by RepoWitness's explicit local
execution boundary. Multi-module repositories require one producer run per
module upstream; the single source-slot overlay contract does not yet compose
those module-relative artifacts safely.

`scip-go` 0.2.7 declares itself in SCIP metadata but omits the required
per-document position-encoding field. Its Go token ranges are UTF-8 byte
offsets, as required for Go producers by the SCIP schema. Treating omission as
UTF-8 based only on a document's untrusted language label would weaken generic
SCIP admission.

## Decision

Add the direct CLI-only `scip-go-import` command.

- The user explicitly supplies the database and repository root. They select
  either an ordinary indexed repository identity or an explicit connected
  workspace and source slot, and may select an exact workspace view.
- It invokes only `scip-go index --output <private-temporary-file>` with the
  supplied root as its working directory. The executable defaults to `scip-go`
  and can be selected explicitly. Full implementation and test relationships
  are included by default; `--skip-implementations` and `--skip-tests` are
  explicit opt-outs for a smaller/faster producer run.
- It is initially limited to standard, single-module projects whose `go.mod`
  is at the supplied root. Rootless and nested-module-only worktrees are an
  explicit unavailable compatibility case, not a partial-overlay claim.
- It forces `GOENV=off`, `GOPACKAGESDRIVER=off`, `GOPROXY=off`, `GOSUMDB=off`,
  `GOTOOLCHAIN=local`, `GOWORK=off`, and `GOFLAGS=-mod=readonly`. The command
  neither downloads a producer nor fetches dependencies; required module data
  must already be locally available.
- Producer and import deadlines are independently bounded. Output and
  diagnostics are not forwarded, and the temporary artifact is removed on a
  best-effort basis after the command returns.
- The generated file is admitted only through the existing no-follow,
  source-fenced, immutable-view `scip-import` path. Failed production, a bad
  artifact, changed source, cancellation, or failed import leaves the prior
  overlay readable.
- An omitted document position encoding is admitted as UTF-8 only when the
  enclosing SCIP metadata identifies the producer as `scip-go` and that
  document identifies itself as Go. Omission remains invalid for every other
  producer and for standalone document decoding; importer identity advances
  with this semantics change.
- `scip-go` can also emit dependency documents whose paths begin with `../`.
  When, and only when, metadata identifies `scip-go`, those parent-relative
  documents are excluded before source admission: they cannot belong to the
  selected source root. Absolute paths, interior parent traversals, and every
  malformed or source-mismatched in-root document still fail atomically. The
  activated-import receipt reports the excluded document count.
- The shared hostile-artifact decoder admits at most 256 MiB total and 2 MiB
  per SCIP document. The latter bound covers observed `scip-go` source files
  up to 1.55 MiB while retaining a hard cap for every retained raw payload.
- Normal `index` and `watch` never run a producer. Onboarding may run this
  producer after its source generation completes; `--no-scip` disables that
  enrichment. The read-only MCP server never exposes this operation. No general producer registry,
  package-manager execution, downloaded tool, background process, or another
  language adapter is introduced.

## Alternatives considered

### Automatically run scip-go during every index

Rejected. It would make ordinary source indexing dependent on Go build and
module state, and broaden routine execution over untrusted worktrees.

### Allow arbitrary Go package drivers or network access

Rejected. A package driver is another executable selected outside the explicit
producer contract, while network access makes an overlay dependent on mutable
remote state. Users can prepare their local Go dependencies before opting in.

### Compose all nested Go modules immediately

Rejected for now. The upstream producer's documents are module-root-relative,
while RepoWitness admits one source-slot overlay against repository-root-relative
source receipts. Combining them requires a separately validated path and symbol
identity contract, not byte concatenation or path rewriting by assumption.

### Introduce a generic compiler or SCIP-producer plugin framework

Rejected. It would broaden the execution surface and policy model before more
than two named, bounded producer contracts are demonstrated.

## Consequences

### Positive

- Standard Go-module users can obtain exact SCIP symbol and occurrence evidence
  without leaving a persistent artifact in their repository.
- Normal source indexing and MCP remain local, read-only, predictable, and free
  of producer execution.
- The existing overlay source fences and categorical result limits continue to
  distinguish precise producer evidence from syntax-only facts.

### Negative and risks

- The command runs an already installed external tool and Go toolchain against
  a user-selected project; it can fail when dependencies are not locally cached.
- The initial root-module boundary excludes nested-module-only and rootless Go
  worktrees from producer import.
- `scip-go` compatibility must be revalidated when Go or the producer changes.

## Validation

- A black-box synthetic `scip-go` checks every fenced environment setting,
  emits a valid SCIP artifact, and proves exact overlay evidence is available.
- Wire-level tests prove that the `scip-go` metadata exception cannot be
  activated by a Go document label alone.
- Mixed-document tests prove that only `scip-go` parent-relative dependency
  documents are excluded and the receipt retains that coverage fact.
- The SCIP wire suite exercises the inclusive 2 MiB per-document boundary and
  rejects one byte over it before invoking a consumer.
- Parser tests cover deterministic single-repository selectors and bounded
  default deadlines.
- The private sibling-worktree smoke script has an opt-in `--go-scip` mode that
  tests every eligible sibling while disclosing only aggregate outcomes.
- Existing contained-import fixtures cover no-follow reads, stale source,
  invalid artifacts, atomic publication, and previous-overlay preservation.

## Follow-up

- Measure the rootless and multi-module Go worktree cases before proposing a
  validated multi-module composition contract.
- Revisit Go producer environment policy only with explicit evidence that a
  controlled build-driver or dependency flow is needed.

## Supersession

None. This composes with ADR-0035, ADR-0037, ADR-0045, ADR-0048, and the
parallel explicit Rust producer boundary in ADR-0052.
