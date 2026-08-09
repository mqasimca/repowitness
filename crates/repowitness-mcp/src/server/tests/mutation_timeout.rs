use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use rmcp::{ServiceExt, model::CallToolRequestParams};
use tokio::time::Instant;

use super::*;
use crate::MemoryMutationOperation;

#[derive(Clone, Copy)]
enum MutationResult {
    DeferredReceipt,
    ExactUnknown,
}

struct TimedMutationService {
    calls: AtomicUsize,
    delay: Duration,
    result: MutationResult,
}

impl TimedMutationService {
    const fn new(delay: Duration, result: MutationResult) -> Self {
        Self {
            calls: AtomicUsize::new(0),
            delay,
            result,
        }
    }
}

impl RepositoryService for TimedMutationService {
    fn code_search(
        &self,
        _request: CodeSearchServiceRequest,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<CodeSearchOutput, RepositoryServiceError> {
        Err(RepositoryServiceError::CodeSearch)
    }

    fn context_build(
        &self,
        _request: EvidenceContextBuildServiceRequest,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<EvidenceContextBuildOutput, RepositoryServiceError> {
        Err(RepositoryServiceError::ContextBuild)
    }

    fn diagnostics(
        &self,
        _request: DiagnosticsServiceRequest,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<DiagnosticsOutput, RepositoryServiceError> {
        Err(RepositoryServiceError::Diagnostics)
    }

    fn memory_recall(
        &self,
        _request: MemoryRecallServiceRequest,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<MemoryRecallOutput, RepositoryServiceError> {
        Err(RepositoryServiceError::MemoryRecall)
    }

    fn memory_manage(
        &self,
        _request: MemoryManageServiceRequest,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<MemoryManageOutput, RepositoryServiceError> {
        self.calls.fetch_add(1, Ordering::AcqRel);
        std::thread::sleep(self.delay);
        match self.result {
            MutationResult::DeferredReceipt => Ok(MemoryManageOutput::approve_with_maintenance(
                "11".repeat(32),
                true,
                true,
                true,
                checkpoint_deferred_memory_maintenance(),
            )),
            MutationResult::ExactUnknown => {
                Err(RepositoryServiceError::memory_mutation_outcome_unknown(
                    MemoryMutationRequestScope::Approve,
                    MemoryMutationOperation::StoreStartup,
                ))
            }
        }
    }

    fn symbol_get(
        &self,
        _request: SymbolGetServiceRequest,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<SymbolGetOutput, RepositoryServiceError> {
        Err(RepositoryServiceError::SymbolGet)
    }
}

async fn call_timed_approval(
    service: Arc<TimedMutationService>,
) -> (CallToolResult, tokio::task::JoinHandle<()>) {
    let (server_transport, client_transport) = tokio::io::duplex(32 * 1024);
    let server = RepoWitnessMcpServer::with_memory_writes(service);
    let server_task = tokio::spawn(async move {
        server
            .serve(server_transport)
            .await
            .expect("server starts")
            .waiting()
            .await
            .expect("server stops");
    });
    let client = ().serve(client_transport).await.expect("client starts");
    let response = client
        .call_tool(
            CallToolRequestParams::new(MEMORY_MANAGE_TOOL_NAME).with_arguments(json_object(
                serde_json::json!({
                    "operation": "approve",
                    "record_id": "mem_00000000000000000000000000",
                    "timeout_ms": 10,
                }),
            )),
        )
        .await
        .expect("mutation returns a tool result");
    client.cancel().await.expect("client closes");
    (response, server_task)
}

#[tokio::test]
async fn mutation_receipt_inside_resolution_grace_is_preserved_without_retry() {
    let service = Arc::new(TimedMutationService::new(
        Duration::from_millis(50),
        MutationResult::DeferredReceipt,
    ));
    let (response, server_task) = call_timed_approval(Arc::clone(&service)).await;
    assert_eq!(response.is_error, Some(false));
    assert_eq!(
        response
            .structured_content
            .as_ref()
            .and_then(|value| value.get("schema_version"))
            .and_then(serde_json::Value::as_u64),
        Some(u64::from(MEMORY_MANAGE_SCHEMA_VERSION))
    );
    let maintenance = response
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/receipt/maintenance"))
        .expect("maintenance evidence");
    assert_eq!(maintenance["complete"], false);
    assert_eq!(maintenance["warning_count"], 1);
    assert_eq!(maintenance["checkpoint"], "deferred");
    assert_eq!(maintenance["shutdown"], "complete");
    assert_eq!(maintenance["database_identity"], "confirmed_at_final_fence");
    assert_eq!(service.calls.load(Ordering::Acquire), 1);
    server_task.await.expect("server task");
}

#[tokio::test]
async fn no_receipt_after_grace_is_unknown_phase_redacted_and_never_retried() {
    let service = Arc::new(TimedMutationService::new(
        Duration::from_millis(800),
        MutationResult::DeferredReceipt,
    ));
    let started = Instant::now();
    let (response, server_task) = call_timed_approval(Arc::clone(&service)).await;
    assert_eq!(response.is_error, Some(true));
    let message = &response.content[0].as_text().expect("text error").text;
    assert!(message.contains("request_scope=approve"));
    assert!(message.contains("operation=unknown_phase"));
    assert!(message.contains("reconciliation_required_before_retry="));
    assert!(message.ends_with("automatic_retry=false"));
    assert!(!message.contains("mem_"));
    assert!(started.elapsed() < Duration::from_millis(650));
    assert_eq!(service.calls.load(Ordering::Acquire), 1);
    server_task.await.expect("server task");
}

#[tokio::test]
async fn exact_unknown_returned_during_grace_is_not_relabelled_or_retried() {
    let service = Arc::new(TimedMutationService::new(
        Duration::from_millis(50),
        MutationResult::ExactUnknown,
    ));
    let (response, server_task) = call_timed_approval(Arc::clone(&service)).await;
    assert_eq!(response.is_error, Some(true));
    let message = &response.content[0].as_text().expect("text error").text;
    assert!(message.contains("request_scope=approve"));
    assert!(message.contains("operation=store_startup"));
    assert!(!message.contains("operation=unknown_phase"));
    assert!(message.contains("read-only database diagnostics"));
    assert_eq!(service.calls.load(Ordering::Acquire), 1);
    server_task.await.expect("server task");
}

#[test]
fn supervisor_margin_exceeds_worker_grace_and_cancellation_uses_a_fresh_bound() {
    let now = Instant::now();
    let operation_deadline = now + Duration::from_secs(30);
    let deadline_resolution = mutation_resolution_deadline(operation_deadline);
    assert!(deadline_resolution > operation_deadline + MCP_MUTATION_RECEIPT_RESOLUTION_GRACE);
    assert!(mutation_resolution_deadline_from_now() < operation_deadline);
}

#[tokio::test]
async fn early_cancellation_wait_for_a_stalled_task_is_one_fixed_grace() {
    let calls = Arc::new(AtomicUsize::new(0));
    let task_calls = Arc::clone(&calls);
    let task = tokio::task::spawn_blocking(move || {
        task_calls.fetch_add(1, Ordering::AcqRel);
        std::thread::sleep(Duration::from_millis(800));
        Ok(MemoryManageOutput::review_with_maintenance(
            true,
            confirmed_memory_maintenance(),
        ))
    });
    let started = Instant::now();
    let outcome = await_mutation_outcome(
        task,
        MemoryMutationRequestScope::Review,
        mutation_resolution_deadline_from_now(),
    )
    .await
    .expect("supervisor returns a service outcome");
    assert!(matches!(
        outcome,
        Err(RepositoryServiceError::MemoryMutationOutcomeUnknown {
            request_scope: MemoryMutationRequestScope::Review,
            operation: MemoryMutationOperation::UnknownPhase,
        })
    ));
    assert!(started.elapsed() < Duration::from_millis(650));
    assert_eq!(calls.load(Ordering::Acquire), 1);
}
