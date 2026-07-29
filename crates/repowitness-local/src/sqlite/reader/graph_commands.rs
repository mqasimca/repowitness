use repowitness_application::ResolvedConfiguration;
use repowitness_domain::ConfigurationDigest;

use crate::sqlite::{
    RustGraphDirection, RustGraphEdgeKinds, RustGraphReadError, RustGraphReadLimits,
    RustGraphTraceStart,
    graph::{
        RustGraphArchitectureSummary, RustGraphAvailability, RustGraphEvidenceResult,
        RustGraphImpactResult, RustGraphSiteSelector, RustGraphSymbolSearchResult,
        RustGraphTraceResult,
    },
};

const MAX_GRAPH_SYMBOL_QUERY_BYTES: usize = 16_384;

type GraphReply = SyncSender<Result<GraphCommandResult, RustGraphReadError>>;

enum GraphOperation {
    Status,
    SymbolSearch {
        query: String,
    },
    Evidence {
        site: RustGraphSiteSelector,
    },
    Architecture,
    Trace {
        start: Box<RustGraphTraceStart>,
        direction: RustGraphDirection,
        edge_kinds: RustGraphEdgeKinds,
    },
    Impact {
        start: Box<RustGraphDefinitionRecord>,
        edge_kinds: RustGraphEdgeKinds,
    },
}

impl GraphOperation {
    const fn label(&self) -> &'static str {
        match self {
            Self::Status => "status",
            Self::SymbolSearch { .. } => "symbol_search",
            Self::Evidence { .. } => "evidence",
            Self::Architecture => "architecture",
            Self::Trace { .. } => "trace",
            Self::Impact { .. } => "impact",
        }
    }
}

enum GraphCommandResult {
    Status(RustGraphAvailability),
    SymbolSearch(RustGraphSymbolSearchResult),
    Evidence(Box<Option<RustGraphEvidenceResult>>),
    Architecture(RustGraphArchitectureSummary),
    Trace(Box<RustGraphTraceResult>),
    Impact(Box<RustGraphImpactResult>),
}

struct GraphCommand {
    view: PinnedWorkspaceView,
    graph_generation: GenerationId,
    limits: Option<RustGraphReadLimits>,
    configuration_digest: Option<ConfigurationDigest>,
    operation: GraphOperation,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
    reply: GraphReply,
}

impl fmt::Debug for GraphCommand {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GraphCommand")
            .field("view", &self.view)
            .field("graph_generation", &self.graph_generation)
            .field("limits", &self.limits)
            .field("configuration_digest", &self.configuration_digest)
            .field("operation", &self.operation.label())
            .field("cancelled", &self.cancelled.load(Ordering::Acquire))
            .field("deadline", &"<monotonic>")
            .finish()
    }
}

