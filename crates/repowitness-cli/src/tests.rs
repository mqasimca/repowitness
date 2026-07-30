use std::cell::{Cell, RefCell};
use std::io;
use std::path::PathBuf;

use super::*;

mod bounded_file;
mod configuration;
mod context;
mod core_index_search;
mod diagnostics;
mod doctor;
mod gc;
mod graph;
mod identity;
mod mcp;
mod memory;
mod memory_manage;
mod runtime_configuration;
mod symbol_inspect_io;
mod watch;

struct FakeInspector {
    outcome: FakeOutcome,
    calls: Cell<u64>,
    root: RefCell<Option<PathBuf>>,
}

#[derive(Clone, Copy)]
enum FakeOutcome {
    Success(GitPathDiscoveryStats),
    Failure(&'static str),
}

struct FakeIndexer {
    outcome: FakeIndexOutcome,
    calls: Cell<u64>,
    repository_root: RefCell<Option<PathBuf>>,
    database: RefCell<Option<PathBuf>>,
    repository_identity: RefCell<Option<OsString>>,
    configuration: RefCell<Option<ResolvedConfiguration>>,
}

#[derive(Clone, Copy)]
enum FakeIndexOutcome {
    Success(CliIndexReport),
    Failure(&'static str),
}

struct FakeSearcher {
    outcome: RefCell<Option<Result<CliSearchReport, &'static str>>>,
    calls: Cell<u64>,
    database: RefCell<Option<PathBuf>>,
    repository_identity: RefCell<Option<OsString>>,
    query: RefCell<Option<OsString>>,
    max_results: Cell<Option<u16>>,
    configuration: RefCell<Option<ResolvedConfiguration>>,
}

struct FakeSymbolGetter {
    outcome: RefCell<Option<Result<CliSymbolReport, &'static str>>>,
    calls: Cell<u64>,
    root: RefCell<Option<PathBuf>>,
    database: RefCell<Option<PathBuf>>,
    repository_identity: RefCell<Option<OsString>>,
    snapshot: RefCell<Option<OsString>>,
    path: RefCell<Option<OsString>>,
    content: RefCell<Option<OsString>>,
    artifact: RefCell<Option<OsString>>,
    generation: Cell<Option<i64>>,
    fact_ordinal: Cell<Option<u64>>,
}

struct FakeMemory;

impl RepositoryMemory for FakeMemory {
    fn revalidate(
        &self,
        _invocation: &MemoryRevalidationInvocation,
    ) -> Result<CliMemoryRevalidationReport, CliMemoryError> {
        Err(CliMemoryError::Failed)
    }

    fn recall(
        &self,
        _invocation: &MemoryRecallInvocation,
        _configuration: &ResolvedConfiguration,
    ) -> Result<MemoryRecallOutput, CliMemoryError> {
        Err(CliMemoryError::Failed)
    }
}

impl FakeInspector {
    fn success(stats: GitPathDiscoveryStats) -> Self {
        Self {
            outcome: FakeOutcome::Success(stats),
            calls: Cell::new(0),
            root: RefCell::new(None),
        }
    }

    fn failure(error: &'static str) -> Self {
        Self {
            outcome: FakeOutcome::Failure(error),
            calls: Cell::new(0),
            root: RefCell::new(None),
        }
    }
}

impl RepositoryPathInspector for FakeInspector {
    fn inspect(&self, root: &Path) -> Result<GitPathDiscoveryStats, String> {
        self.calls.set(self.calls.get() + 1);
        self.root.replace(Some(root.to_owned()));
        match self.outcome {
            FakeOutcome::Success(stats) => Ok(stats),
            FakeOutcome::Failure(error) => Err(error.to_owned()),
        }
    }
}

impl FakeIndexer {
    fn success(report: CliIndexReport) -> Self {
        Self {
            outcome: FakeIndexOutcome::Success(report),
            calls: Cell::new(0),
            repository_root: RefCell::new(None),
            database: RefCell::new(None),
            repository_identity: RefCell::new(None),
            configuration: RefCell::new(None),
        }
    }

