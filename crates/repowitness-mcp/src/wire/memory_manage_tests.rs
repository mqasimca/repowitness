use super::*;
use crate::MemoryMutationOperation;

const RECORD_ID: &str = "mem_00000000000000000000000000";

#[test]
fn operations_are_exact_bounded_and_redacted() {
    let write: MemoryManageInput = serde_json::from_value(serde_json::json!({
        "operation": "write",
        "record_yaml": "private memory yaml",
        "timeout_ms": 100
    }))
    .expect("write input");
    assert!(!format!("{write:?}").contains("private"));
    let write = write.validate().expect("valid write");
    assert_eq!(
        write.mutation_request_scope(),
        MemoryMutationRequestScope::Write
    );
    assert!(matches!(write, MemoryManageServiceRequest::Write { .. }));

    let approve: MemoryManageInput = serde_json::from_value(serde_json::json!({
        "operation": "approve",
        "record_id": RECORD_ID
    }))
    .expect("approval input");
    let approve = approve.validate().expect("valid approval");
    assert_eq!(
        approve.mutation_request_scope(),
        MemoryMutationRequestScope::Approve
    );
    assert!(matches!(
        approve,
        MemoryManageServiceRequest::Approve { .. }
    ));

    let review: MemoryManageInput = serde_json::from_value(serde_json::json!({
        "operation": "review",
        "record_id": RECORD_ID,
        "revision_sha256": "11".repeat(32),
        "evidence_ordinal": 15,
        "review_decision": "manual_link",
        "target_path": "rwp1:h:7372632F6C69622E7273",
        "target_artifact_sha256": "22".repeat(32),
        "target_fact_ordinal": MAX_MCP_INTEROPERABLE_INTEGER,
    }))
    .expect("review input");
    let request = review.validate().expect("valid review");
    assert!(!format!("{request:?}").contains("737263"));
    assert_eq!(
        request.mutation_request_scope(),
        MemoryMutationRequestScope::Review
    );
    assert!(matches!(
        request,
        MemoryManageServiceRequest::Review {
            decision: MemoryManageReviewDecision::ManualLink,
            ..
        }
    ));

    let history: MemoryManageInput =
        serde_json::from_value(serde_json::json!({"operation": "import_history"}))
            .expect("history input");
    let history = history.validate().expect("valid history");
    assert_eq!(
        history.mutation_request_scope(),
        MemoryMutationRequestScope::ImportHistory
    );
    assert!(matches!(
        history,
        MemoryManageServiceRequest::ImportHistory { .. }
    ));
}

#[test]
fn invalid_combinations_and_unknown_fields_fail_closed() {
    assert!(
        serde_json::from_value::<MemoryManageInput>(serde_json::json!({
            "operation": "approve",
            "record_id": RECORD_ID,
            "actor": "untrusted-caller"
        }),)
        .is_err()
    );
    for value in [
        serde_json::json!({
            "operation": "write",
            "record_yaml": "",
        }),
        serde_json::json!({
            "operation": "approve",
            "record_id": RECORD_ID,
            "record_yaml": "not allowed",
        }),
        serde_json::json!({
            "operation": "review",
            "record_id": RECORD_ID,
            "revision_sha256": "11".repeat(32),
            "evidence_ordinal": 16,
            "review_decision": "approve",
            "target_path": "rwp1:h:7372632F6C69622E7273",
            "target_artifact_sha256": "22".repeat(32),
            "target_fact_ordinal": 0,
        }),
        serde_json::json!({
            "operation": "import_history",
            "record_id": RECORD_ID,
        }),
        serde_json::json!({
            "operation": "review",
            "record_id": RECORD_ID,
            "revision_sha256": "11".repeat(32),
            "evidence_ordinal": 0,
            "review_decision": "approve",
            "target_path": "rwp1:h:7372632F6C69622E7273",
            "target_artifact_sha256": "22".repeat(32),
            "target_fact_ordinal": MAX_MCP_INTEROPERABLE_INTEGER + 1,
        }),
    ] {
        let input: MemoryManageInput = serde_json::from_value(value).expect("wire shape");
        assert!(input.validate().is_err());
    }
}