impl OwnedSqliteReader {
    /// Reports categorical graph availability for one immutable pinned view.
    pub fn rust_graph_status(
        &self,
        view: &PinnedWorkspaceView,
        graph_generation: GenerationId,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<RustGraphAvailability, RustGraphReadError> {
        match self.execute_graph_read(
            view,
            graph_generation,
            None,
            None,
            GraphOperation::Status,
            cancelled,
            deadline,
        )? {
            GraphCommandResult::Status(status) => Ok(status),
            _ => Err(RustGraphReadError::CorruptGraph),
        }
    }

    /// Searches exact names and qualified names in one complete graph.
    #[allow(
        clippy::too_many_arguments,
        reason = "view, generation, bounds, cancellation, and deadline are explicit trust inputs"
    )]
    pub fn search_rust_graph_symbols(
        &self,
        view: &PinnedWorkspaceView,
        graph_generation: GenerationId,
        query: &str,
        limits: RustGraphReadLimits,
        configuration: Option<&ResolvedConfiguration>,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<RustGraphSymbolSearchResult, RustGraphReadError> {
        validate_graph_query(query)?;
        match self.execute_graph_read(
            view,
            graph_generation,
            Some(limits.constrained_by(configuration)),
            configuration.map(ResolvedConfiguration::digest),
            GraphOperation::SymbolSearch {
                query: query.to_owned(),
            },
            cancelled,
            deadline,
        )? {
            GraphCommandResult::SymbolSearch(result) => Ok(result),
            _ => Err(RustGraphReadError::CorruptGraph),
        }
    }

    /// Loads one exact graph site, categorical outcome, and retained candidates.
    #[allow(
        clippy::too_many_arguments,
        reason = "view, generation, bounds, cancellation, and deadline are explicit trust inputs"
    )]
    pub fn rust_graph_evidence(
        &self,
        view: &PinnedWorkspaceView,
        graph_generation: GenerationId,
        site: RustGraphSiteSelector,
        limits: RustGraphReadLimits,
        configuration: Option<&ResolvedConfiguration>,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<Option<RustGraphEvidenceResult>, RustGraphReadError> {
        validate_graph_site_selector(&site)?;
        match self.execute_graph_read(
            view,
            graph_generation,
            Some(limits.constrained_by(configuration)),
            configuration.map(ResolvedConfiguration::digest),
            GraphOperation::Evidence { site },
            cancelled,
            deadline,
        )? {
            GraphCommandResult::Evidence(result) => Ok(*result),
            _ => Err(RustGraphReadError::CorruptGraph),
        }
    }

    /// Summarizes exact declaration and unique-edge counts by stable kind.
    #[allow(
        clippy::too_many_arguments,
        reason = "view, generation, bounds, cancellation, and deadline are explicit trust inputs"
    )]
    pub fn rust_graph_architecture(
        &self,
        view: &PinnedWorkspaceView,
        graph_generation: GenerationId,
        limits: RustGraphReadLimits,
        configuration: Option<&ResolvedConfiguration>,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<RustGraphArchitectureSummary, RustGraphReadError> {
        match self.execute_graph_read(
            view,
            graph_generation,
            Some(limits.constrained_by(configuration)),
            configuration.map(ResolvedConfiguration::digest),
            GraphOperation::Architecture,
            cancelled,
            deadline,
        )? {
            GraphCommandResult::Architecture(result) => Ok(result),
            _ => Err(RustGraphReadError::CorruptGraph),
        }
    }

    /// Traverses retained unique and ambiguous Rust relationships from one exact start.
    #[allow(
        clippy::too_many_arguments,
        reason = "view, generation, semantics, bounds, cancellation, and deadline are explicit"
    )]
    pub fn trace_rust_graph(
        &self,
        view: &PinnedWorkspaceView,
        graph_generation: GenerationId,
        start: RustGraphTraceStart,
        direction: RustGraphDirection,
        edge_kinds: RustGraphEdgeKinds,
        limits: RustGraphReadLimits,
        configuration: Option<&ResolvedConfiguration>,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<RustGraphTraceResult, RustGraphReadError> {
        validate_graph_trace_start(&start)?;
        match self.execute_graph_read(
            view,
            graph_generation,
            Some(limits.constrained_by(configuration)),
            configuration.map(ResolvedConfiguration::digest),
            GraphOperation::Trace {
                start: Box::new(start),
                direction,
                edge_kinds,
            },
            cancelled,
            deadline,
        )? {
            GraphCommandResult::Trace(result) => Ok(*result),
            _ => Err(RustGraphReadError::CorruptGraph),
        }
    }

    /// Computes conservative inbound impact for one exact declaration.
    #[allow(
        clippy::too_many_arguments,
        reason = "view, generation, semantics, bounds, cancellation, and deadline are explicit"
    )]
    pub fn analyze_rust_graph_impact(
        &self,
        view: &PinnedWorkspaceView,
        graph_generation: GenerationId,
        start: RustGraphDefinitionRecord,
        edge_kinds: RustGraphEdgeKinds,
        limits: RustGraphReadLimits,
        configuration: Option<&ResolvedConfiguration>,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<RustGraphImpactResult, RustGraphReadError> {
        validate_graph_definition_selector(&start)?;
        match self.execute_graph_read(
            view,
            graph_generation,
            Some(limits.constrained_by(configuration)),
            configuration.map(ResolvedConfiguration::digest),
            GraphOperation::Impact {
                start: Box::new(start),
                edge_kinds,
            },
            cancelled,
            deadline,
        )? {
            GraphCommandResult::Impact(result) => Ok(*result),
            _ => Err(RustGraphReadError::CorruptGraph),
        }
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "this is the one internal graph-request boundary"
    )]
    fn execute_graph_read(
        &self,
        view: &PinnedWorkspaceView,
        graph_generation: GenerationId,
        limits: Option<RustGraphReadLimits>,
        configuration_digest: Option<ConfigurationDigest>,
        operation: GraphOperation,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<GraphCommandResult, RustGraphReadError> {
        graph_control(&cancelled, deadline)?;
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(
            ReaderCommand::Graph(Box::new(GraphCommand {
                view: view.clone(),
                graph_generation,
                limits,
                configuration_digest,
                operation,
                cancelled: Arc::clone(&cancelled),
                deadline,
                reply,
            })),
            deadline,
        )
        .map_err(map_graph_store_error)?;
        match receive_graph_reply(&receiver, deadline) {
            Ok(result) => Ok(result),
            Err(error) => {
                cancelled.store(true, Ordering::Release);
                Err(error)
            }
        }
    }
}

