use super::*;

struct RecordingMemory {
    revalidate_result: RefCell<Option<Result<CliMemoryRevalidationReport, &'static str>>>,
    recall_result: RefCell<Option<Result<MemoryRecallOutput, &'static str>>>,
    revalidate_calls: Cell<u64>,
    recall_calls: Cell<u64>,
    repository_root: RefCell<Option<PathBuf>>,
    database: RefCell<Option<PathBuf>>,
    repository_identity: RefCell<Option<OsString>>,
    recall_all: Cell<Option<bool>>,
    recall_query: RefCell<Option<OsString>>,
    recall_limit: Cell<Option<u16>>,
}

impl RecordingMemory {
    fn revalidation(report: CliMemoryRevalidationReport) -> Self {
        Self {
            revalidate_result: RefCell::new(Some(Ok(report))),
            recall_result: RefCell::new(Some(Err("must not be called"))),
            revalidate_calls: Cell::new(0),
            recall_calls: Cell::new(0),
            repository_root: RefCell::new(None),
            database: RefCell::new(None),
            repository_identity: RefCell::new(None),
            recall_all: Cell::new(None),
            recall_query: RefCell::new(None),
            recall_limit: Cell::new(None),
        }
    }

    fn recall(report: MemoryRecallOutput) -> Self {
        Self {
            revalidate_result: RefCell::new(Some(Err("must not be called"))),
            recall_result: RefCell::new(Some(Ok(report))),
            revalidate_calls: Cell::new(0),
            recall_calls: Cell::new(0),
            repository_root: RefCell::new(None),
            database: RefCell::new(None),
            repository_identity: RefCell::new(None),
            recall_all: Cell::new(None),
            recall_query: RefCell::new(None),
            recall_limit: Cell::new(None),
        }
    }

    fn failure(message: &'static str) -> Self {
        Self {
            revalidate_result: RefCell::new(Some(Err(message))),
            recall_result: RefCell::new(Some(Err(message))),
            revalidate_calls: Cell::new(0),
            recall_calls: Cell::new(0),
            repository_root: RefCell::new(None),
            database: RefCell::new(None),
            repository_identity: RefCell::new(None),
            recall_all: Cell::new(None),
            recall_query: RefCell::new(None),
            recall_limit: Cell::new(None),
        }
    }
}

impl RepositoryMemory for RecordingMemory {
    fn revalidate(
        &self,
        invocation: &MemoryRevalidationInvocation,
    ) -> Result<CliMemoryRevalidationReport, String> {
        self.revalidate_calls.set(self.revalidate_calls.get() + 1);
        self.repository_root
            .replace(Some(invocation.repository_root.clone()));
        self.database.replace(Some(invocation.database.clone()));
        self.repository_identity
            .replace(Some(invocation.repository_identity.clone()));
        self.revalidate_result
            .borrow_mut()
            .take()
            .expect("revalidation fake is called at most once")
            .map_err(str::to_owned)
    }

    fn recall(&self, invocation: &MemoryRecallInvocation) -> Result<MemoryRecallOutput, String> {
        self.recall_calls.set(self.recall_calls.get() + 1);
        self.database.replace(Some(invocation.database.clone()));
        self.repository_identity
            .replace(Some(invocation.repository_identity.clone()));
        self.recall_limit.set(Some(invocation.max_results));
        match &invocation.selection {
            CliMemoryRecallSelection::All => self.recall_all.set(Some(true)),
            CliMemoryRecallSelection::Query(query) => {
                self.recall_all.set(Some(false));
                self.recall_query.replace(Some(query.clone()));
            }
        }
        self.recall_result
            .borrow_mut()
            .take()
            .expect("recall fake is called at most once")
            .map_err(str::to_owned)
    }
}

