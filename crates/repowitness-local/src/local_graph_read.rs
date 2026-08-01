//! One-shot local composition for canonical generation-pinned graph reads.

use std::{
    error::Error,
    fmt,
    path::Path,
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};

use repowitness_application::{
    ConnectedWorkspaceIdTextV1, RepositoryIdentityTextError, RepositoryIdentityTextV1,
    ResolvedConfiguration, RustGraphDefinitionSelector,
    RustGraphReadError as ApplicationGraphReadError, RustGraphReadOperation, RustGraphReadPort,
    RustGraphReadPortResult, RustGraphReadRequest, RustGraphReadResult, RustGraphReadSelection,
    RustGraphSiteSelector as ApplicationSiteSelector, RustGraphTraceDirection,
    RustGraphTraceLimits, RustGraphTraceStartSelector, SourceSlotIdTextV1,
    WorkspaceIdentityTextError, rust_graph_read,
};
use repowitness_domain::ConnectedWorkspaceId;

use crate::{
    GenerationId, OwnedSqliteReader, RustGraphArchitectureSummary, RustGraphAvailability,
    RustGraphDefinitionRecord, RustGraphDirection, RustGraphEvidenceResult, RustGraphImpactResult,
    RustGraphPublicationSummary, RustGraphReadError, RustGraphReadLimits, RustGraphSiteSelector,
    RustGraphSymbolSearchResult, RustGraphTraceResult, RustGraphTraceStart, SqliteStoreError,
};

/// Default end-to-end deadline for one local graph read.
pub const DEFAULT_LOCAL_RUST_GRAPH_READ_DEADLINE: Duration = Duration::from_secs(30);

/// Operation-specific bounded output from the local graph adapter.
#[derive(Debug, Eq, PartialEq)]
pub enum LocalRustGraphReadOutput {
    /// Categorical graph publication availability.
    Status(RustGraphAvailability),
    /// Exact definition-name search.
    Search(RustGraphSymbolSearchResult),
    /// Exact site evidence, or no matching site.
    Evidence(Box<LocalRustGraphEvidenceRead>),
    /// Count-only architecture summary.
    Architecture(RustGraphArchitectureSummary),
    /// Deterministic bounded traversal.
    Trace(RustGraphTraceResult),
    /// Conservative inbound impact.
    Impact(RustGraphImpactResult),
}

/// One exact evidence lookup with the complete publication searched even on a miss.
#[derive(Debug, Eq, PartialEq)]
pub struct LocalRustGraphEvidenceRead {
    publication: RustGraphPublicationSummary,
    evidence: Option<RustGraphEvidenceResult>,
}

impl LocalRustGraphEvidenceRead {
    /// Returns the immutable complete graph receipt searched by this lookup.
    #[must_use]
    pub const fn publication(&self) -> &RustGraphPublicationSummary {
        &self.publication
    }

    /// Returns exact site evidence when the selector exists in the pinned graph.
    #[must_use]
    pub const fn evidence(&self) -> Option<&RustGraphEvidenceResult> {
        self.evidence.as_ref()
    }
}

/// Validated local graph result with the concrete view and generation.
pub type LocalRustGraphReadResult = RustGraphReadResult<LocalRustGraphReadOutput>;

/// Explicit inputs for one local graph operation.
pub struct LocalRustGraphReadRequest<'a> {
    database: &'a Path,
    workspace: LocalRustGraphWorkspace<'a>,
    exact_pin: Option<(i64, i64)>,
    operation: RustGraphReadOperation,
    configuration: Option<&'a ResolvedConfiguration>,
    deadline: Duration,
}

/// One explicitly selected graph workspace context.
pub enum LocalRustGraphWorkspace<'a> {
    /// The compatible default workspace derived from one repository identity.
    SingleRepository {
        /// Canonical repository identity text.
        repository_identity: &'a str,
    },
    /// One explicitly selected source slot in a connected workspace.
    ConnectedWorkspace {
        /// Canonical connected-workspace identity text.
        connected_workspace: &'a str,
        /// Canonical source-slot identity text.
        source_slot: &'a str,
    },
}