fn receive_graph_reply(
    receiver: &Receiver<Result<GraphCommandResult, RustGraphReadError>>,
    deadline: Instant,
) -> Result<GraphCommandResult, RustGraphReadError> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(RustGraphReadError::DeadlineExceeded);
    }
    receiver
        .recv_timeout(remaining)
        .map_err(|error| match error {
            mpsc::RecvTimeoutError::Timeout => RustGraphReadError::DeadlineExceeded,
            mpsc::RecvTimeoutError::Disconnected => RustGraphReadError::Store,
        })?
}

fn validate_graph_query(query: &str) -> Result<(), RustGraphReadError> {
    if query.is_empty()
        || query.len() > MAX_GRAPH_SYMBOL_QUERY_BYTES
        || query.chars().any(char::is_control)
    {
        Err(RustGraphReadError::InvalidQuery)
    } else {
        Ok(())
    }
}

fn validate_graph_site_selector(site: &RustGraphSiteSelector) -> Result<(), RustGraphReadError> {
    if [
        site.occurrence_span().start().get(),
        site.occurrence_span().end().get(),
        site.target_span().start().get(),
        site.target_span().end().get(),
    ]
    .into_iter()
    .any(|offset| i64::try_from(offset).is_err())
        || site.identity().is_none()
    {
        Err(RustGraphReadError::InvalidSelector)
    } else {
        Ok(())
    }
}

fn validate_graph_definition_selector(
    definition: &RustGraphDefinitionRecord,
) -> Result<(), RustGraphReadError> {
    if [
        definition.name_span().start().get(),
        definition.name_span().end().get(),
        definition.declaration_span().start().get(),
        definition.declaration_span().end().get(),
    ]
    .into_iter()
    .any(|offset| i64::try_from(offset).is_err())
        || definition.identity().is_none()
    {
        Err(RustGraphReadError::InvalidSelector)
    } else {
        Ok(())
    }
}

fn validate_graph_trace_start(start: &RustGraphTraceStart) -> Result<(), RustGraphReadError> {
    match start {
        RustGraphTraceStart::Definition(definition) => {
            validate_graph_definition_selector(definition)
        }
        RustGraphTraceStart::Site(site) => validate_graph_site_selector(site),
    }
}