fn invoke_memory(arguments: &[&str], memory: &impl RepositoryMemory) -> (u8, String, String) {
    let args =
        std::iter::once(OsString::from("repowitness")).chain(arguments.iter().map(OsString::from));
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run_with_adapters(
        args,
        &mut stdout,
        &mut stderr,
        &FakeInspector::failure("must not be called"),
        &FakeIndexer::failure("must not be called"),
        &FakeSearcher::failure("must not be called"),
        &FakeSymbolGetter::failure("must not be called"),
        memory,
    );
    (
        code,
        String::from_utf8(stdout).expect("stdout is UTF-8"),
        String::from_utf8(stderr).expect("stderr is UTF-8"),
    )
}

fn identity() -> String {
    format!("rwi1:h:{}", "06".repeat(32))
}

fn recall_output() -> MemoryRecallOutput {
    MemoryRecallOutput {
        schema_version: 1,
        recall_profile: 1,
        query_sha256: Some("11".repeat(32)),
        snapshot_sha256: "22".repeat(32),
        generation: 7,
        projection: 4,
        source_epoch: 2,
        target: McpMemoryTarget {
            kind: "worktree".to_owned(),
            source_snapshot_sha256: Some("22".repeat(32)),
            commit_object_format: Some("sha1".to_owned()),
            commit_hex: Some("33".repeat(20)),
        },
        producer: McpMemoryProducer {
            id: "rust-correspondence-v1".to_owned(),
            version: 1,
            profile_sha256: "44".repeat(32),
        },
        matches_returned: 1,
        matches_total: 1,
        matches_omitted: 0,
        coverage: McpMemoryCoverage {
            searched: 1,
            skipped: 0,
            unresolved: 0,
            truncated: 0,
            total: 1,
            current: 1,
            not_applicable: 0,
            stale: 0,
            needs_review: 0,
            indeterminate: 0,
            conflicted: 0,
            contradicted: 0,
            superseded: 0,
            quarantined: 0,
            tombstoned: 0,
        },
        limitation: "rust_symbol_memory_only".to_owned(),
        records: vec![McpMemoryRecord {
            record_id: "mem_00000000000000000000000000".to_owned(),
            revision_sha256: Some("55".repeat(32)),
            selected: Some(McpSelectedMemory {
                schema_version: 1,
                display_revision: 3,
                kind: "decision".to_owned(),
                title: "Private=title\n".to_owned(),
                body: "private body".to_owned(),
                assurance: "locally_approved".to_owned(),
                lifecycle: "active".to_owned(),
                tombstone: false,
            }),
            effective_state: "current".to_owned(),
            validity_state: "valid".to_owned(),
            evidence_state: "exact".to_owned(),
            reason: "evidence_exact".to_owned(),
            evidence_count: 0,
            resolved_count: 0,
            review_count: 0,
            indeterminate_count: 0,
            head_count: 1,
            missing_parent_count: 0,
            evidence: Vec::new(),
        }],
    }
}

#[test]
fn memory_revalidation_passes_explicit_inputs_and_reports_public_counts() {
    let memory = RecordingMemory::revalidation(CliMemoryRevalidationReport {
        projection_id: 4,
        generation: 7,
        source_epoch: 2,
        recovered_generations: 1,
        projected_records: 3,
        skipped_records: 1,
        unresolved_records: 2,
        git_queries: 5,
        head_available: true,
    });
    let identity = identity();
    let (code, stdout, stderr) = invoke_memory(
        &[
            "memory-revalidate",
            "--database",
            "../private.db",
            "--repository-id",
            &identity,
            "../private-repository",
        ],
        &memory,
    );
    assert_eq!(code, EXIT_SUCCESS);
    assert!(stderr.is_empty());
    assert!(stdout.contains("operation=memory-revalidate\n"));
    assert!(stdout.contains("projection=4\ngeneration=7\nsource_epoch=2\n"));
    assert!(stdout.contains("projected_records=3\n"));
    assert!(!stdout.contains("private"));
    assert_eq!(memory.revalidate_calls.get(), 1);
    assert_eq!(
        memory.repository_root.borrow().as_deref(),
        Some(Path::new("../private-repository"))
    );
    assert_eq!(
        memory.database.borrow().as_deref(),
        Some(Path::new("../private.db"))
    );
    assert_eq!(
        memory.repository_identity.borrow().as_deref(),
        Some(OsStr::new(&identity))
    );
}