    fn failure(error: &'static str) -> Self {
        Self {
            outcome: FakeIndexOutcome::Failure(error),
            calls: Cell::new(0),
            repository_root: RefCell::new(None),
            database: RefCell::new(None),
            repository_identity: RefCell::new(None),
            configuration: RefCell::new(None),
        }
    }
}

impl RepositoryIndexer for FakeIndexer {
    fn index(
        &self,
        invocation: &IndexInvocation,
        configuration: &ResolvedConfiguration,
    ) -> Result<CliIndexReport, String> {
        self.calls.set(self.calls.get() + 1);
        self.repository_root
            .replace(Some(invocation.repository_root.clone()));
        self.database.replace(Some(invocation.database.clone()));
        self.repository_identity
            .replace(Some(invocation.repository_identity.clone()));
        self.configuration.replace(Some(configuration.clone()));
        match self.outcome {
            FakeIndexOutcome::Success(report) => Ok(report),
            FakeIndexOutcome::Failure(error) => Err(error.to_owned()),
        }
    }
}

impl FakeSearcher {
    fn success(report: CliSearchReport) -> Self {
        Self {
            outcome: RefCell::new(Some(Ok(report))),
            calls: Cell::new(0),
            database: RefCell::new(None),
            repository_identity: RefCell::new(None),
            query: RefCell::new(None),
            max_results: Cell::new(None),
            configuration: RefCell::new(None),
        }
    }

    fn failure(error: &'static str) -> Self {
        Self {
            outcome: RefCell::new(Some(Err(error))),
            calls: Cell::new(0),
            database: RefCell::new(None),
            repository_identity: RefCell::new(None),
            query: RefCell::new(None),
            max_results: Cell::new(None),
            configuration: RefCell::new(None),
        }
    }
}

impl RepositorySearcher for FakeSearcher {
    fn search(
        &self,
        invocation: &SearchInvocation,
        configuration: &ResolvedConfiguration,
    ) -> Result<CliSearchReport, String> {
        self.calls.set(self.calls.get() + 1);
        self.database.replace(Some(invocation.database.clone()));
        self.repository_identity
            .replace(Some(invocation.repository_identity.clone()));
        self.query.replace(Some(invocation.query.clone()));
        self.max_results.set(Some(invocation.max_results));
        self.configuration.replace(Some(configuration.clone()));
        self.outcome
            .borrow_mut()
            .take()
            .expect("fake searcher should be called at most once")
            .map_err(str::to_owned)
    }
}

impl FakeSymbolGetter {
    fn success(report: CliSymbolReport) -> Self {
        Self {
            outcome: RefCell::new(Some(Ok(report))),
            calls: Cell::new(0),
            root: RefCell::new(None),
            database: RefCell::new(None),
            repository_identity: RefCell::new(None),
            snapshot: RefCell::new(None),
            path: RefCell::new(None),
            content: RefCell::new(None),
            artifact: RefCell::new(None),
            generation: Cell::new(None),
            fact_ordinal: Cell::new(None),
        }
    }

