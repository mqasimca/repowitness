# Phase 1 portable-core validation

- Status: Completed CI evidence
- Observed: 2026-07-30 UTC
- RepoWitness revision: `006197af6bc2a43d77cfa94c0b599b2e28d67704`
- Workflow: [CI run 30583304375](https://github.com/mqasimca/repowitness/actions/runs/30583304375)

## Results

The `portable core` CI matrix completed successfully for the exact revision on:

| Platform | Job | Checks |
|---|---|---|
| macOS 15 | [job 91008638547](https://github.com/mqasimca/repowitness/actions/runs/30583304375/job/91008638547) | formatting, all targets/features check and Clippy, full workspace all-feature tests, doctests, and rustdoc |
| Windows 2025 | [job 91008638576](https://github.com/mqasimca/repowitness/actions/runs/30583304375/job/91008638576) | formatting, all targets/features check and Clippy, full workspace all-feature tests, doctests, and rustdoc |

This supplies platform execution evidence for the Rust workspace’s currently
committed Phase 1 contracts, including the platform-conditional test coverage
selected by each host.

## Limits

The jobs predate the uncommitted Codex-envelope and adversarial-matrix scripts.
They do not run the Ubuntu-only resource benchmark, retain an attestation
artifact, or independently establish platform-specific resource budgets. A
clean commit and new CI results remain required before this evidence can cover
the current worktree.
