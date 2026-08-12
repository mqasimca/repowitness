/// Complete explicit input for one bounded local Phase 0 indexing operation.
#[derive(Clone, Copy)]
pub struct LocalIndexRequest<'a> {
    repository_root: &'a Path,
    database: &'a Path,
    repository_identity: &'a str,
    migration_applied_at_unix_ms: u64,
    limits: LocalRustIndexLimits,
    configuration: Option<&'a ResolvedConfiguration>,
    build_graph: bool,
}

impl fmt::Debug for LocalIndexRequest<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalIndexRequest")
            .field("repository_root", &"<redacted-path>")
            .field("database", &"<redacted-path>")
            .field("repository_identity", &"<redacted-identity>")
            .field(
                "migration_applied_at_unix_ms",
                &self.migration_applied_at_unix_ms,
            )
            .field("limits", &self.limits)
            .field(
                "configuration_digest",
                &self.configuration.map(ResolvedConfiguration::digest),
            )
            .field("build_graph", &self.build_graph)
            .finish()
    }
}

impl<'a> LocalIndexRequest<'a> {
    /// Constructs a request using the conservative default indexing limits.
    #[must_use]
    pub fn new(
        repository_root: &'a Path,
        database: &'a Path,
        repository_identity: &'a str,
        migration_applied_at_unix_ms: u64,
    ) -> Self {
        Self {
            repository_root,
            database,
            repository_identity,
            migration_applied_at_unix_ms,
            limits: LocalRustIndexLimits::default(),
            configuration: None,
            build_graph: true,
        }
    }

    /// Replaces the complete end-to-end resource policy.
    #[must_use]
    pub const fn with_limits(mut self, limits: LocalRustIndexLimits) -> Self {
        self.limits = limits;
        self
    }

    /// Applies one fully resolved, path-free semantic configuration.
    #[must_use]
    pub const fn with_configuration(mut self, configuration: &'a ResolvedConfiguration) -> Self {
        self.configuration = Some(configuration);
        self
    }

    /// Skips the optional Rust graph projection while retaining atomic source
    /// facts, raw syntax sites, and repository topology.
    #[must_use]
    pub const fn without_graph(mut self) -> Self {
        self.build_graph = false;
        self
    }

    pub(crate) const fn build_graph(self) -> bool {
        self.build_graph
    }
}

/// Non-sensitive aggregate outcome from one activated local generation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalIndexReport {
    generation: GenerationId,
    source_epoch: u64,
    recovered_generations: u64,
    discovered_paths: u64,
    indexed_rust_files: u64,
    indexed_go_files: u64,
    indexed_typescript_files: u64,
    indexed_tsx_files: u64,
    indexed_python_files: u64,
    skipped_policy_paths: u64,
    skipped_unsupported_paths: u64,
    total_source_bytes: u64,
    total_facts: u64,
    syntax_error_nodes: u64,
    known_parser_limitation_nodes: u64,
    reused_rust_files: u64,
    analyzed_rust_files: u64,
    reused_go_files: u64,
    analyzed_go_files: u64,
    reused_typescript_files: u64,
    analyzed_typescript_files: u64,
    reused_tsx_files: u64,
    analyzed_tsx_files: u64,
    reused_python_files: u64,
    analyzed_python_files: u64,
}

impl LocalIndexReport {
    /// Returns the database-local active generation identity.
    #[must_use]
    pub const fn generation(self) -> GenerationId {
        self.generation
    }

    /// Returns the source epoch compared during atomic activation.
    #[must_use]
    pub const fn source_epoch(self) -> u64 {
        self.source_epoch
    }

    /// Returns incomplete generations recovered when the writer started.
    #[must_use]
    pub const fn recovered_generations(self) -> u64 {
        self.recovered_generations
    }

    /// Returns all repository paths admitted by bounded Git discovery.
    #[must_use]
    pub const fn discovered_paths(self) -> u64 {
        self.discovered_paths
    }

