use super::*;

#[path = "memory_manage_outcome.rs"]
mod outcome_unknown;

#[derive(Debug, PartialEq, Eq)]
enum CapturedManageInvocation {
    Write {
        repository_root: PathBuf,
        repository_identity: OsString,
        input: PathBuf,
    },
    Approve {
        repository_root: PathBuf,
        database: PathBuf,
        repository_identity: OsString,
        record_id: OsString,
        actor: OsString,
    },
    Sync {
        repository_root: PathBuf,
        database: PathBuf,
        repository_identity: OsString,
        record_id: OsString,
        actor: OsString,
    },
    Review {
        repository_root: PathBuf,
        database: PathBuf,
        repository_identity: OsString,
        record_id: OsString,
        revision: OsString,
        evidence_ordinal: u8,
        operation: MemoryCorrespondenceReviewOperation,
        target_path: OsString,
        target_artifact: OsString,
        target_fact_ordinal: u64,
        target_snapshot: Option<OsString>,
        actor: OsString,
    },
    ImportHistory {
        repository_root: PathBuf,
        database: PathBuf,
        repository_identity: OsString,
        actor: OsString,
    },
}

struct RecordingManager {
    result: RefCell<Option<Result<CliMemoryManageReport, CliMemoryError>>>,
    captured: RefCell<Option<CapturedManageInvocation>>,
    calls: Cell<u64>,
}

impl RecordingManager {
    fn success(report: CliMemoryManageReport) -> Self {
        Self {
            result: RefCell::new(Some(Ok(report))),
            captured: RefCell::new(None),
            calls: Cell::new(0),
        }
    }

    fn failure(message: &'static str) -> Self {
        let _ = message;
        Self {
            result: RefCell::new(Some(Err(CliMemoryError::Failed))),
            captured: RefCell::new(None),
            calls: Cell::new(0),
        }
    }

    fn outcome_unknown(
        request_scope: MemoryMutationRequestScope,
        operation: MemoryMutationOperation,
    ) -> Self {
        Self {
            result: RefCell::new(Some(Err(CliMemoryError::MutationOutcomeUnknown {
                request_scope,
                operation,
            }))),
            captured: RefCell::new(None),
            calls: Cell::new(0),
        }
    }
}

impl RepositoryMemory for RecordingManager {
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

    fn manage(
        &self,
        invocation: &MemoryManageInvocation,
    ) -> Result<CliMemoryManageReport, CliMemoryError> {
        self.calls.set(self.calls.get() + 1);
        let captured = match invocation {
            MemoryManageInvocation::Write {
                repository_root,
                repository_identity,
                input,
            } => CapturedManageInvocation::Write {
                repository_root: repository_root.clone(),
                repository_identity: repository_identity.clone(),
                input: input.clone(),
            },
            MemoryManageInvocation::Approve {
                repository_root,
                database,
                repository_identity,
                record_id,
                actor,
            } => CapturedManageInvocation::Approve {
                repository_root: repository_root.clone(),
                database: database.clone(),
                repository_identity: repository_identity.clone(),
                record_id: record_id.clone(),
                actor: actor.clone(),
            },
            MemoryManageInvocation::Sync {
                repository_root,
                database,
                repository_identity,
                record_id,
                actor,
            } => CapturedManageInvocation::Sync {
                repository_root: repository_root.clone(),
                database: database.clone(),
                repository_identity: repository_identity.clone(),
                record_id: record_id.clone(),
                actor: actor.clone(),
            },
            MemoryManageInvocation::Review {
                repository_root,
                database,
                repository_identity,
                record_id,
                revision,
                evidence_ordinal,
                operation,
                target_path,
                target_artifact,
                target_fact_ordinal,
                target_snapshot,
                actor,
            } => CapturedManageInvocation::Review {
                repository_root: repository_root.clone(),
                database: database.clone(),
                repository_identity: repository_identity.clone(),
                record_id: record_id.clone(),
                revision: revision.clone(),
                evidence_ordinal: *evidence_ordinal,
                operation: *operation,
                target_path: target_path.clone(),
                target_artifact: target_artifact.clone(),
                target_fact_ordinal: *target_fact_ordinal,
                target_snapshot: target_snapshot.clone(),
                actor: actor.clone(),
            },
            MemoryManageInvocation::ImportHistory {
                repository_root,
                database,
                repository_identity,
                actor,
            } => CapturedManageInvocation::ImportHistory {
                repository_root: repository_root.clone(),
                database: database.clone(),
                repository_identity: repository_identity.clone(),
                actor: actor.clone(),
            },
        };
        self.captured.replace(Some(captured));
        self.result
            .borrow_mut()
            .take()
            .expect("management fake is called at most once")
    }
}

