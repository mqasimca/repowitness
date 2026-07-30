use super::*;

#[test]
fn outcome_unknown_is_attributed_redacted_and_never_retried() {
    let identity = identity();
    let revision = "44".repeat(32);
    let artifact = "55".repeat(32);
    let cases = [
        (
            MemoryMutationRequestScope::Approve,
            MemoryMutationOperation::StoreStartup,
            vec![
                "memory-manage",
                "approve",
                "--repository-id",
                identity.as_str(),
                "--database",
                "private-approval.db",
                "--record-id",
                "mem_00000000000000000000000000",
                "--actor",
                "private-approval-actor",
                "private-approval-repository",
            ],
        ),
        (
            MemoryMutationRequestScope::Review,
            MemoryMutationOperation::CorrespondenceReview,
            vec![
                "memory-manage",
                "review",
                "--repository-id",
                identity.as_str(),
                "--database",
                "private-review.db",
                "--record-id",
                "mem_00000000000000000000000000",
                "--revision",
                revision.as_str(),
                "--evidence",
                "0",
                "--operation",
                "approve",
                "--target-path",
                "rwp1:h:7372632F6C69622E7273",
                "--target-artifact",
                artifact.as_str(),
                "--target-fact",
                "0",
                "--actor",
                "private-review-actor",
                "private-review-repository",
            ],
        ),
        (
            MemoryMutationRequestScope::ImportHistory,
            MemoryMutationOperation::HistoryImport,
            vec![
                "memory-manage",
                "import-history",
                "--repository-id",
                identity.as_str(),
                "--database",
                "private-history.db",
                "--actor",
                "private-history-actor",
                "private-history-repository",
            ],
        ),
    ];

    for (request_scope, operation, arguments) in cases {
        let manager = RecordingManager::outcome_unknown(request_scope, operation);
        let (code, stdout, stderr) = invoke_manage(&arguments, &manager);
        assert_eq!(code, EXIT_SOFTWARE);
        assert!(stdout.is_empty());
        assert!(stderr.contains(&format!(
            "request_scope={}\noperation={}\n",
            request_scope.as_str(),
            operation.as_str()
        )));
        assert!(stderr.contains("reconciliation_required_before_retry="));
        assert!(stderr.ends_with("automatic_retry=false\n"));
        assert!(!stderr.contains("private"));
        assert!(!stderr.contains(&identity));
        assert!(!stderr.contains(&revision));
        assert!(!stderr.contains(&artifact));
        assert_eq!(manager.calls.get(), 1);
    }
}

#[test]
fn local_and_mcp_unknown_mapping_preserves_exact_management_phase() {
    for (request_scope, local, expected) in [
        (
            MemoryMutationRequestScope::Approve,
            LocalMemoryMutation::StoreStartup,
            MemoryMutationOperation::StoreStartup,
        ),
        (
            MemoryMutationRequestScope::Approve,
            LocalMemoryMutation::Approval,
            MemoryMutationOperation::Approval,
        ),
        (
            MemoryMutationRequestScope::Approve,
            LocalMemoryMutation::Checkpoint,
            MemoryMutationOperation::Checkpoint,
        ),
        (
            MemoryMutationRequestScope::Review,
            LocalMemoryMutation::StoreStartup,
            MemoryMutationOperation::StoreStartup,
        ),
        (
            MemoryMutationRequestScope::Review,
            LocalMemoryMutation::CorrespondenceReview,
            MemoryMutationOperation::CorrespondenceReview,
        ),
        (
            MemoryMutationRequestScope::Review,
            LocalMemoryMutation::Checkpoint,
            MemoryMutationOperation::Checkpoint,
        ),
        (
            MemoryMutationRequestScope::ImportHistory,
            LocalMemoryMutation::StoreStartup,
            MemoryMutationOperation::StoreStartup,
        ),
        (
            MemoryMutationRequestScope::ImportHistory,
            LocalMemoryMutation::HistoryImport,
            MemoryMutationOperation::HistoryImport,
        ),
        (
            MemoryMutationRequestScope::ImportHistory,
            LocalMemoryMutation::Checkpoint,
            MemoryMutationOperation::Checkpoint,
        ),
    ] {
        let local_error = LocalMemoryManageError::MutationOutcomeUnknown { operation: local };
        assert_eq!(
            CliMemoryError::from_management(request_scope, local_error),
            CliMemoryError::MutationOutcomeUnknown {
                request_scope,
                operation: expected,
            }
        );
        assert_eq!(
            mcp_memory_manage_error(request_scope, local_error).memory_mutation_attribution(),
            Some((request_scope, expected))
        );
    }
}

#[test]
fn undelivered_success_receipts_require_reconciliation_and_never_look_retryable() {
    let cases = [
        (
            CliMemoryManageReport::Write {
                revision: "33".repeat(32),
                created: false,
                canonical_bytes: 1,
                publication: CliMemoryPublicationStatus::confirmed_for_test(),
            },
            MemoryMutationRequestScope::Write,
            MemoryMutationOperation::CanonicalWrite,
        ),
        (
            CliMemoryManageReport::Approve {
                revision: "44".repeat(32),
                version_inserted: true,
                observation_inserted: true,
                approval_inserted: true,
                maintenance: CliMemoryMaintenanceStatus::confirmed_for_test(),
            },
            MemoryMutationRequestScope::Approve,
            MemoryMutationOperation::Approval,
        ),
        (
            CliMemoryManageReport::Review {
                inserted: true,
                maintenance: CliMemoryMaintenanceStatus::checkpoint_deferred_for_test(),
            },
            MemoryMutationRequestScope::Review,
            MemoryMutationOperation::CorrespondenceReview,
        ),
        (
            CliMemoryManageReport::ImportHistory {
                commits_inspected: 2,
                records_inspected: 3,
                imported_versions: 1,
                appended_observations: 1,
                total_record_bytes: 128,
                git_processes: 2,
                history_complete: true,
                maintenance: CliMemoryMaintenanceStatus::shutdown_deferred_for_test(),
            },
            MemoryMutationRequestScope::ImportHistory,
            MemoryMutationOperation::HistoryImport,
        ),
    ];

    for (report, request_scope, operation) in cases {
        let mut stderr = Vec::new();
        assert_eq!(
            emit_memory_manage_report(&mut FailingWriter, &mut stderr, report),
            EXIT_IO
        );
        let stderr = String::from_utf8(stderr).expect("diagnostic should be UTF-8");
        assert!(stderr.starts_with("error: committed memory receipt could not be written\n"));
        assert!(stderr.contains(&format!(
            "request_scope={}\noperation={}\n",
            request_scope.as_str(),
            operation.as_str()
        )));
        assert!(stderr.contains(operation.reconciliation_guidance()));
        assert!(stderr.ends_with("automatic_retry=false\n"));
        assert!(!stderr.contains("33"));
        assert!(!stderr.contains("44"));
    }
}