#[test]
fn review_rejects_encoded_invalid_repository_paths() {
    for target_path in [
        "rwp1:h:00",
        "rwp1:h:2F737263",
        "rwp1:h:7372632F2E2E2F6C69622E7273",
        "rwp1:h:7372632F2E6769742F636F6E666967",
    ] {
        let input: MemoryManageInput = serde_json::from_value(serde_json::json!({
            "operation": "review",
            "record_id": RECORD_ID,
            "revision_sha256": "11".repeat(32),
            "evidence_ordinal": 0,
            "review_decision": "approve",
            "target_path": target_path,
            "target_artifact_sha256": "22".repeat(32),
            "target_fact_ordinal": 0,
        }))
        .expect("wire shape");
        assert!(input.validate().is_err());
    }
}

#[test]
fn inline_yaml_byte_limit_is_inclusive() {
    let exact: MemoryManageInput = serde_json::from_value(serde_json::json!({
        "operation": "write",
        "record_yaml": "x".repeat(MAX_INLINE_MEMORY_YAML_BYTES),
    }))
    .expect("wire shape");
    assert!(exact.validate().is_ok());

    let oversized: MemoryManageInput = serde_json::from_value(serde_json::json!({
        "operation": "write",
        "record_yaml": "x".repeat(MAX_INLINE_MEMORY_YAML_BYTES + 1),
    }))
    .expect("wire shape");
    assert!(oversized.validate().is_err());
}

#[test]
fn write_receipt_preserves_post_commit_warnings_without_paths() {
    assert_eq!(MEMORY_MANAGE_SCHEMA_VERSION, 2);
    let output = MemoryManageOutput::write_with_publication(
        "11".repeat(32),
        true,
        12,
        MemoryManagePublicationStatus {
            complete: false,
            warning_count: 2,
            temporary_cleanup: MemoryManagePublicationStepStatus::Deferred,
            target_identity: MemoryManageFileIdentityStatus::ChangedAfterCommit,
            records_directory_identity: MemoryManageFileIdentityStatus::ConfirmedAtFinalFence,
            directory_sync: MemoryManagePublicationStepStatus::Complete,
        },
    );
    let value = serde_json::to_value(output).expect("output serializes");
    assert_eq!(
        value["schema_version"],
        serde_json::json!(MEMORY_MANAGE_SCHEMA_VERSION)
    );
    let publication = &value["receipt"]["publication"];
    assert_eq!(publication["complete"], false);
    assert_eq!(publication["warning_count"], 2);
    assert_eq!(publication["target_identity"], "changed_after_commit");
    assert!(!value.to_string().contains('/'));
}