impl<'a> LocalRustGraphReadRequest<'a> {
    /// Constructs an active-view request with a conservative deadline.
    #[must_use]
    pub const fn new(
        database: &'a Path,
        repository_identity: &'a str,
        operation: RustGraphReadOperation,
    ) -> Self {
        Self {
            database,
            workspace: LocalRustGraphWorkspace::SingleRepository {
                repository_identity,
            },
            exact_pin: None,
            operation,
            configuration: None,
            deadline: DEFAULT_LOCAL_RUST_GRAPH_READ_DEADLINE,
        }
    }

    /// Constructs an active-view request for one explicit connected-workspace source slot.
    #[must_use]
    pub const fn for_connected_workspace(
        database: &'a Path,
        connected_workspace: &'a str,
        source_slot: &'a str,
        operation: RustGraphReadOperation,
    ) -> Self {
        Self {
            database,
            workspace: LocalRustGraphWorkspace::ConnectedWorkspace {
                connected_workspace,
                source_slot,
            },
            exact_pin: None,
            operation,
            configuration: None,
            deadline: DEFAULT_LOCAL_RUST_GRAPH_READ_DEADLINE,
        }
    }

    /// Pins one exact immutable workspace view and graph generation.
    pub fn with_exact_pin(
        mut self,
        workspace_view: i64,
        graph_generation: i64,
    ) -> Result<Self, LocalRustGraphReadError> {
        if workspace_view <= 0 || graph_generation <= 0 {
            return Err(LocalRustGraphReadError::InvalidSelection);
        }
        self.exact_pin = Some((workspace_view, graph_generation));
        Ok(self)
    }

    /// Applies resolved configuration as a monotonic graph bound.
    #[must_use]
    pub const fn with_configuration(mut self, configuration: &'a ResolvedConfiguration) -> Self {
        self.configuration = Some(configuration);
        self
    }

    /// Replaces the end-to-end deadline duration.
    #[must_use]
    pub const fn with_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = deadline;
        self
    }
}

impl fmt::Debug for LocalRustGraphReadRequest<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalRustGraphReadRequest")
            .field("database", &"<redacted-path>")
            .field("workspace", &self.workspace)
            .field("exact_pin", &self.exact_pin)
            .field("operation", &operation_label(&self.operation))
            .field(
                "configuration_digest",
                &self.configuration.map(ResolvedConfiguration::digest),
            )
            .field("deadline", &self.deadline)
            .finish()
    }
}

impl fmt::Debug for LocalRustGraphWorkspace<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match self {
            Self::SingleRepository { .. } => "single_repository",
            Self::ConnectedWorkspace { .. } => "connected_workspace",
        };
        formatter
            .debug_struct("LocalRustGraphWorkspace")
            .field("kind", &kind)
            .finish_non_exhaustive()
    }
}

/// Stable content-redacted one-shot local graph failure.
#[derive(Debug)]
pub enum LocalRustGraphReadError {
    /// The configured repository identity is malformed or non-canonical.
    RepositoryIdentity(RepositoryIdentityTextError),
    /// The configured connected-workspace identity is malformed or non-canonical.
    ConnectedWorkspaceIdentity(WorkspaceIdentityTextError),
    /// The configured source-slot identity is malformed or non-canonical.
    SourceSlotIdentity(WorkspaceIdentityTextError),
    /// The exact view/generation pair is invalid.
    InvalidSelection,
    /// The absolute deadline cannot be represented.
    DeadlineNotRepresentable,
    /// The read-only SQLite owner could not start.
    ReaderStart(SqliteStoreError),
    /// The shared application read failed.
    Read(ApplicationGraphReadError<LocalRustGraphPortError>),
    /// The read-only SQLite owner did not shut down cleanly.
    Shutdown(SqliteStoreError),
}

impl fmt::Display for LocalRustGraphReadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::RepositoryIdentity(_) => "repository identity is invalid",
            Self::ConnectedWorkspaceIdentity(_) => "connected workspace identity is invalid",
            Self::SourceSlotIdentity(_) => "source slot identity is invalid",
            Self::InvalidSelection => "Rust graph immutable context selection is invalid",
            Self::DeadlineNotRepresentable => "Rust graph deadline cannot be represented",
            Self::ReaderStart(_) => "Rust graph reader startup failed",
            Self::Read(_) => "local Rust graph read failed",
            Self::Shutdown(_) => "Rust graph reader shutdown failed",
        })
    }
}