    /// Returns case-sensitive `.rs` files included in this generation.
    #[must_use]
    pub const fn indexed_rust_files(self) -> u64 {
        self.indexed_rust_files
    }

    /// Returns case-sensitive `.go` files included in this generation.
    #[must_use]
    pub const fn indexed_go_files(self) -> u64 {
        self.indexed_go_files
    }

    /// Returns case-sensitive `.ts` files included in this generation.
    #[must_use]
    pub const fn indexed_typescript_files(self) -> u64 {
        self.indexed_typescript_files
    }

    /// Returns case-sensitive `.tsx` files included in this generation.
    #[must_use]
    pub const fn indexed_tsx_files(self) -> u64 {
        self.indexed_tsx_files
    }

    /// Returns case-sensitive `.py` and `.pyi` files included in this generation.
    #[must_use]
    pub const fn indexed_python_files(self) -> u64 {
        self.indexed_python_files
    }

    /// Returns supported-language paths excluded by resolved policy.
    #[must_use]
    pub const fn skipped_policy_paths(self) -> u64 {
        self.skipped_policy_paths
    }

    /// Returns discovered paths outside the supported language scope.
    #[must_use]
    pub const fn skipped_unsupported_paths(self) -> u64 {
        self.skipped_unsupported_paths
    }

    /// Compatibility accessor for paths outside the indexed language scope.
    #[must_use]
    pub const fn skipped_non_rust_paths(self) -> u64 {
        self.skipped_unsupported_paths
    }

    /// Returns exact analyzed supported-source bytes.
    #[must_use]
    pub const fn total_source_bytes(self) -> u64 {
        self.total_source_bytes
    }

    /// Returns extracted symbol facts in the active generation.
    #[must_use]
    pub const fn total_facts(self) -> u64 {
        self.total_facts
    }

    /// Returns explicit Tree-sitter error-node coverage.
    #[must_use]
    pub const fn syntax_error_nodes(self) -> u64 {
        self.syntax_error_nodes
    }

    /// Returns the non-subtractive subset caused by known parser limitations.
    #[must_use]
    pub const fn known_parser_limitation_nodes(self) -> u64 {
        self.known_parser_limitation_nodes
    }

    /// Returns files restored from exact persisted analysis artifacts.
    #[must_use]
    pub const fn reused_rust_files(self) -> u64 {
        self.reused_rust_files
    }

    /// Returns files parsed by the current Rust analysis producer.
    #[must_use]
    pub const fn analyzed_rust_files(self) -> u64 {
        self.analyzed_rust_files
    }

    /// Returns Go files restored from exact persisted analysis artifacts.
    #[must_use]
    pub const fn reused_go_files(self) -> u64 {
        self.reused_go_files
    }

    /// Returns Go files parsed by the current analysis producer.
    #[must_use]
    pub const fn analyzed_go_files(self) -> u64 {
        self.analyzed_go_files
    }

    /// Returns TypeScript files restored from exact persisted analysis artifacts.
    #[must_use]
    pub const fn reused_typescript_files(self) -> u64 {
        self.reused_typescript_files
    }

    /// Returns TypeScript files parsed by the current analysis producer.
    #[must_use]
    pub const fn analyzed_typescript_files(self) -> u64 {
        self.analyzed_typescript_files
    }

    /// Returns TSX files restored from exact persisted analysis artifacts.
    #[must_use]
    pub const fn reused_tsx_files(self) -> u64 {
        self.reused_tsx_files
    }

    /// Returns TSX files parsed by the current analysis producer.
    #[must_use]
    pub const fn analyzed_tsx_files(self) -> u64 {
        self.analyzed_tsx_files
    }

    /// Returns Python files restored from exact persisted analysis artifacts.
    #[must_use]
    pub const fn reused_python_files(self) -> u64 {
        self.reused_python_files
    }

    /// Returns Python files parsed by the current analysis producer.
    #[must_use]
    pub const fn analyzed_python_files(self) -> u64 {
        self.analyzed_python_files
    }
}