fn map_graph_store_error(error: SqliteStoreError) -> RustGraphReadError {
    match error {
        SqliteStoreError::DeadlineExceeded | SqliteStoreError::ReplyTimeout => {
            RustGraphReadError::DeadlineExceeded
        }
        SqliteStoreError::Cancelled => RustGraphReadError::Cancelled,
        _ => RustGraphReadError::Store,
    }
}

#[cfg(test)]
mod graph_command_tests {
    use repowitness_analysis::RustGraphSiteKind;
    use repowitness_application::SourceSlotEpoch;
    use repowitness_domain::{
        AnalysisArtifactDigest, ByteOffset, ByteSpan, ConfigurationDigest, ConnectedWorkspaceId,
        RepositoryIdentityDigest, RepositoryPath, RepositoryPathLimits, SourceSlotId,
    };

    use super::*;
    use crate::sqlite::{PinnedWorkspaceViewMember, WorkspaceViewId};

    #[test]
    fn graph_command_debug_redacts_query_and_configuration_contents() {
        let workspace = ConnectedWorkspaceId::new([0xA1; 32]);
        let member = PinnedWorkspaceViewMember::new(
            0,
            SourceSlotId::new([0xB2; 32]),
            SourceSlotEpoch::INITIAL,
            RepositoryIdentityDigest::new([0xC3; 32]),
            GenerationId::from_database(7),
        );
        let view =
            PinnedWorkspaceView::new(workspace, WorkspaceViewId::from_database(11), vec![member]);
        let (reply, _receiver) = mpsc::sync_channel(1);
        let command = GraphCommand {
            view,
            graph_generation: GenerationId::from_database(7),
            limits: Some(RustGraphReadLimits::default()),
            configuration_digest: Some(ConfigurationDigest::new([0xD4; 32])),
            operation: GraphOperation::SymbolSearch {
                query: "private_customer_symbol".to_owned(),
            },
            cancelled: Arc::new(AtomicBool::new(false)),
            deadline: Instant::now(),
            reply,
        };
        let rendered = format!("{command:?}");

        assert!(rendered.contains("symbol_search"));
        assert!(rendered.contains("ConfigurationDigest"));
        assert!(!rendered.contains("private_customer_symbol"));
        for secret in ["A1", "B2", "C3", "D4"] {
            assert!(!rendered.contains(secret));
        }

        let private_path = RepositoryPath::try_from_bytes(
            b"src/private_customer_symbol.rs",
            RepositoryPathLimits::new(4096, 256),
        )
        .expect("private fixture path should validate");
        let target_span = ByteSpan::try_new(ByteOffset::new(10), ByteOffset::new(16))
            .expect("target span should validate");
        let occurrence_span = ByteSpan::try_new(ByteOffset::new(8), ByteOffset::new(18))
            .expect("occurrence span should validate");
        let private_site = RustGraphSiteSelector::new(
            SourceSlotId::new([0xE5; 32]),
            private_path,
            AnalysisArtifactDigest::new([0xF6; 32]),
            7,
            RustGraphSiteKind::Call,
            occurrence_span,
            target_span,
        );
        let (reply, _receiver) = mpsc::sync_channel(1);
        let trace_command = GraphCommand {
            view: command.view.clone(),
            graph_generation: GenerationId::from_database(7),
            limits: Some(RustGraphReadLimits::default()),
            configuration_digest: Some(ConfigurationDigest::new([0xD4; 32])),
            operation: GraphOperation::Trace {
                start: Box::new(RustGraphTraceStart::Site(private_site)),
                direction: RustGraphDirection::Outbound,
                edge_kinds: RustGraphEdgeKinds::ALL,
            },
            cancelled: Arc::new(AtomicBool::new(false)),
            deadline: Instant::now(),
            reply,
        };
        let rendered = format!("{trace_command:?}");
        assert!(rendered.contains("trace"));
        assert!(!rendered.contains("private_customer_symbol"));
        assert!(!rendered.contains("E5"));
        assert!(!rendered.contains("F6"));
    }
}