impl Error for LocalRustGraphReadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::RepositoryIdentity(error) => Some(error),
            Self::ConnectedWorkspaceIdentity(error) | Self::SourceSlotIdentity(error) => {
                Some(error)
            }
            Self::ReaderStart(error) | Self::Shutdown(error) => Some(error),
            Self::Read(error) => Some(error),
            Self::InvalidSelection | Self::DeadlineNotRepresentable => None,
        }
    }
}

/// Stable local implementation failure behind the application graph port.
#[derive(Debug)]
pub enum LocalRustGraphPortError {
    /// The selected workspace view is unavailable or not single-source.
    ViewUnavailable,
    /// SQLite view pinning failed.
    View(SqliteStoreError),
    /// Native graph persistence or traversal failed.
    Graph(RustGraphReadError),
    /// An evidence lookup returned without a complete graph publication.
    GraphUnavailable,
}

impl fmt::Display for LocalRustGraphPortError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ViewUnavailable => "Rust graph workspace view is unavailable",
            Self::View(_) => "Rust graph workspace view read failed",
            Self::Graph(_) => "Rust graph operation failed",
            Self::GraphUnavailable => "Rust graph publication is unavailable",
        })
    }
}

impl Error for LocalRustGraphPortError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::View(error) => Some(error),
            Self::Graph(error) => Some(error),
            Self::ViewUnavailable | Self::GraphUnavailable => None,
        }
    }
}

struct LocalRustGraphPort<'a> {
    reader: &'a OwnedSqliteReader,
    configuration: Option<&'a ResolvedConfiguration>,
}

impl RustGraphReadPort for LocalRustGraphPort<'_> {
    type Output = LocalRustGraphReadOutput;
    type Error = LocalRustGraphPortError;

    fn read(
        &self,
        selection: RustGraphReadSelection,
        operation: &RustGraphReadOperation,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<RustGraphReadPortResult<Self::Output>, Self::Error> {
        let requested_view = selection.exact_pin().map(|(view, _)| view);
        let view = self
            .reader
            .pin_workspace_view(
                selection.connected_workspace(),
                requested_view,
                Arc::clone(&cancelled),
                deadline,
            )
            .map_err(LocalRustGraphPortError::View)?
            .ok_or(LocalRustGraphPortError::ViewUnavailable)?;
        let member = match selection.source_slot() {
            Some(source_slot) => view
                .members()
                .iter()
                .find(|member| member.source_slot() == source_slot)
                .copied()
                .ok_or(LocalRustGraphPortError::ViewUnavailable)?,
            None => {
                let [member] = view.members() else {
                    return Err(LocalRustGraphPortError::ViewUnavailable);
                };
                *member
            }
        };
        let graph_generation = match selection.exact_pin() {
            Some((_, generation)) if member.generation().get() == generation => {
                GenerationId::from_database(generation)
            }
            Some(_) => return Err(LocalRustGraphPortError::ViewUnavailable),
            None => member.generation(),
        };
        let output =
            self.read_operation(&view, graph_generation, operation, cancelled, deadline)?;
        Ok(RustGraphReadPortResult::new(
            view.connected_workspace(),
            view.view().get(),
            graph_generation.get(),
            output,
        ))
    }
}