fn invoke_manage(arguments: &[&str], memory: &impl RepositoryMemory) -> (u8, String, String) {
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
        &LocalConfigurationLoader,
    );
    (
        code,
        String::from_utf8(stdout).expect("stdout is UTF-8"),
        String::from_utf8(stderr).expect("stderr is UTF-8"),
    )
}

fn identity() -> String {
    format!("rwi1:h:{}", "07".repeat(32))
}

#[test]
fn memory_manage_write_passes_explicit_paths_and_emits_safe_json() {
    let manager = RecordingManager::success(CliMemoryManageReport::Write {
        revision: "11".repeat(32),
        created: true,
        canonical_bytes: 619,
        publication: CliMemoryPublicationStatus::confirmed_for_test(),
    });
    let identity = identity();
    let (code, stdout, stderr) = invoke_manage(
        &[
            "memory-manage",
            "write",
            "--repository-id",
            &identity,
            "--input",
            "../private-input.yaml",
            "--",
            "../private-repository",
        ],
        &manager,
    );

    assert_eq!(code, EXIT_SUCCESS);
    assert!(stderr.is_empty());
    assert_eq!(
        stdout,
        format!(
            "{{\"schema_version\":2,\"operation\":\"write\",\"revision_sha256\":\"{}\",\"created\":true,\"canonical_bytes\":619,\"publication\":{{\"complete\":true,\"warning_count\":0,\"temporary_cleanup\":\"complete\",\"target_identity\":\"confirmed_at_final_fence\",\"records_directory_identity\":\"confirmed_at_final_fence\",\"directory_sync\":\"complete\"}}}}\n",
            "11".repeat(32)
        )
    );
    assert_eq!(manager.calls.get(), 1);
    assert_eq!(
        manager.captured.borrow().as_ref(),
        Some(&CapturedManageInvocation::Write {
            repository_root: PathBuf::from("../private-repository"),
            repository_identity: OsString::from(identity),
            input: PathBuf::from("../private-input.yaml"),
        })
    );
    assert!(!stdout.contains("private"));
}

