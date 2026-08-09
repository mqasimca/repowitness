use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use super::*;

pub(super) struct CancellationService {
    pub(super) started: AtomicBool,
    pub(super) observed: AtomicBool,
}

impl RepositoryService for CancellationService {
    fn code_search(
        &self,
        _request: CodeSearchServiceRequest,
        cancelled: Arc<AtomicBool>,
    ) -> Result<CodeSearchOutput, RepositoryServiceError> {
        self.started.store(true, Ordering::Release);
        while !cancelled.load(Ordering::Acquire) {
            std::thread::sleep(Duration::from_millis(1));
        }
        self.observed.store(true, Ordering::Release);
        Err(RepositoryServiceError::CodeSearch)
    }

    fn symbol_get(
        &self,
        _request: SymbolGetServiceRequest,
        _cancelled: Arc<AtomicBool>,
    ) -> Result<SymbolGetOutput, RepositoryServiceError> {
        Err(RepositoryServiceError::SymbolGet)
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
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn protocol_cancellation_reaches_blocking_work_and_suppresses_its_response() {
    let service = Arc::new(CancellationService {
        started: AtomicBool::new(false),
        observed: AtomicBool::new(false),
    });
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let server = RepoWitnessMcpServer::new(service.clone());
    let server_task = tokio::spawn(async move {
        server
            .serve(server_transport)
            .await
            .expect("server starts")
            .waiting()
            .await
            .expect("server stops")
    });
    let (client_read, mut client_write) = tokio::io::split(client_transport);
    let mut client_read = BufReader::new(client_read);

    send_json(
        &mut client_write,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": {"name": "cancellation-test", "version": "1"}
            }
        }),
    )
    .await;
    assert_eq!(
        read_json(&mut client_read).await["id"],
        serde_json::json!(1)
    );
    send_json(
        &mut client_write,
        serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        }),
    )
    .await;
    send_json(
        &mut client_write,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "code_search",
                "arguments": {"query": "run", "timeout_ms": 10000}
            }
        }),
    )
    .await;
    tokio::time::timeout(Duration::from_secs(1), async {
        while !service.started.load(Ordering::Acquire) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("service starts");
    send_json(
        &mut client_write,
        serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/cancelled",
            "params": {"requestId": 2, "reason": "test cancellation"}
        }),
    )
    .await;
    send_json(
        &mut client_write,
        serde_json::json!({"jsonrpc": "2.0", "id": 3, "method": "ping"}),
    )
    .await;

    let response = read_json(&mut client_read).await;
    assert_eq!(response["id"], serde_json::json!(3));
    tokio::time::timeout(Duration::from_secs(1), async {
        while !service.observed.load(Ordering::Acquire) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("blocking service observes cancellation");
    assert!(
        tokio::time::timeout(Duration::from_millis(50), read_json(&mut client_read))
            .await
            .is_err(),
        "cancelled request must not produce a response"
    );
    drop(client_write);
    drop(client_read);
    server_task.await.expect("server task");
}

#[tokio::test]
async fn stalled_read_cleanup_wait_is_bounded() {
    let task = tokio::task::spawn_blocking(|| {
        std::thread::sleep(Duration::from_millis(800));
        Err::<CodeSearchOutput, RepositoryServiceError>(RepositoryServiceError::CodeSearch)
    });
    let started = Instant::now();
    await_cancelled_task(task).await;
    assert!(started.elapsed() < Duration::from_millis(650));
}

async fn send_json<W: tokio::io::AsyncWrite + Unpin>(writer: &mut W, value: serde_json::Value) {
    let encoded = serde_json::to_vec(&value).expect("JSON encodes");
    writer.write_all(&encoded).await.expect("message writes");
    writer.write_all(b"\n").await.expect("delimiter writes");
    writer.flush().await.expect("message flushes");
}

async fn read_json<R: tokio::io::AsyncRead + Unpin>(
    reader: &mut BufReader<R>,
) -> serde_json::Value {
    let mut line = String::new();
    let bytes = reader.read_line(&mut line).await.expect("response reads");
    assert!(bytes > 0, "server closed before responding");
    serde_json::from_str(&line).expect("response is JSON")
}
