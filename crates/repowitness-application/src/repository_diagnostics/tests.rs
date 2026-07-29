use std::{
    convert::Infallible,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use super::*;

struct FakePort {
    output: RepositoryDiagnosticsPortResult<u64, u64>,
    cancel_during_call: bool,
}

impl RepositoryDiagnosticsPort for FakePort {
    type Error = Infallible;
    type Generation = u64;
    type Projection = u64;

    fn diagnose(
        &self,
        _repository: RepositoryIdentityDigest,
        cancelled: Arc<AtomicBool>,
        _deadline: Instant,
    ) -> Result<RepositoryDiagnosticsPortResult<u64, u64>, Self::Error> {
        if self.cancel_during_call {
            cancelled.store(true, Ordering::Release);
        }
        Ok(RepositoryDiagnosticsPortResult::new(
            self.output.snapshot,
            self.output.generation,
            self.output.source_epoch,
            self.output.producer_manifest,
            self.output.index_coverage,
            self.output.parser_diagnostics,
            self.output.memory_projection,
        ))
    }
}

fn digest(byte: u8) -> SourceSnapshotDigest {
    SourceSnapshotDigest::new([byte; 32])
}

fn repository() -> RepositoryIdentityDigest {
    RepositoryIdentityDigest::new([3; 32])
}

fn producer() -> ProducerManifestDigest {
    ProducerManifestDigest::new([4; 32])
}

fn valid_coverage() -> MemoryRecallProjectionCoverage {
    MemoryRecallProjectionCoverage::new(3, 1, 1, 0, 3, 1, 0, 1, 1, 0, 0, 0, 0, 0, 0)
}

fn output(
    memory: Option<RepositoryDiagnosticsMemoryProjection<u64>>,
) -> RepositoryDiagnosticsPortResult<u64, u64> {
    RepositoryDiagnosticsPortResult::new(
        digest(5),
        7,
        11,
        producer(),
        RustIndexCoverage::new(9, 2, 1, 0),
        RepositoryParserDiagnostics::new(7, 2),
        memory,
    )
}

fn request(cancelled: Arc<AtomicBool>, deadline: Instant) -> RepositoryDiagnosticsRequest {
    RepositoryDiagnosticsRequest::new(repository(), cancelled, deadline)
}

#[test]
fn complete_state_preserves_evidence_coverage_and_static_capabilities() {
    let memory = RepositoryDiagnosticsMemoryProjection::new(13, 11, digest(5), valid_coverage());
    let port = FakePort {
        output: output(Some(memory)),
        cancel_during_call: false,
    };
    let result = repository_diagnostics(
        &port,
        request(
            Arc::new(AtomicBool::new(false)),
            Instant::now() + Duration::from_secs(1),
        ),
    )
    .expect("valid diagnostics");

    assert_eq!(result.snapshot(), digest(5));
    assert_eq!(*result.generation(), 7);
    assert_eq!(result.source_epoch(), 11);
    assert_eq!(result.producer_manifest(), producer());
    assert_eq!(result.index_coverage(), RustIndexCoverage::new(9, 2, 1, 0));
    assert_eq!(result.syntax_error_nodes(), 7);
    assert_eq!(result.known_parser_limitation_nodes(), 2);
    assert_eq!(result.memory_projection(), Some(&memory));
    assert_eq!(
        result
            .supported_languages()
            .iter()
            .map(|language| language.as_str())
            .collect::<Vec<_>>(),
        ["rust", "go", "typescript", "tsx", "python"]
    );
    assert_eq!(
        result
            .capabilities()
            .iter()
            .map(|capability| capability.as_str())
            .collect::<Vec<_>>(),
        [
            "lexical_source_search",
            "exact_symbol_source",
            "bounded_rust_syntax_graph",
            "current_memory_recall",
            "bounded_context_build"
        ]
    );
    assert_eq!(
        result
            .limitations()
            .iter()
            .map(|limitation| limitation.as_str())
            .collect::<Vec<_>>(),
        [
            "rust_graph_syntax_derived_only",
            "no_package_macro_scip_dynamic_or_cross_language_graph",
            "no_history_search",
            "no_vector_retrieval",
            "no_model_tokenizer",
            "no_remote_transport",
        ]
    );
    assert_eq!(result.profile_version(), 3);
}

#[test]
fn absent_memory_projection_is_a_valid_inspectable_state() {
    let port = FakePort {
        output: output(None),
        cancel_during_call: false,
    };
    let result = repository_diagnostics(
        &port,
        request(
            Arc::new(AtomicBool::new(false)),
            Instant::now() + Duration::from_secs(1),
        ),
    )
    .expect("source-only diagnostics");
    assert_eq!(result.memory_projection(), None);
}

#[test]
fn mixed_source_or_invalid_projection_coverage_fails_closed() {
    for memory in [
        RepositoryDiagnosticsMemoryProjection::new(13, 12, digest(5), valid_coverage()),
        RepositoryDiagnosticsMemoryProjection::new(13, 11, digest(6), valid_coverage()),
        RepositoryDiagnosticsMemoryProjection::new(
            13,
            11,
            digest(5),
            MemoryRecallProjectionCoverage::new(3, 0, 0, 0, 3, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0),
        ),
    ] {
        let port = FakePort {
            output: output(Some(memory)),
            cancel_during_call: false,
        };
        let error = repository_diagnostics(
            &port,
            request(
                Arc::new(AtomicBool::new(false)),
                Instant::now() + Duration::from_secs(1),
            ),
        )
        .expect_err("invalid adapter state must fail");
        assert!(matches!(
            error,
            RepositoryDiagnosticsError::InvalidPortOutput(_)
        ));
    }
}

#[test]
fn known_parser_limitations_must_be_a_subset_of_raw_syntax_errors() {
    let mut invalid = output(None);
    invalid.parser_diagnostics = RepositoryParserDiagnostics::new(1, 2);
    let port = FakePort {
        output: invalid,
        cancel_during_call: false,
    };
    let error = repository_diagnostics(
        &port,
        request(
            Arc::new(AtomicBool::new(false)),
            Instant::now() + Duration::from_secs(1),
        ),
    )
    .expect_err("invalid parser diagnostics must fail closed");
    assert!(matches!(
        error,
        RepositoryDiagnosticsError::InvalidPortOutput(
            RepositoryDiagnosticsPortOutputError::InvalidParserDiagnostics
        )
    ));
}

#[test]
fn cancellation_and_deadline_are_checked_before_and_after_the_port() {
    let port = FakePort {
        output: output(None),
        cancel_during_call: false,
    };
    let error = repository_diagnostics(
        &port,
        request(
            Arc::new(AtomicBool::new(true)),
            Instant::now() + Duration::from_secs(1),
        ),
    )
    .expect_err("cancelled request");
    assert!(matches!(error, RepositoryDiagnosticsError::Cancelled));

    let error = repository_diagnostics(
        &port,
        request(
            Arc::new(AtomicBool::new(false)),
            Instant::now() - Duration::from_millis(1),
        ),
    )
    .expect_err("expired request");
    assert!(matches!(
        error,
        RepositoryDiagnosticsError::DeadlineExceeded
    ));

    let port = FakePort {
        output: output(None),
        cancel_during_call: true,
    };
    let error = repository_diagnostics(
        &port,
        request(
            Arc::new(AtomicBool::new(false)),
            Instant::now() + Duration::from_secs(1),
        ),
    )
    .expect_err("port-side cancellation");
    assert!(matches!(error, RepositoryDiagnosticsError::Cancelled));
}