#[test]
fn memory_manage_approval_and_history_keep_trust_inputs_explicit() {
    let identity = identity();
    let approval = RecordingManager::success(CliMemoryManageReport::Approve {
        revision: "22".repeat(32),
        version_inserted: false,
        observation_inserted: true,
        approval_inserted: true,
        maintenance: CliMemoryMaintenanceStatus::confirmed_for_test(),
    });
    let (code, stdout, stderr) = invoke_manage(
        &[
            "memory-manage",
            "approve",
            "--repository-id",
            &identity,
            "--database",
            "../private.db",
            "--record-id",
            "mem_00000000000000000000000000",
            "--actor",
            "local-user",
            "../private-repository",
        ],
        &approval,
    );
    assert_eq!(code, EXIT_SUCCESS);
    assert!(stderr.is_empty());
    assert!(stdout.contains("\"schema_version\":2"));
    assert!(stdout.contains("\"operation\":\"approve\""));
    assert!(stdout.contains("\"approval_inserted\":true"));
    assert!(stdout.contains(
        "\"maintenance\":{\"complete\":true,\"warning_count\":0,\"checkpoint\":\"complete\",\"shutdown\":\"complete\",\"database_identity\":\"confirmed_at_final_fence\"}"
    ));
    assert_eq!(
        approval.captured.borrow().as_ref(),
        Some(&CapturedManageInvocation::Approve {
            repository_root: PathBuf::from("../private-repository"),
            database: PathBuf::from("../private.db"),
            repository_identity: OsString::from(&identity),
            record_id: OsString::from("mem_00000000000000000000000000"),
            actor: OsString::from("local-user"),
        })
    );

    let history = RecordingManager::success(CliMemoryManageReport::ImportHistory {
        commits_inspected: 3,
        records_inspected: 5,
        imported_versions: 2,
        appended_observations: 5,
        total_record_bytes: 4096,
        git_processes: 11,
        history_complete: true,
        maintenance: CliMemoryMaintenanceStatus::confirmed_for_test(),
    });
    let (code, stdout, stderr) = invoke_manage(
        &[
            "memory-manage",
            "import-history",
            "--repository-id",
            &identity,
            "--database",
            "../private.db",
            "--actor",
            "local-user",
            "../private-repository",
        ],
        &history,
    );
    assert_eq!(code, EXIT_SUCCESS);
    assert!(stderr.is_empty());
    assert!(stdout.contains("\"schema_version\":2"));
    assert!(stdout.contains("\"operation\":\"import_history\""));
    assert!(stdout.contains("\"records_inspected\":5"));
    assert!(stdout.contains(
        "\"maintenance\":{\"complete\":true,\"warning_count\":0,\"checkpoint\":\"complete\",\"shutdown\":\"complete\",\"database_identity\":\"confirmed_at_final_fence\"}"
    ));
    assert_eq!(
        history.captured.borrow().as_ref(),
        Some(&CapturedManageInvocation::ImportHistory {
            repository_root: PathBuf::from("../private-repository"),
            database: PathBuf::from("../private.db"),
            repository_identity: OsString::from(identity),
            actor: OsString::from("local-user"),
        })
    );
}

#[test]
fn memory_manage_sync_requires_the_explicit_team_record_selector() {
    let identity = identity();
    let manager = RecordingManager::success(CliMemoryManageReport::Sync {
        revision: "33".repeat(32),
        version_inserted: true,
        observation_inserted: true,
        maintenance: CliMemoryMaintenanceStatus::confirmed_for_test(),
    });
    let (code, stdout, stderr) = invoke_manage(
        &[
            "memory-manage",
            "sync",
            "--repository-id",
            &identity,
            "--database",
            "../private.sqlite3",
            "--record-id",
            "rwm1:h:00000000000000000000000000",
            "--actor",
            "reviewer",
            "--",
            "../private-repository",
        ],
        &manager,
    );
    assert_eq!(code, EXIT_SUCCESS);
    assert!(stderr.is_empty());
    assert!(stdout.contains("\"operation\":\"sync\""));
    assert!(stdout.contains(&"33".repeat(32)));
    assert_eq!(
        manager.captured.borrow().as_ref(),
        Some(&CapturedManageInvocation::Sync {
            repository_root: PathBuf::from("../private-repository"),
            database: PathBuf::from("../private.sqlite3"),
            repository_identity: OsString::from(identity),
            record_id: OsString::from("rwm1:h:00000000000000000000000000"),
            actor: OsString::from("reviewer"),
        })
    );
}

