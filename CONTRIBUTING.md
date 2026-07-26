# Contributing to RepoWitness

RepoWitness welcomes public contributions under the project [MIT License](LICENSE).

## Contribution license

Unless you explicitly state otherwise before submission, any contribution you intentionally submit for inclusion in RepoWitness is licensed under the MIT License. By submitting it, you represent that you have the right to do so.

Third-party dependencies, tools, grammars, generated artifacts, and other incorporated material keep their own licenses and notices. The RepoWitness license does not relicense them.

## Clean-room and provenance policy

Contributions must be original work or material the contributor is authorized to submit.

- Do not copy or port upstream source code, tests, fixtures, generated code, or substantial documentation into RepoWitness merely because it is publicly visible or permissively licensed.
- A maintainer must explicitly approve any proposed reuse before it is added. The change must record its source, exact version, license compatibility, notices, transformation, and why independent implementation is insufficient.
- Independent research, protocol/specification reading, behavioral comparison, and black-box differential testing are allowed. Record material sources and describe observed behavior without copying protected expression.
- Preserve all required copyright, attribution, and license notices for approved third-party material.
- Do not submit secrets, credentials, personal data, proprietary code, or material governed by incompatible terms.
- Contributors remain responsible for reviewing generated or assisted work for correctness, provenance, and license compliance.

When provenance is uncertain, stop and open an issue or design discussion before submitting the material.

## Engineering expectations

Follow [AGENTS.md](AGENTS.md), the [engineering standard](docs/engineering.md), accepted [architecture decisions](docs/adr/README.md), and the narrowest relevant validation commands. A change is not complete until its behavior, limitations, evidence, tests, and documentation agree.

The current Rust workspace provides GNU Make wrappers:

```text
make help
make ci
make test-all
```

`make ci` is the required local baseline. `make test-all` adds the supported
no-default-feature and release profiles. SQLite timing/resource probes are
manual and opt-in through `make test-sqlite-benchmarks`; their results are not
release budgets.
