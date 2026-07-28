use std::fmt;

use super::{CanonicalSourceManifest, PreparedRustFile, PreparedRustIndex, SourceManifestDigest};

impl PreparedRustIndex {
    /// Returns the canonical content-manifest identity.
    #[must_use]
    pub const fn manifest_digest(&self) -> SourceManifestDigest {
        self.manifest_digest
    }

    /// Returns the canonical, sorted source manifest.
    #[must_use]
    pub const fn manifest(&self) -> &CanonicalSourceManifest {
        &self.manifest
    }

    /// Returns per-file output in canonical repository-path order.
    #[must_use]
    pub const fn files(&self) -> &[PreparedRustFile] {
        &self.files
    }

    /// Returns the exact aggregate immutable source-byte count.
    #[must_use]
    pub const fn total_source_bytes(&self) -> u64 {
        self.total_source_bytes
    }

    /// Returns the aggregate extracted symbol-fact count.
    #[must_use]
    pub const fn total_facts(&self) -> u64 {
        self.total_facts
    }

    /// Returns the aggregate explicit Tree-sitter error-node count.
    #[must_use]
    pub const fn total_syntax_error_nodes(&self) -> u64 {
        self.total_syntax_error_nodes
    }

    /// Returns the conservative non-subtractive subset attributed to known parser limitations.
    #[must_use]
    pub const fn total_known_parser_limitation_nodes(&self) -> u64 {
        self.total_known_parser_limitation_nodes
    }

    /// Returns files restored from exact, validated analysis artifacts.
    #[must_use]
    pub const fn reused_files(&self) -> u64 {
        self.reused_files
    }

    /// Returns files analyzed by the current syntax producer.
    #[must_use]
    pub const fn analyzed_files(&self) -> u64 {
        self.analyzed_files
    }

    /// Returns indexed Rust files.
    #[must_use]
    pub const fn indexed_rust_files(&self) -> u64 {
        self.indexed_rust_files
    }

    /// Returns indexed Go files.
    #[must_use]
    pub const fn indexed_go_files(&self) -> u64 {
        self.indexed_go_files
    }

    /// Returns indexed plain TypeScript files.
    #[must_use]
    pub const fn indexed_typescript_files(&self) -> u64 {
        self.indexed_typescript_files
    }

    /// Returns indexed JSX-aware TypeScript files.
    #[must_use]
    pub const fn indexed_tsx_files(&self) -> u64 {
        self.indexed_tsx_files
    }

    /// Returns indexed Python files.
    #[must_use]
    pub const fn indexed_python_files(&self) -> u64 {
        self.indexed_python_files
    }

    /// Returns exact reused Rust artifacts.
    #[must_use]
    pub const fn reused_rust_files(&self) -> u64 {
        self.reused_rust_files
    }

    /// Returns exact reused Go artifacts.
    #[must_use]
    pub const fn reused_go_files(&self) -> u64 {
        self.reused_go_files
    }

    /// Returns exact reused plain TypeScript artifacts.
    #[must_use]
    pub const fn reused_typescript_files(&self) -> u64 {
        self.reused_typescript_files
    }

    /// Returns exact reused JSX-aware TypeScript artifacts.
    #[must_use]
    pub const fn reused_tsx_files(&self) -> u64 {
        self.reused_tsx_files
    }

    /// Returns exact reused Python artifacts.
    #[must_use]
    pub const fn reused_python_files(&self) -> u64 {
        self.reused_python_files
    }

    /// Returns Rust files parsed by the current producer.
    #[must_use]
    pub const fn analyzed_rust_files(&self) -> u64 {
        self.analyzed_rust_files
    }

    /// Returns Go files parsed by the current producer.
    #[must_use]
    pub const fn analyzed_go_files(&self) -> u64 {
        self.analyzed_go_files
    }

    /// Returns plain TypeScript files parsed by the current producer.
    #[must_use]
    pub const fn analyzed_typescript_files(&self) -> u64 {
        self.analyzed_typescript_files
    }

    /// Returns JSX-aware TypeScript files parsed by the current producer.
    #[must_use]
    pub const fn analyzed_tsx_files(&self) -> u64 {
        self.analyzed_tsx_files
    }

    /// Returns Python files parsed by the current producer.
    #[must_use]
    pub const fn analyzed_python_files(&self) -> u64 {
        self.analyzed_python_files
    }
}

impl fmt::Debug for PreparedRustIndex {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedRustIndex")
            .field("manifest_digest", &self.manifest_digest)
            .field("file_count", &self.manifest.count())
            .field("total_source_bytes", &self.total_source_bytes)
            .field("total_facts", &self.total_facts)
            .field("total_syntax_error_nodes", &self.total_syntax_error_nodes)
            .field(
                "total_known_parser_limitation_nodes",
                &self.total_known_parser_limitation_nodes,
            )
            .field("reused_files", &self.reused_files)
            .field("analyzed_files", &self.analyzed_files)
            .field("indexed_rust_files", &self.indexed_rust_files)
            .field("indexed_go_files", &self.indexed_go_files)
            .field("indexed_typescript_files", &self.indexed_typescript_files)
            .field("indexed_tsx_files", &self.indexed_tsx_files)
            .field("indexed_python_files", &self.indexed_python_files)
            .finish()
    }
}