#[test]
fn memory_manage_review_parses_exact_selector_and_emits_safe_json() {
    let manager = RecordingManager::success(CliMemoryManageReport::Review {
        inserted: true,
        maintenance: CliMemoryMaintenanceStatus::checkpoint_deferred_for_test(),
    });
    let identity = identity();
    let revision = "44".repeat(32);
    let artifact = "55".repeat(32);
    let (code, stdout, stderr) = invoke_manage(
        &[
            "memory-manage",
            "review",
            "--repository-id",
            &identity,
            "--database",
            "../private.db",
            "--record-id",
            "mem_00000000000000000000000000",
            "--revision",
            &revision,
            "--evidence",
            "15",
            "--operation",
            "manual-link",
            "--target-path",
            "rwp1:h:7372632F6C69622E7273",
            "--target-artifact",
            &artifact,
            "--target-fact",
            "9007199254740991",
            "--actor",
            "trusted-reviewer",
            "../private-repository",
        ],
        &manager,
    );

    assert_eq!(code, EXIT_SUCCESS);
    assert!(stderr.is_empty());
    assert_eq!(
        stdout,
        "{\"schema_version\":2,\"operation\":\"review\",\"inserted\":true,\"maintenance\":{\"complete\":false,\"warning_count\":1,\"checkpoint\":\"deferred\",\"shutdown\":\"complete\",\"database_identity\":\"confirmed_at_final_fence\"}}\n"
    );
    assert_eq!(
        manager.captured.borrow().as_ref(),
        Some(&CapturedManageInvocation::Review {
            repository_root: PathBuf::from("../private-repository"),
            database: PathBuf::from("../private.db"),
            repository_identity: OsString::from(identity),
            record_id: OsString::from("mem_00000000000000000000000000"),
            revision: OsString::from(revision),
            evidence_ordinal: 15,
            operation: MemoryCorrespondenceReviewOperation::ManualLink,
            target_path: OsString::from("rwp1:h:7372632F6C69622E7273"),
            target_artifact: OsString::from(artifact),
            target_fact_ordinal: 9_007_199_254_740_991,
            target_snapshot: None,
            actor: OsString::from("trusted-reviewer"),
        })
    );
    assert!(!stdout.contains("private"));
    assert!(!stdout.contains("trusted-reviewer"));
}

#[test]
fn changed_database_identity_is_a_warning_not_complete_maintenance() {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let report = CliMemoryManageReport::Review {
        inserted: true,
        maintenance: CliMemoryMaintenanceStatus::changed_database_for_test(),
    };

    assert_eq!(
        emit_memory_manage_report(&mut stdout, &mut stderr, report),
        EXIT_SUCCESS
    );
    assert!(stderr.is_empty());
    let value: serde_json::Value =
        serde_json::from_slice(&stdout).expect("receipt should be valid JSON");
    let maintenance = &value["maintenance"];
    assert_eq!(maintenance["complete"], false);
    assert_eq!(maintenance["warning_count"], 1);
    assert_eq!(maintenance["checkpoint"], "complete");
    assert_eq!(maintenance["shutdown"], "complete");
    assert_eq!(maintenance["database_identity"], "changed_after_commit");
}

const INVALID_MEMORY_MANAGE_ARGUMENTS: &[&[&str]] = &[
    &["memory-manage"],
    &["memory-manage", "unknown"],
    &[
        "memory-manage",
        "write",
        "--repository-id",
        "id",
        "repository",
    ],
    &[
        "memory-manage",
        "write",
        "--repository-id",
        "id",
        "--input",
        "one",
        "--input",
        "two",
        "repository",
    ],
    &[
        "memory-manage",
        "write",
        "--repository-id",
        "id",
        "--database",
        "db",
        "--input",
        "input",
        "repository",
    ],
    &[
        "memory-manage",
        "approve",
        "--repository-id",
        "id",
        "--database",
        "db",
        "--record-id",
        "record",
        "--actor",
        "actor",
    ],
    &[
        "memory-manage",
        "import-history",
        "--repository-id",
        "id",
        "--database",
        "db",
        "--actor",
        "actor",
        "one",
        "two",
    ],
    &[
        "memory-manage",
        "review",
        "--repository-id",
        "id",
        "--database",
        "db",
        "--record-id",
        "record",
        "--revision",
        "revision",
        "--evidence",
        "16",
        "--operation",
        "approve",
        "--target-path",
        "path",
        "--target-artifact",
        "artifact",
        "--target-fact",
        "0",
        "--actor",
        "actor",
        "repository",
    ],
    &[
        "memory-manage",
        "review",
        "--repository-id",
        "id",
        "--database",
        "db",
        "--record-id",
        "record",
        "--revision",
        "revision",
        "--evidence",
        "0",
        "--operation",
        "guess",
        "--target-path",
        "path",
        "--target-artifact",
        "artifact",
        "--target-fact",
        "0",
        "--actor",
        "actor",
        "repository",
    ],
    &[
        "memory-manage",
        "review",
        "--repository-id",
        "id",
        "--database",
        "db",
        "--record-id",
        "record",
        "--revision",
        "revision",
        "--evidence",
        "0",
        "--operation",
        "approve",
        "--target-path",
        "path",
        "--target-artifact",
        "artifact",
        "--target-fact",
        "9007199254740992",
        "--actor",
        "actor",
        "repository",
    ],
];

