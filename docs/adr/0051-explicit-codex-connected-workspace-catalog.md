# ADR-0051: Compose an explicit Codex connected-workspace catalog

- Status: Proposed
- Date: 2026-08-02
- Owners: Project maintainers
- Scope: Codex catalog admission, connected-workspace indexing, and MCP source routing

## Context

The proposed Codex catalog in ADR-0050 makes one global MCP connection usable
from any individual Git worktree. It deliberately indexes only the current
worktree. That is insufficient when a locally checked-out product has several
repositories whose source changes must be indexed and published together.

RepoWitness already has accepted connected-workspace contracts: ADR-0026
models an immutable view of explicit source slots, ADR-0031 requires atomic
publication across all of those slots, and ADR-0032 requires a caller-supplied
manifest. A product-stack convenience layer must compose those contracts. It
must not infer membership from sibling directories, package metadata, source
imports, or model requests; doing so would silently expand filesystem authority
and could mistake a co-located unrelated repository for a product dependency.

## Decision

Add explicit `repowitness codex workspace` lifecycle commands below the one
global Codex installation:

```text
repowitness codex workspace create --name <label> \
  --repository <absolute-path> --repository <absolute-path> [--repository <absolute-path> ...]
repowitness codex workspace list
repowitness codex workspace remove --name <label>
```

Creation accepts two through thirty-two explicitly supplied Git worktree
paths. It resolves each only to its containing worktree root, creates opaque
repository, source-slot, and connected-workspace identities, writes one exact
generated connected-workspace manifest in private Codex-owned state, and
performs the existing atomic connected-workspace index before publishing the
new catalog membership. The private catalog stores labels and the canonical
membership needed to recognize a future process current directory; it is not
an MCP response or a repository artifact.

At `mcp-serve --catalog` startup, RepoWitness resolves the current worktree as
in ADR-0050. If exactly one explicitly registered connected workspace contains
that root, it refreshes every declared source through the existing one-request
connected-workspace index and starts MCP only after an immutable view is
published. Each member is exposed as one pre-admitted opaque `repository_id`.
Omitting the selector still means the process-current member; choosing another
member requires its exact opaque ID. Source-view-aware discovery uses that
member's exact connected-workspace and source-slot receipt rather than a
repository-global active-generation fallback.

The initial source-view-aware Catalog surface is bounded to lexical code
search, relevant-path projection (including the finite code-graph envelope),
typed declaration search, architecture maps, Rust graph reads, SCIP
evidence/relationship reads, and Phase 2 context compilation. Each result
continues to report its normal concrete snapshot/generation evidence; tools
whose current contracts are repository-active-generation-only remain outside
the connected-workspace catalog selection until their receipt contracts are
extended.

Membership is not automatically inferred or updated. Adding, removing, or
replacing a repository requires removing and recreating the named workspace;
identities and prior immutable views remain retained under normal lifecycle
policy. Removing a name removes only its private catalog registration and
intentionally retains its index and manifest for recovery/retention. There is
no background watcher, daemon, root scan, remote catalog, MCP mutation tool,
or general cross-repository query.

RepoWitness does not claim an inferred relationship merely because two source
slots share a workspace. Cross-repository facts are available only when a
source-specific supported evidence producer (currently such as a SCIP overlay)
has emitted an attributed relationship; lexical and syntax results remain
source-scoped. This preserves the Phase 0 evidence boundary.

## Alternatives considered

### Automatically group sibling repositories

This would be convenient for a few layouts but turns ambient filesystem shape
into authorization and offers no trustworthy definition of a product stack.

### Have MCP callers register or select arbitrary roots

Tool input is model controlled. Allowing it to change workspace membership or
filesystem authority violates the read-only startup boundary.

### Add one permanent background service for all local repositories

That adds supervision, ownership, upgrade, cancellation, retention, and
privacy concerns before foreground incremental startup has been measured as
insufficient.

### Infer cross-repository links from names or imports

Those heuristics are useful candidates, not evidence. They would overstate
relationship certainty and conflict with the source-slot attribution contract.

## Consequences

### Positive

- One installed Codex MCP connection can serve an explicitly declared local
  product stack from any of its member worktrees.
- Every refresh uses existing atomic connected-workspace publication rather
  than independently updating repositories into a mixed state.
- Default selection remains ergonomic while cross-member access is explicit
  and host paths remain private.

### Negative and risks

- Initial creation and future session startup can be slower because all
  declared members refresh together.
- The operator must explicitly maintain product-stack membership.
- Cross-source semantic relationship coverage remains producer-dependent; a
  shared workspace never creates an implicit graph edge.
- Connected-workspace catalog onboarding is unavailable on platforms where
  private onboarding state fails closed; Windows requires an equivalent
  private-state ACL boundary before it can be supported.

## Validation

- Test create/list/remove output, private-state ownership, invalid membership,
  duplicate roots, and path-free output using synthetic worktrees.
- Run an installed-binary stdio fixture that creates a two-member workspace,
  proves atomic catalog startup, current-member default selection, explicit
  other-member selection, immutable source-slot receipts, and no raw-root
  disclosure.
- Test source-view-aware code search, relevant paths, and typed symbol search
  against the non-default member, including generation-view pinning.
- Run the private sibling-repository corpus only through the aggregate-only
  validation script; do not record its topology, source, or paths in public
  fixtures or documents.

## Follow-up

- Extend remaining repository-active-generation tools only with source-view
  receipt contracts and dedicated mixed-generation fixtures.
- Measure foreground multi-member refresh latency before considering a shared
  local coordinator.
- Revisit membership edits only with a compatible immutable-identity and
  retention design.

## Supersession

None. This composes ADR-0026, ADR-0031, ADR-0032, and ADR-0050 without
changing their accepted source, publication, or authority contracts.
