# Contributing to RepoWitness

RepoWitness accepts public contributions under the project [MIT License](LICENSE).

## Contribution license

Unless you state otherwise before submission, a contribution that you submit to
RepoWitness uses the MIT License. By submitting it, you confirm that you have
the right to do this.

Third-party dependencies, tools, grammars, generated artifacts, and other
material keep their own licenses and notices. The RepoWitness license does not
change those licenses.

## Clean-room and provenance policy

Contributions must be original work or material that the contributor can submit.

- Do not copy or port upstream source code, tests, fixtures, generated code, or
  large documents into RepoWitness because they are public or use a permissive
  license.
- A maintainer must approve reuse before you add it. Record its source, exact
  version, license compatibility, notices, transformation, and the reason that
  an independent implementation is not enough.
- You can do independent research, read protocols and specifications, compare
  behavior, and do black-box differential tests. Record material sources and
  describe behavior. Do not copy protected expression.
- Preserve all required copyright, attribution, and license notices for approved third-party material.
- Do not submit secrets, credentials, personal data, proprietary code, or material governed by incompatible terms.
- Contributors must review generated or assisted work for correctness,
  provenance, and license compliance.

If provenance is uncertain, stop. Open an issue or a design discussion before
you submit the material.

## Engineering expectations

Follow [AGENTS.md](AGENTS.md), the [engineering standard](docs/engineering.md),
and accepted [architecture decisions](docs/adr/README.md). Run the narrowest
relevant validation commands. A change is not complete until its behavior,
limits, evidence, tests, and documentation agree.

The current Rust workspace provides GNU Make wrappers:

```text
make help
make ci
make test-all
```

`make ci` is the required local baseline. `make test-all` adds the
no-default-feature and release profiles. `make test-sqlite-benchmarks` runs
manual SQLite timing and resource probes. These results are not release
budgets.

Pull requests run equivalent checks in the GitHub Actions `ci` job: the
non-test baseline followed by every supported test profile exactly once. The
workflow has read-only repository permissions, pins external actions by full
commit, caches only Cargo dependencies and the pinned dependency-policy tool,
and checks the complete pull-request diff. The `main` branch requires this job.