#[test]
fn database_receipts_preserve_deferred_maintenance_without_sensitive_data() {
    let outputs = [
        MemoryManageOutput::approve_with_maintenance(
            "11".repeat(32),
            true,
            true,
            true,
            MemoryManageMaintenanceStatus::from_evidence(
                MemoryManageMaintenanceStepStatus::Deferred,
                MemoryManageMaintenanceStepStatus::Complete,
                MemoryManageDatabaseIdentityStatus::ConfirmedAtFinalFence,
            ),
        ),
        MemoryManageOutput::review_with_maintenance(
            true,
            MemoryManageMaintenanceStatus::from_evidence(
                MemoryManageMaintenanceStepStatus::Complete,
                MemoryManageMaintenanceStepStatus::Deferred,
                MemoryManageDatabaseIdentityStatus::ConfirmedAtFinalFence,
            ),
        ),
        MemoryManageOutput::import_history_with_maintenance(
            1,
            1,
            1,
            1,
            128,
            2,
            true,
            MemoryManageMaintenanceStatus::from_evidence(
                MemoryManageMaintenanceStepStatus::Deferred,
                MemoryManageMaintenanceStepStatus::Deferred,
                MemoryManageDatabaseIdentityStatus::ConfirmedAtFinalFence,
            ),
        ),
        MemoryManageOutput::review_with_maintenance(
            true,
            MemoryManageMaintenanceStatus::from_evidence(
                MemoryManageMaintenanceStepStatus::Complete,
                MemoryManageMaintenanceStepStatus::Complete,
                MemoryManageDatabaseIdentityStatus::ChangedAfterCommit,
            ),
        ),
        MemoryManageOutput::review_with_maintenance(
            true,
            MemoryManageMaintenanceStatus::from_evidence(
                MemoryManageMaintenanceStepStatus::Complete,
                MemoryManageMaintenanceStepStatus::Complete,
                MemoryManageDatabaseIdentityStatus::Unconfirmed,
            ),
        ),
    ];
    let expected = [
        serde_json::json!({
            "complete": false,
            "warning_count": 1,
            "checkpoint": "deferred",
            "shutdown": "complete",
            "database_identity": "confirmed_at_final_fence",
        }),
        serde_json::json!({
            "complete": false,
            "warning_count": 1,
            "checkpoint": "complete",
            "shutdown": "deferred",
            "database_identity": "confirmed_at_final_fence",
        }),
        serde_json::json!({
            "complete": false,
            "warning_count": 2,
            "checkpoint": "deferred",
            "shutdown": "deferred",
            "database_identity": "confirmed_at_final_fence",
        }),
        serde_json::json!({
            "complete": false,
            "warning_count": 1,
            "checkpoint": "complete",
            "shutdown": "complete",
            "database_identity": "changed_after_commit",
        }),
        serde_json::json!({
            "complete": false,
            "warning_count": 1,
            "checkpoint": "complete",
            "shutdown": "complete",
            "database_identity": "unconfirmed",
        }),
    ];

    for (output, expected) in outputs.into_iter().zip(expected) {
        let value = serde_json::to_value(output).expect("output serializes");
        assert_eq!(
            value["schema_version"],
            serde_json::json!(MEMORY_MANAGE_SCHEMA_VERSION)
        );
        assert_eq!(value["receipt"]["maintenance"], expected);
        assert!(!value.to_string().contains("private"));
        assert!(!value.to_string().contains('/'));
    }
}

#[test]
fn outcome_unknown_attribution_is_exact_bounded_and_redacted() {
    let exact = crate::RepositoryServiceError::memory_mutation_outcome_unknown(
        MemoryMutationRequestScope::Approve,
        MemoryMutationOperation::StoreStartup,
    );
    assert_eq!(
        exact.memory_mutation_attribution(),
        Some((
            MemoryMutationRequestScope::Approve,
            MemoryMutationOperation::StoreStartup,
        ))
    );
    let exact_message = exact.to_string();
    assert!(exact_message.contains("request_scope=approve"));
    assert!(exact_message.contains("operation=store_startup"));
    assert!(exact_message.contains("read-only database diagnostics"));

    let lost = crate::RepositoryServiceError::memory_mutation_phase_unknown(
        MemoryMutationRequestScope::Review,
    );
    assert_eq!(
        lost.memory_mutation_attribution(),
        Some((
            MemoryMutationRequestScope::Review,
            MemoryMutationOperation::UnknownPhase,
        ))
    );
    let lost_message = lost.to_string();
    assert!(lost_message.contains("request_scope=review"));
    assert!(lost_message.contains("operation=unknown_phase"));
    assert!(lost_message.contains("reconciliation_required_before_retry="));
    assert!(lost_message.ends_with("automatic_retry=false"));

    for message in [exact_message, lost_message] {
        assert!(!message.contains("private"));
        assert!(!message.contains("actor"));
        assert!(!message.contains('/'));
        assert!(message.len() < 512);
    }
}