#[test]
fn memory_recall_passes_literal_selection_and_encodes_untrusted_text() {
    let memory = RecordingMemory::recall(recall_output());
    let identity = identity();
    let (code, stdout, stderr) = invoke_memory(
        &[
            "memory-recall",
            "--repository-id",
            &identity,
            "--database",
            "../private.db",
            "--query",
            "Private Decision",
            "--limit",
            "7",
        ],
        &memory,
    );
    assert_eq!(code, EXIT_SUCCESS);
    assert!(stderr.is_empty());
    assert!(stdout.contains("operation=memory-recall\nrecall_profile=1\n"));
    assert!(stdout.contains("record_0_selected_title_encoding=lowercase_hex\n"));
    assert!(stdout.contains(&format!(
        "record_0_selected_title_hex={}\n",
        hex(b"Private=title\n")
    )));
    assert!(!stdout.contains("Private=title"));
    assert!(!stdout.contains("private body"));
    assert_eq!(memory.recall_calls.get(), 1);
    assert_eq!(memory.recall_all.get(), Some(false));
    assert_eq!(
        memory.recall_query.borrow().as_deref(),
        Some(OsStr::new("Private Decision"))
    );
    assert_eq!(memory.recall_limit.get(), Some(7));
}

#[test]
fn memory_command_boundaries_fail_before_adapter_io() {
    let memory = RecordingMemory::failure("must not be called");
    for arguments in [
        vec!["memory-recall"],
        vec!["memory-recall", "--all"],
        vec!["memory-recall", "--repository-id", "id", "--database", "db"],
        vec![
            "memory-recall",
            "--repository-id",
            "id",
            "--database",
            "db",
            "--all",
            "--query",
            "term",
        ],
        vec![
            "memory-recall",
            "--repository-id",
            "id",
            "--database",
            "db",
            "--all",
            "--limit",
            "0",
        ],
        vec!["memory-revalidate"],
        vec!["memory-revalidate", "--database", "db", "repository"],
        vec![
            "memory-revalidate",
            "--repository-id",
            "id",
            "--database",
            "db",
            "--unknown",
        ],
    ] {
        let (code, stdout, stderr) = invoke_memory(&arguments, &memory);
        assert_eq!(code, EXIT_USAGE);
        assert!(stdout.is_empty());
        assert!(stderr.starts_with("error:"));
    }
    assert_eq!(memory.revalidate_calls.get(), 0);
    assert_eq!(memory.recall_calls.get(), 0);
}

#[test]
fn memory_help_and_failures_are_redacted() {
    let memory = RecordingMemory::failure("sensitive adapter detail: private query");
    for command in ["memory-revalidate", "memory-recall"] {
        let (code, stdout, stderr) = invoke_memory(&[command, "--help"], &memory);
        assert_eq!(code, EXIT_SUCCESS);
        assert!(!stdout.is_empty());
        assert!(stderr.is_empty());
    }

    let identity = identity();
    let (code, stdout, stderr) = invoke_memory(
        &[
            "memory-recall",
            "--repository-id",
            &identity,
            "--database",
            "private.db",
            "--query",
            "private query",
        ],
        &memory,
    );
    assert_eq!(code, EXIT_SOFTWARE);
    assert!(stdout.is_empty());
    assert_eq!(stderr, "error: memory recall failed\n");
    assert!(!stderr.contains("private"));
    assert!(!stderr.contains(&identity));

    let memory = RecordingMemory::failure("sensitive adapter detail: private repository");
    let (code, stdout, stderr) = invoke_memory(
        &[
            "memory-revalidate",
            "--repository-id",
            &identity,
            "--database",
            "private.db",
            "private-repository",
        ],
        &memory,
    );
    assert_eq!(code, EXIT_SOFTWARE);
    assert!(stdout.is_empty());
    assert_eq!(stderr, "error: memory revalidation failed\n");
}