impl LocalRustGraphPort<'_> {
    fn read_operation(
        &self,
        view: &crate::PinnedWorkspaceView,
        generation: GenerationId,
        operation: &RustGraphReadOperation,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<LocalRustGraphReadOutput, LocalRustGraphPortError> {
        let output = match operation {
            RustGraphReadOperation::Status => LocalRustGraphReadOutput::Status(
                self.reader
                    .rust_graph_status(view, generation, cancelled, deadline)
                    .map_err(LocalRustGraphPortError::Graph)?,
            ),
            RustGraphReadOperation::Search { query, limits } => LocalRustGraphReadOutput::Search(
                self.reader
                    .search_rust_graph_symbols(
                        view,
                        generation,
                        query.as_str(),
                        local_limits(*limits)?,
                        self.configuration,
                        cancelled,
                        deadline,
                    )
                    .map_err(LocalRustGraphPortError::Graph)?,
            ),
            RustGraphReadOperation::Evidence { site, limits } => {
                LocalRustGraphReadOutput::Evidence(Box::new(
                    self.read_evidence(view, generation, site, *limits, cancelled, deadline)?,
                ))
            }
            RustGraphReadOperation::Architecture { limits } => {
                LocalRustGraphReadOutput::Architecture(
                    self.reader
                        .rust_graph_architecture(
                            view,
                            generation,
                            local_limits(*limits)?,
                            self.configuration,
                            cancelled,
                            deadline,
                        )
                        .map_err(LocalRustGraphPortError::Graph)?,
                )
            }
            RustGraphReadOperation::Trace {
                start,
                direction,
                edge_kinds,
                limits,
            } => LocalRustGraphReadOutput::Trace(
                self.reader
                    .trace_rust_graph(
                        view,
                        generation,
                        local_start(start),
                        local_direction(*direction),
                        *edge_kinds,
                        local_limits(*limits)?,
                        self.configuration,
                        cancelled,
                        deadline,
                    )
                    .map_err(LocalRustGraphPortError::Graph)?,
            ),
            RustGraphReadOperation::Impact {
                start,
                edge_kinds,
                limits,
            } => LocalRustGraphReadOutput::Impact(
                self.reader
                    .analyze_rust_graph_impact(
                        view,
                        generation,
                        local_definition(start),
                        *edge_kinds,
                        local_limits(*limits)?,
                        self.configuration,
                        cancelled,
                        deadline,
                    )
                    .map_err(LocalRustGraphPortError::Graph)?,
            ),
        };
        Ok(output)
    }

    fn read_evidence(
        &self,
        view: &crate::PinnedWorkspaceView,
        generation: GenerationId,
        site: &ApplicationSiteSelector,
        limits: RustGraphTraceLimits,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    ) -> Result<LocalRustGraphEvidenceRead, LocalRustGraphPortError> {
        let evidence = self
            .reader
            .rust_graph_evidence(
                view,
                generation,
                local_site(site),
                local_limits(limits)?,
                self.configuration,
                Arc::clone(&cancelled),
                deadline,
            )
            .map_err(LocalRustGraphPortError::Graph)?;
        let publication = match evidence.as_ref() {
            Some(evidence) => evidence.publication().clone(),
            None => match self
                .reader
                .rust_graph_status(view, generation, cancelled, deadline)
                .map_err(LocalRustGraphPortError::Graph)?
            {
                RustGraphAvailability::Complete(publication) => *publication,
                RustGraphAvailability::NotProduced { .. } => {
                    return Err(LocalRustGraphPortError::GraphUnavailable);
                }
            },
        };
        Ok(LocalRustGraphEvidenceRead {
            publication,
            evidence,
        })
    }
}

/// Opens one reader, executes the shared graph use case, and shuts down.
pub fn read_local_rust_graph(
    request: LocalRustGraphReadRequest<'_>,
    cancelled: Arc<AtomicBool>,
) -> Result<LocalRustGraphReadResult, LocalRustGraphReadError> {
    let selection = resolve_selection(&request)?;
    let deadline = Instant::now()
        .checked_add(request.deadline)
        .ok_or(LocalRustGraphReadError::DeadlineNotRepresentable)?;
    let reader = OwnedSqliteReader::start(request.database, deadline)
        .map_err(LocalRustGraphReadError::ReaderStart)?;
    let port = LocalRustGraphPort {
        reader: &reader,
        configuration: request.configuration,
    };
    let result = rust_graph_read(
        &port,
        RustGraphReadRequest::new(selection, request.operation, cancelled, deadline),
    );
    let shutdown = reader.shutdown(deadline);
    match (result, shutdown) {
        (Ok(result), Ok(())) => Ok(result),
        (Err(error), _) => Err(LocalRustGraphReadError::Read(error)),
        (Ok(_), Err(error)) => Err(LocalRustGraphReadError::Shutdown(error)),
    }
}

