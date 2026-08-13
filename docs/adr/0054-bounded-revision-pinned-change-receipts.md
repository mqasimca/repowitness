# ADR-0054: Produce bounded revision-pinned change receipts for review

- Status: Proposed
- Date: 2026-08-08
- Owners: Project maintainers
- Scope: Local Git comparison, change-receipt application use case, CLI, and local stdio MCP

## Context

Codex can retrieve exact source, current memory, test markers, topology, and
bounded graph evidence, but it has no one receipt that binds those materials to
an explicit proposed change. Concatenating `git diff` and independent read
commands would not provide one deadline, one coverage contract, or protection
against a changing worktree.

The roadmap proposes agent preflight receipts and PR-review packets. They must
help a reviewer navigate material constraints without presenting an opaque
correctness score, fabricating test execution, or adding a hosted pull-request
authority.

## Decision

Add one read-only, versioned `verify` application use case. It accepts one
bounded declared intent, one explicit full base Git object ID, and one admitted
current worktree. The local adapter derives the comparison; callers cannot
submit an unverified patch as evidence.

The V1 receipt binds:

1. the exact resolved base object and canonical bounded path-change manifest;
2. one opaque current-worktree Git-state digest captured both before and after
   the comparison and context attempt; and
3. either a separately pinned Phase 0 indexed context pack or the categorical
   `stale_source` absence of that pack; and
4. a categorical `verified`, `mismatch`, or `unavailable` comparison between
   the active indexed source identity and the fenced worktree.

The local adapter performs a final source fence after collecting the comparison
and any generation-pinned context. A changing worktree or comparison basis
fails closed rather than returning a mixed receipt. If an indexed declaration
selected for context no longer has matching on-disk bytes, V1 returns the
otherwise complete fenced manifest with context explicitly unavailable; it
never inserts stale declaration text or silently retargets a symbol.

The schema-2 receipt uses a full base object ID and current worktree comparison. It does not
claim branch ancestry, merge-base selection, hunk-level declaration impact,
semantic rename/correspondence, runtime behavior, test execution, review
approval, or merge eligibility. `verified` means the indexed source identity
and fenced source manifest match; `mismatch` means they do not; `unavailable`
means the comparison could not be completed. Renames are disabled in the
current diff profile, so
each path remains an independent change-manifest observation.

The receipt is deterministic and carries a digest suitable for an external
caller to retain. V1 does not persist PR metadata or a review verdict, does not
write to a forge, and does not add a hosted integration. CLI and MCP remain thin
read-only adapters over the same application use case.

## Alternatives considered

### Let Codex combine `git diff` and existing MCP calls

This requires no new code but cannot pin a common comparison basis, enforce a
single budget/deadline, or make missing evidence visible as one receipt.

### Accept caller-supplied patch text

Patch text can be incomplete, spoofed, unbounded, and detached from the
admitted worktree. The local adapter must derive and verify comparison facts.

### Return an approval or risk score

Such a verdict would hide the evidence and require a separately governed
policy, false-positive budget, and merge authority. V1 remains advisory.

### Persist every receipt in SQLite

Durable review records introduce retention, privacy, migration, and audit
policy. A deterministic caller-retained receipt is sufficient for the initial
evaluation.

## Consequences

### Positive

- Codex receives one evidence-bearing review packet for an exact change.
- Existing immutable generation and current-memory rules remain reusable.
- The first contract is local, bounded, and forge-independent.

### Negative and risks

- Git object comparison adds bounded subprocess and object-availability
  failure modes.
- A changed declaration can make otherwise relevant indexed context unavailable
  until reconciliation publishes a matching generation.
- V1 deliberately contains no graph, language-impact, or test-marker evidence;
  those require their own later review-packet contract.

## Validation

- Synthetic fixtures cover modified and stale-indexed declarations, canonical
  input rejection, bounded output, deterministic ordering, and empty/untracked
  path handling in the local manifest adapter.
- CLI and MCP contracts cover read-only exposure, the no-verdict boundary, and
  categorical `stale_source` context absence.
- A predeclared design-partner evaluation compares accepted findings, missed
  material constraints, reviewer time, and false-positive burden with ordinary
  source search.

## Follow-up

- Implement the shared V1 receipt before adding a hosted forge integration.
- Add candidate test markers, change-to-declaration attribution, merge-base
  comparison, and durable receipt retention only through separately reviewed
  contracts and evidence.

## Supersession

None.
