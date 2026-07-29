use std::cell::{Cell, RefCell};

use repowitness_local::{ConfigurationPolicyOverrides, ConfigurationPreferenceOverrides};

use super::*;

struct RecordingConfigurationLoader {
    calls: Cell<u64>,
    invocation: RefCell<Option<ConfigurationInvocation>>,
    outcome: Result<ResolvedConfiguration, ConfigurationLoadError>,
}

struct ConfigurationRecordingContextBuilder {
    calls: Cell<u64>,
    query_results: Cell<Option<u64>>,
}

impl RepositoryContextBuilder for ConfigurationRecordingContextBuilder {
    fn build(
        &self,
        _invocation: &ContextInvocation,
        configuration: &ResolvedConfiguration,
    ) -> Result<ContextBuildOutput, String> {
        self.calls.set(self.calls.get() + 1);
        self.query_results.set(Some(
            *configuration.preferences().query_results().effective(),
        ));
        Err("intentional test stop".to_owned())
    }
}

struct ConfigurationRecordingMemory {
    calls: Cell<u64>,
    query_results: Cell<Option<u64>>,
}

impl RepositoryMemory for ConfigurationRecordingMemory {
    fn revalidate(
        &self,
        _invocation: &MemoryRevalidationInvocation,
    ) -> Result<CliMemoryRevalidationReport, String> {
        Err("must not be called".to_owned())
    }

    fn recall(
        &self,
        _invocation: &MemoryRecallInvocation,
        configuration: &ResolvedConfiguration,
    ) -> Result<MemoryRecallOutput, String> {
        self.calls.set(self.calls.get() + 1);
        self.query_results.set(Some(
            *configuration.preferences().query_results().effective(),
        ));
        Err("intentional test stop".to_owned())
    }
}

struct ConfigurationRecordingDiagnosticsReader {
    calls: Cell<u64>,
    digest: Cell<Option<[u8; 32]>>,
}

impl RepositoryDiagnosticsReader for ConfigurationRecordingDiagnosticsReader {
    fn diagnose(
        &self,
        _invocation: &DiagnosticsInvocation,
        configuration: &ResolvedConfiguration,
    ) -> Result<DiagnosticsOutput, String> {
        self.calls.set(self.calls.get() + 1);
        self.digest.set(Some(*configuration.digest().as_bytes()));
        Err("intentional test stop".to_owned())
    }
}

impl RecordingConfigurationLoader {
    fn new(outcome: Result<ResolvedConfiguration, ConfigurationLoadError>) -> Self {
        Self {
            calls: Cell::new(0),
            invocation: RefCell::new(None),
            outcome,
        }
    }
}

impl ConfigurationLoader for RecordingConfigurationLoader {
    fn load(
        &self,
        invocation: &ConfigurationInvocation,
    ) -> Result<ResolvedConfiguration, ConfigurationLoadError> {
        self.calls.set(self.calls.get() + 1);
        self.invocation.replace(Some(invocation.clone()));
        self.outcome.clone()
    }
}

fn query_limited_configuration(limit: u64) -> ResolvedConfiguration {
    let layer = ConfigurationLayer::try_new(
        ConfigurationLayerKind::Repository,
        None,
        ConfigurationPreferenceOverrides::try_new(Some(limit), None, None, None, None, None)
            .expect("valid preference"),
        ConfigurationPolicyOverrides::default(),
    )
    .expect("valid layer");
    resolve_configuration(&[layer]).expect("resolved configuration")
}