fn resolve_selection(
    request: &LocalRustGraphReadRequest<'_>,
) -> Result<RustGraphReadSelection, LocalRustGraphReadError> {
    match request.workspace {
        LocalRustGraphWorkspace::SingleRepository {
            repository_identity,
        } => {
            let repository = RepositoryIdentityTextV1::decode(repository_identity)
                .map_err(LocalRustGraphReadError::RepositoryIdentity)?;
            let workspace = ConnectedWorkspaceId::for_single_repository(repository);
            match request.exact_pin {
                Some((view, generation)) => {
                    RustGraphReadSelection::exact(workspace, view, generation)
                        .map_err(|_| LocalRustGraphReadError::InvalidSelection)
                }
                None => Ok(RustGraphReadSelection::active(workspace)),
            }
        }
        LocalRustGraphWorkspace::ConnectedWorkspace {
            connected_workspace,
            source_slot,
        } => {
            let workspace = ConnectedWorkspaceIdTextV1::decode(connected_workspace)
                .map_err(LocalRustGraphReadError::ConnectedWorkspaceIdentity)?;
            let source_slot = SourceSlotIdTextV1::decode(source_slot)
                .map_err(LocalRustGraphReadError::SourceSlotIdentity)?;
            match request.exact_pin {
                Some((view, generation)) => RustGraphReadSelection::exact_source_slot(
                    workspace,
                    source_slot,
                    view,
                    generation,
                )
                .map_err(|_| LocalRustGraphReadError::InvalidSelection),
                None => Ok(RustGraphReadSelection::active_source_slot(
                    workspace,
                    source_slot,
                )),
            }
        }
    }
}

fn local_limits(
    limits: RustGraphTraceLimits,
) -> Result<RustGraphReadLimits, LocalRustGraphPortError> {
    RustGraphReadLimits::try_new_with_input(
        limits.max_input_edges(),
        limits.max_input_bytes(),
        limits.max_depth(),
        limits.max_results(),
        limits.max_visited_nodes(),
        limits.max_visited_edges(),
        limits.max_frontier(),
        limits.max_output_bytes(),
    )
    .map_err(LocalRustGraphPortError::Graph)
}

fn local_definition(selector: &RustGraphDefinitionSelector) -> RustGraphDefinitionRecord {
    RustGraphDefinitionRecord::new(
        selector.source_slot(),
        GenerationId::from_database(selector.source_generation()),
        selector.path().clone(),
        selector.content_digest(),
        selector.artifact(),
        selector.fact_ordinal(),
        selector.kind(),
        selector.name().to_owned(),
        selector.qualified_name().to_owned(),
        selector.name_span(),
        selector.declaration_span(),
    )
}

fn local_site(selector: &ApplicationSiteSelector) -> RustGraphSiteSelector {
    RustGraphSiteSelector::new(
        selector.source_slot(),
        selector.path().clone(),
        selector.artifact(),
        selector.ordinal(),
        selector.kind(),
        selector.occurrence_span(),
        selector.target_span(),
    )
}

fn local_start(selector: &RustGraphTraceStartSelector) -> RustGraphTraceStart {
    match selector {
        RustGraphTraceStartSelector::Definition(definition) => {
            RustGraphTraceStart::Definition(local_definition(definition))
        }
        RustGraphTraceStartSelector::Site(site) => RustGraphTraceStart::Site(local_site(site)),
    }
}

const fn local_direction(direction: RustGraphTraceDirection) -> RustGraphDirection {
    match direction {
        RustGraphTraceDirection::Outbound => RustGraphDirection::Outbound,
        RustGraphTraceDirection::Inbound => RustGraphDirection::Inbound,
    }
}

const fn operation_label(operation: &RustGraphReadOperation) -> &'static str {
    match operation {
        RustGraphReadOperation::Status => "status",
        RustGraphReadOperation::Search { .. } => "search",
        RustGraphReadOperation::Evidence { .. } => "evidence",
        RustGraphReadOperation::Architecture { .. } => "architecture",
        RustGraphReadOperation::Trace { .. } => "trace",
        RustGraphReadOperation::Impact { .. } => "impact",
    }
}

#[cfg(test)]
mod tests;
