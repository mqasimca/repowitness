# ADR-0009: Use the MIT License and clean-room contribution rules

- Status: Accepted
- Date: 2026-07-23
- Owners: Project maintainers
- Scope: Original project content, incoming contributions, and third-party provenance

## Context

RepoWitness is intended to be developed publicly. Public visibility alone does not grant permission to use, modify, or redistribute the repository, and implementation cannot responsibly select dependencies or accept contributions without a project license and provenance policy.

The project also studies existing code-intelligence systems. Even when an upstream project uses a permissive license, copying its source, tests, fixtures, generated code, or substantial documentation would create attribution, compatibility, maintainability, and clean-room risks that independent behavioral research avoids.

## Decision

License original RepoWitness work under the MIT License:

- Keep the standard MIT text in the repository-root `LICENSE`.
- Use the SPDX identifier `MIT` in Cargo workspace/package metadata.
- Treat contributions intentionally submitted for inclusion as MIT-licensed unless the contributor explicitly states otherwise before submission.
- Require contributors to have the right to submit their work.

Apply the clean-room and provenance rules in [`CONTRIBUTING.md`](../../CONTRIBUTING.md):

- Contributions are original or explicitly authorized.
- Do not copy or port upstream source, tests, fixtures, generated code, or substantial documentation without prior maintainer approval and recorded provenance, version, license compatibility, notices, and rationale.
- Independent specification research, behavioral comparison, and black-box differential testing are allowed.
- Third-party components retain their own licenses and notices; RepoWitness does not relicense them.
- Generated or assisted contributions receive the same provenance and correctness review as handwritten work.

The license does not grant rights to third-party names, marks, or separately licensed material.

## Alternatives considered

### MIT OR Apache-2.0

This is common in the Rust ecosystem and gives recipients a choice that includes Apache-2.0's express patent terms. It was not selected because the maintainer chose the simpler single MIT license.

### Apache-2.0 only

Apache-2.0 includes an express patent grant and more detailed contribution and notice terms. Its additional conditions were not needed for the initial project policy.

### Copyleft license

A copyleft license could require some downstream modifications to remain available under the same terms. That obligation would reduce permissive adoption and is not the selected product strategy.

### No license

Public source without a license would remain copyright-restricted and would conflict with the goal of public collaboration.

## Consequences

### Positive

- The project is straightforward to use, modify, redistribute, and incorporate.
- Cargo and dependency-policy metadata use one standard SPDX identifier.
- Incoming contribution terms are clear and match the outbound project license.
- Clean-room rules reduce accidental copying and make approved reuse auditable.

### Negative and risks

- MIT does not include Apache-2.0's express patent grant or patent-termination language.
- Permissive recipients may distribute closed-source derivatives.
- Maintainers must review provenance and preserve third-party notices where reuse is approved.
- A contributor may lack rights despite making a representation; review and provenance records remain necessary.

## Validation

- Keep `LICENSE` byte-for-byte equivalent to the standard MIT terms except for the permitted copyright line.
- Link the license and contribution policy from the README and contributor documentation.
- Keep Cargo `license` metadata set to `MIT`.
- Keep the committed dependency license/source policy enabled for production
  dependencies.
- Require approved third-party reuse to carry its provenance and notices in the same change.

## Revisit conditions

Revisit through a superseding ADR if a project legal entity, patent policy, major distributor, contributor agreement, or ecosystem requirement makes the MIT-only choice insufficient. Existing releases and third-party contributions cannot be relicensed without the required rights.

## Implementation status

Implemented. The repository includes the MIT `LICENSE`, workspace/package
metadata inherits `MIT`, `CONTRIBUTING.md` records the clean-room and provenance
rules, and `cargo-deny` enforces the reviewed dependency license and source
policy.

## Supersession

None.