#[test]
fn memory_manage_rejects_incomplete_ambiguous_and_cross_operation_options() {
    let manager = RecordingManager::failure("must not be called");
    for &arguments in INVALID_MEMORY_MANAGE_ARGUMENTS {
        let (code, stdout, stderr) = invoke_manage(arguments, &manager);
        assert_eq!(code, EXIT_USAGE);
        assert!(stdout.is_empty());
        assert!(stderr.starts_with("error:"));
    }
    assert_eq!(manager.calls.get(), 0);
}

#[test]
fn memory_manage_help_and_failure_do_not_echo_sensitive_inputs() {
    let manager = RecordingManager::failure("sensitive adapter detail: private-input.yaml");
    let (code, stdout, stderr) = invoke_manage(&["memory-manage", "--help"], &manager);
    assert_eq!(code, EXIT_SUCCESS);
    assert!(stdout.contains("explicit local trust"));
    assert!(stderr.is_empty());

    let identity = identity();
    let (code, stdout, stderr) = invoke_manage(
        &[
            "memory-manage",
            "write",
            "--repository-id",
            &identity,
            "--input",
            "private-input.yaml",
            "private-repository",
        ],
        &manager,
    );
    assert_eq!(code, EXIT_SOFTWARE);
    assert!(stdout.is_empty());
    assert_eq!(stderr, "error: memory management failed\n");
    assert!(!stderr.contains("private"));
    assert!(!stderr.contains(&identity));
}

#[test]
fn memory_manage_rejects_invalid_receipt_revisions_without_json_injection() {
    let identity = identity();
    let cases = [
        (
            CliMemoryManageReport::Write {
                revision: "\"}\n{\"injected\":true".to_owned(),
                created: true,
                canonical_bytes: 1,
                publication: CliMemoryPublicationStatus::confirmed_for_test(),
            },
            vec![
                "memory-manage",
                "write",
                "--repository-id",
                identity.as_str(),
                "--input",
                "input.yaml",
                "repository",
            ],
        ),
        (
            CliMemoryManageReport::Approve {
                revision: "AA".repeat(32),
                version_inserted: true,
                observation_inserted: true,
                approval_inserted: true,
                maintenance: CliMemoryMaintenanceStatus::confirmed_for_test(),
            },
            vec![
                "memory-manage",
                "approve",
                "--repository-id",
                identity.as_str(),
                "--database",
                "index.db",
                "--record-id",
                "mem_00000000000000000000000000",
                "--actor",
                "local-user",
                "repository",
            ],
        ),
    ];

    for (report, arguments) in cases {
        let manager = RecordingManager::success(report);
        let (code, stdout, stderr) = invoke_manage(&arguments, &manager);
        assert_eq!(code, EXIT_SOFTWARE);
        assert!(stdout.is_empty());
        assert_eq!(stderr, "error: memory management failed\n");
    }
}

struct FailingWriter;

impl Write for FailingWriter {
    fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
        Err(io::Error::other("injected output failure"))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