#[test]
fn index_and_search_receive_one_explicit_resolved_configuration() {
    let configuration = query_limited_configuration(3);
    let index_loader = RecordingConfigurationLoader::new(Ok(configuration.clone()));
    let indexer = FakeIndexer::success(index_report());
    let args = [
        OsString::from("repowitness"),
        OsString::from("index"),
        OsString::from("--repository-config"),
        OsString::from("../repository.toml"),
        OsString::from("--database"),
        OsString::from("../index.db"),
        OsString::from("--user-config"),
        OsString::from("../user.toml"),
        OsString::from("--repository-id"),
        OsString::from("repository-id"),
        OsString::from("--workspace-config"),
        OsString::from("../workspace.toml"),
        OsString::from("../repository"),
    ];
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run_with_adapters(
        args,
        &mut stdout,
        &mut stderr,
        &FakeInspector::failure("must not be called"),
        &indexer,
        &FakeSearcher::failure("must not be called"),
        &FakeSymbolGetter::failure("must not be called"),
        &FakeMemory,
        &index_loader,
    );

    assert_eq!(code, EXIT_SUCCESS);
    assert!(stderr.is_empty());
    assert_eq!(index_loader.calls.get(), 1);
    let supplied = index_loader.invocation.borrow();
    let supplied = supplied.as_ref().expect("configuration invocation");
    assert_eq!(supplied.user.as_deref(), Some(Path::new("../user.toml")));
    assert_eq!(
        supplied.workspace.as_deref(),
        Some(Path::new("../workspace.toml"))
    );
    assert_eq!(
        supplied.repository.as_deref(),
        Some(Path::new("../repository.toml"))
    );
    let received = indexer.configuration.borrow();
    assert_eq!(
        received.as_ref().map(ResolvedConfiguration::digest),
        Some(configuration.digest())
    );

    let search_loader = RecordingConfigurationLoader::new(Ok(configuration.clone()));
    let searcher = FakeSearcher::success(search_report());
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run_with_adapters(
        [
            OsString::from("repowitness"),
            OsString::from("search"),
            OsString::from("--repository-id"),
            OsString::from("repository-id"),
            OsString::from("--database"),
            OsString::from("../index.db"),
            OsString::from("--query"),
            OsString::from("Widget"),
            OsString::from("--repository-config"),
            OsString::from("../repository.toml"),
        ],
        &mut stdout,
        &mut stderr,
        &FakeInspector::failure("must not be called"),
        &FakeIndexer::failure("must not be called"),
        &searcher,
        &FakeSymbolGetter::failure("must not be called"),
        &FakeMemory,
        &search_loader,
    );
    assert_eq!(code, EXIT_SUCCESS);
    assert!(stderr.is_empty());
    let received = searcher.configuration.borrow();
    assert_eq!(
        received
            .as_ref()
            .map(|value| *value.preferences().query_results().effective()),
        Some(3)
    );
}

#[test]
fn invalid_or_duplicate_configuration_fails_before_repository_adapters() {
    let loader = RecordingConfigurationLoader::new(Err(ConfigurationLoadError::Invalid));
    let searcher = FakeSearcher::failure("must not be called");
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run_with_adapters(
        [
            OsString::from("repowitness"),
            OsString::from("search"),
            OsString::from("--repository-id"),
            OsString::from("repository-id"),
            OsString::from("--database"),
            OsString::from("../index.db"),
            OsString::from("--query"),
            OsString::from("private query"),
            OsString::from("--user-config"),
            OsString::from("../private-configuration.toml"),
        ],
        &mut stdout,
        &mut stderr,
        &FakeInspector::failure("must not be called"),
        &FakeIndexer::failure("must not be called"),
        &searcher,
        &FakeSymbolGetter::failure("must not be called"),
        &FakeMemory,
        &loader,
    );
    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(loader.calls.get(), 1);
    assert_eq!(searcher.calls.get(), 0);
    assert!(stdout.is_empty());
    assert_eq!(stderr, b"error: configuration resolution failed\n");

    let loader = RecordingConfigurationLoader::new(Ok(query_limited_configuration(3)));
    let indexer = FakeIndexer::failure("must not be called");
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run_with_adapters(
        [
            OsString::from("repowitness"),
            OsString::from("index"),
            OsString::from("--repository-id"),
            OsString::from("repository-id"),
            OsString::from("--database"),
            OsString::from("../index.db"),
            OsString::from("--user-config"),
            OsString::from("one.toml"),
            OsString::from("--user-config"),
            OsString::from("two.toml"),
            OsString::from("../repository"),
        ],
        &mut stdout,
        &mut stderr,
        &FakeInspector::failure("must not be called"),
        &indexer,
        &FakeSearcher::failure("must not be called"),
        &FakeSymbolGetter::failure("must not be called"),
        &FakeMemory,
        &loader,
    );
    assert_eq!(code, EXIT_USAGE);
    assert_eq!(loader.calls.get(), 0);
    assert_eq!(indexer.calls.get(), 0);
    assert!(stdout.is_empty());
    assert_eq!(
        stderr,
        b"error: each configuration layer may be supplied only once\n"
    );
}

#[cfg(unix)]
#[test]
fn configuration_extraction_preserves_non_utf8_paths_and_option_shaped_values() {
    use std::os::unix::ffi::{OsStrExt, OsStringExt};

    let configuration_path = OsString::from_vec(vec![b'.', b'.', b'/', 0xFF]);
    let loader =
        RecordingConfigurationLoader::new(Ok(resolve_configuration(&[]).expect("defaults")));
    let searcher = FakeSearcher::success(search_report());
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run_with_adapters(
        [
            OsString::from("repowitness"),
            OsString::from("search"),
            OsString::from("--query"),
            OsString::from("--repository-config"),
            OsString::from("--database"),
            OsString::from("../index.db"),
            OsString::from("--user-config"),
            configuration_path.clone(),
            OsString::from("--repository-id"),
            OsString::from("repository-id"),
        ],
        &mut stdout,
        &mut stderr,
        &FakeInspector::failure("must not be called"),
        &FakeIndexer::failure("must not be called"),
        &searcher,
        &FakeSymbolGetter::failure("must not be called"),
        &FakeMemory,
        &loader,
    );
    assert_eq!(code, EXIT_SUCCESS);
    assert!(stderr.is_empty());
    assert_eq!(
        searcher.query.borrow().as_deref(),
        Some(OsStr::new("--repository-config"))
    );
    let supplied = loader.invocation.borrow();
    let supplied = supplied.as_ref().expect("configuration invocation");
    assert_eq!(
        supplied
            .user
            .as_ref()
            .expect("user configuration")
            .as_os_str()
            .as_bytes(),
        configuration_path.as_os_str().as_bytes()
    );
    assert!(supplied.repository.is_none());
}