    fn failure(error: &'static str) -> Self {
        Self {
            outcome: RefCell::new(Some(Err(error))),
            calls: Cell::new(0),
            root: RefCell::new(None),
            database: RefCell::new(None),
            repository_identity: RefCell::new(None),
            snapshot: RefCell::new(None),
            path: RefCell::new(None),
            content: RefCell::new(None),
            artifact: RefCell::new(None),
            generation: Cell::new(None),
            fact_ordinal: Cell::new(None),
        }
    }
}

impl RepositorySymbolGetter for FakeSymbolGetter {
    fn get(&self, invocation: &SymbolInvocation) -> Result<CliSymbolReport, String> {
        self.calls.set(self.calls.get() + 1);
        self.root.replace(Some(invocation.root.clone()));
        self.database.replace(Some(invocation.database.clone()));
        self.repository_identity
            .replace(Some(invocation.repository_identity.clone()));
        self.snapshot.replace(Some(invocation.snapshot.clone()));
        self.path.replace(Some(invocation.path.clone()));
        self.content.replace(Some(invocation.content.clone()));
        self.artifact.replace(Some(invocation.artifact.clone()));
        self.generation.set(Some(invocation.generation));
        self.fact_ordinal.set(Some(invocation.fact_ordinal));
        self.outcome
            .borrow_mut()
            .take()
            .expect("fake symbol getter should be called at most once")
            .map_err(str::to_owned)
    }
}

fn index_report() -> CliIndexReport {
    CliIndexReport {
        generation: 3,
        source_epoch: 0,
        recovered_generations: 1,
        discovered_paths: 8,
        indexed_rust_files: 2,
        indexed_go_files: 1,
        indexed_typescript_files: 1,
        indexed_tsx_files: 1,
        indexed_python_files: 1,
        skipped_policy_paths: 0,
        skipped_unsupported_paths: 2,
        total_source_bytes: 101,
        total_facts: 7,
        syntax_error_nodes: 3,
        known_parser_limitation_nodes: 2,
        reused_rust_files: 1,
        analyzed_rust_files: 1,
        reused_go_files: 0,
        analyzed_go_files: 1,
        reused_typescript_files: 1,
        analyzed_typescript_files: 0,
        reused_tsx_files: 0,
        analyzed_tsx_files: 1,
        reused_python_files: 1,
        analyzed_python_files: 0,
    }
}

fn search_report() -> CliSearchReport {
    CliSearchReport {
        generation: 9,
        snapshot: "11".repeat(32),
        resolution: "confirmed",
        query_digest: "22".repeat(32),
        returned_matches: 1,
        total_matches: 3,
        searched: 8,
        skipped: 2,
        unresolved: 1,
        truncated: 2,
        matches: vec![CliSearchMatch {
            path: "rwp1:h:7372632F6C69622E7273".to_owned(),
            fact_ordinal: 7,
            content_digest: "33".repeat(32),
            artifact_digest: "44".repeat(32),
            producer_manifest: "55".repeat(32),
            language: "rust",
            kind: "function",
            name: "run".to_owned(),
            qualified_name: "fixture::run".to_owned(),
            name_start: 7,
            name_end: 10,
            declaration_start: 0,
            declaration_end: 13,
        }],
    }
}

fn symbol_report() -> CliSymbolReport {
    CliSymbolReport {
        generation: 9,
        snapshot: "11".repeat(32),
        resolution: "confirmed",
        path: "rwp1:h:7372632F6C69622E7273".to_owned(),
        content_digest: "33".repeat(32),
        artifact_digest: "44".repeat(32),
        fact_ordinal: 7,
        searched: 8,
        skipped: 2,
        unresolved: 1,
        truncated: 0,
        symbol: Some(CliSymbolData {
            producer_manifest: "55".repeat(32),
            language: "rust",
            kind: "function",
            name: "run".to_owned(),
            qualified_name: "fixture::run".to_owned(),
            name_start: 7,
            name_end: 10,
            declaration_start: 0,
            declaration_end: 13,
            declaration_encoding: "utf8",
            declaration: "pub fn run() {}".to_owned(),
        }),
    }
}

fn invoke(arguments: &[&str], inspector: &impl RepositoryPathInspector) -> (u8, String, String) {
    invoke_with_adapters(
        arguments,
        inspector,
        &FakeIndexer::failure("must not be called"),
        &FakeSearcher::failure("must not be called"),
    )
}

fn invoke_with_adapters(
    arguments: &[&str],
    inspector: &impl RepositoryPathInspector,
    indexer: &impl RepositoryIndexer,
    searcher: &impl RepositorySearcher,
) -> (u8, String, String) {
    invoke_with_symbol_adapter(
        arguments,
        inspector,
        indexer,
        searcher,
        &FakeSymbolGetter::failure("must not be called"),
    )
}

fn invoke_with_symbol_adapter(
    arguments: &[&str],
    inspector: &impl RepositoryPathInspector,
    indexer: &impl RepositoryIndexer,
    searcher: &impl RepositorySearcher,
    symbol_getter: &impl RepositorySymbolGetter,
) -> (u8, String, String) {
    let args =
        std::iter::once(OsString::from("repowitness")).chain(arguments.iter().map(OsString::from));
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run_with_adapters(
        args,
        &mut stdout,
        &mut stderr,
        inspector,
        indexer,
        searcher,
        symbol_getter,
        &FakeMemory,
        &LocalConfigurationLoader,
    );
    (
        code,
        String::from_utf8(stdout).expect("test stdout is UTF-8"),
        String::from_utf8(stderr).expect("test stderr is UTF-8"),
    )
}
