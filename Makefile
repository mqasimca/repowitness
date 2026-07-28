SHELL := /bin/sh

CARGO ?= cargo
CARGO_DENY_VERSION ?= 0.19.4

.DEFAULT_GOAL := help
.NOTPARALLEL:

.PHONY: \
	all \
	benchmarks \
	check \
	ci \
	clippy \
	deny \
	deps \
	diff-check \
	docs \
	fmt \
	fmt-check \
	fuzz-check \
	grammars \
	help \
	rustdoc \
	test \
	test-all \
	test-all-features \
	test-doc \
	test-no-default-features \
	test-release \
	test-sqlite \
	test-sqlite-benchmarks

help:
	@printf '%s\n' \
		'RepoWitness verification targets:' \
		'  make fmt                       Format the Rust workspace' \
		'  make fmt-check                 Check Rust formatting' \
		'  make fuzz-check                Check the standalone fuzz crate and lockfile' \
		'  make check                     Check all targets and features' \
		'  make clippy                    Run Clippy with warnings denied' \
		'  make test                      Run default workspace tests' \
		'  make test-all                  Run every supported test profile' \
		'  make test-sqlite               Run the SQLite durability spike' \
		'  make test-sqlite-benchmarks    Run manual SQLite probes in release mode' \
		'  make rustdoc                   Build warning-free API documentation' \
		'  make deny                      Check advisories, licenses, bans, and sources' \
		'  make docs                      Check Markdown files and local links' \
		'  make deps                      Check workspace dependency directions' \
		'  make grammars                  Check vendored grammar integrity and regeneration' \
		'  make benchmarks                Check benchmark manifests' \
		'  make diff-check                Check the Git diff for whitespace errors' \
		'  make ci                        Run the required pull-request checks' \
		'  make all                       Alias for make ci'

fmt:
	$(CARGO) fmt --all

fmt-check:
	$(CARGO) fmt --all -- --check

fuzz-check:
	$(CARGO) check --manifest-path fuzz/Cargo.toml \
		--all-targets --locked --target-dir target/fuzz-check

check:
	$(CARGO) check --workspace --all-targets --all-features --locked

clippy:
	$(CARGO) clippy --workspace --all-targets --all-features --locked -- -D warnings

test:
	$(CARGO) test --workspace --locked

test-no-default-features:
	$(CARGO) test --workspace --no-default-features --locked

test-all-features:
	$(CARGO) test --workspace --all-features --locked

test-doc:
	$(CARGO) test --workspace --doc --all-features --locked

test-release:
	$(CARGO) test --workspace --release --all-features --locked

test-all: test test-no-default-features test-all-features test-doc test-release

test-sqlite:
	$(CARGO) test -p repowitness-local --test sqlite_generation_spike --locked

test-sqlite-benchmarks:
	$(CARGO) test -p repowitness-local --test sqlite_generation_spike \
		--release --locked -- --ignored --nocapture

rustdoc:
	RUSTDOCFLAGS="-D warnings" $(CARGO) doc \
		--workspace --all-features --no-deps --locked

deny:
	@$(CARGO) deny --version >/dev/null 2>&1 || { \
		printf '%s\n' \
			'cargo-deny is required; run: cargo install cargo-deny --version $(CARGO_DENY_VERSION) --locked' >&2; \
		exit 127; \
	}
	$(CARGO) deny --locked check
	$(CARGO) deny --manifest-path fuzz/Cargo.toml --locked check

docs:
	./scripts/check-docs

deps:
	./scripts/check-workspace-deps

grammars:
	./scripts/check-vendored-grammars
	./scripts/check-vendored-grammar-regeneration

benchmarks:
	./scripts/check-benchmarks
	./scripts/validate-codex-evaluation --self-test

diff-check:
	git diff --check

ci: fmt-check fuzz-check check clippy test test-all-features test-doc rustdoc deny deps grammars benchmarks docs diff-check

all: ci