#[test]
fn index_separator_keeps_an_option_shaped_repository_path_positional() {
    let configuration = resolve_configuration(&[]).expect("defaults");
    let loader = RecordingConfigurationLoader::new(Ok(configuration));
    let indexer = FakeIndexer::success(index_report());
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run_with_adapters(
        [
            OsString::from("repowitness"),
            OsString::from("index"),
            OsString::from("--repository-id"),
            OsString::from("repository-id"),
            OsString::from("--database"),
            OsString::from("../index.db"),
            OsString::from("--"),
            OsString::from("--repository-config"),
        ],
        &mut stdout,
        &mut stderr,
        &FakeInspector::failure("must not be called"),
        &indexer,
        &FakeSearcher::failure("must not be called"),
        &FakeSymbolGetter::failure("must not be called"),
        &FakeMemory,
        &loader,
    );

    assert_eq!(code, EXIT_SUCCESS);
    assert!(stderr.is_empty());
    assert_eq!(
        indexer.repository_root.borrow().as_deref(),
        Some(Path::new("--repository-config"))
    );
    let supplied = loader.invocation.borrow();
    let supplied = supplied.as_ref().expect("configuration invocation");
    assert!(supplied.user.is_none());
    assert!(supplied.workspace.is_none());
    assert!(supplied.repository.is_none());
}

#[test]
fn context_recall_and_diagnostics_share_the_resolved_configuration() {
    let configuration = query_limited_configuration(3);

    let context_loader = RecordingConfigurationLoader::new(Ok(configuration.clone()));
    let context = ConfigurationRecordingContextBuilder {
        calls: Cell::new(0),
        query_results: Cell::new(None),
    };
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run_context_build(
        [
            OsString::from("--root"),
            OsString::from("../repository"),
            OsString::from("--database"),
            OsString::from("../index.db"),
            OsString::from("--repository-id"),
            OsString::from("repository-id"),
            OsString::from("--intent"),
            OsString::from("Widget"),
            OsString::from("--workspace-config"),
            OsString::from("../workspace.toml"),
        ]
        .into_iter(),
        &mut stdout,
        &mut stderr,
        &context,
        &context_loader,
    );
    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(context.calls.get(), 1);
    assert_eq!(context.query_results.get(), Some(3));
    assert_eq!(context_loader.calls.get(), 1);

    let memory_loader = RecordingConfigurationLoader::new(Ok(configuration.clone()));
    let memory = ConfigurationRecordingMemory {
        calls: Cell::new(0),
        query_results: Cell::new(None),
    };
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run_memory_recall(
        [
            OsString::from("--all"),
            OsString::from("--repository-config"),
            OsString::from("../repository.toml"),
            OsString::from("--repository-id"),
            OsString::from("repository-id"),
            OsString::from("--database"),
            OsString::from("../index.db"),
        ]
        .into_iter(),
        &mut stdout,
        &mut stderr,
        &memory,
        &memory_loader,
    );
    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(memory.calls.get(), 1);
    assert_eq!(memory.query_results.get(), Some(3));
    assert_eq!(memory_loader.calls.get(), 1);

    let diagnostics_loader = RecordingConfigurationLoader::new(Ok(configuration.clone()));
    let diagnostics = ConfigurationRecordingDiagnosticsReader {
        calls: Cell::new(0),
        digest: Cell::new(None),
    };
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run_diagnostics(
        [
            OsString::from("--user-config"),
            OsString::from("../user.toml"),
            OsString::from("--database"),
            OsString::from("../index.db"),
            OsString::from("--repository-id"),
            OsString::from("repository-id"),
        ]
        .into_iter(),
        &mut stdout,
        &mut stderr,
        &diagnostics,
        &diagnostics_loader,
    );
    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(diagnostics.calls.get(), 1);
    assert_eq!(
        diagnostics.digest.get(),
        Some(*configuration.digest().as_bytes())
    );
    assert_eq!(diagnostics_loader.calls.get(), 1);

    let identity = mcp_configuration_identity(&configuration);
    assert_eq!(
        identity.digest_sha256,
        hex(configuration.digest().as_bytes())
    );
    assert_eq!(identity.schema_version, 1);
    assert_eq!(identity.resolver_version, 1);
    assert_eq!(identity.profile, "local");
    assert!(!identity.digest_sha256.contains('/'));
    assert!(!identity.digest_sha256.contains('\\'));
}
