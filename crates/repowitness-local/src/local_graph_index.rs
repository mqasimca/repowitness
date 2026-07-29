use std::{
    error::Error,
    fmt,
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

use repowitness_analysis::{
    RustGraphDefinitionOccurrence, RustGraphResolution, RustGraphResolutionControl,
    RustGraphResolutionError, RustGraphResolutionLimits, RustGraphSiteOccurrence,
    resolve_rust_graph_sites,
};
use repowitness_application::{
    CanonicalAnalysisArtifactKey, PreparedRustIndex, SourceLanguage, hash_analysis_artifact_key,
};
use repowitness_domain::{
    ConnectedWorkspaceId, RepositoryIdentityDigest, RepositoryPath, SourceSlotId,
};

use crate::{
    GenerationId,
    rust_index::PreparedLocalRustGraphArtifact,
    sqlite::{
        PreparedRustGraphGeneration, RustGraphPreparationControl, RustGraphPreparationError,
        RustGraphSource, prepare_rust_graph_generation,
    },
};

type GraphArtifactInput = (
    SourceSlotId,
    RepositoryPath,
    CanonicalAnalysisArtifactKey,
    repowitness_analysis::RustGraphSiteAnalysis,
);

/// Complete raw-artifact and resolution input awaiting one concrete generation.
#[derive(Clone)]
pub(crate) struct PreparedLocalRustGraphProjection {
    connected_workspace: ConnectedWorkspaceId,
    source_slot: SourceSlotId,
    artifacts: Vec<GraphArtifactInput>,
    definitions: Vec<RustGraphDefinitionOccurrence>,
    resolution: RustGraphResolution,
}

impl PreparedLocalRustGraphProjection {
    pub(crate) fn into_generation(
        self,
        generation: GenerationId,
        cancelled: &AtomicBool,
        deadline: Instant,
    ) -> Result<PreparedRustGraphGeneration, LocalRustGraphProjectionError> {
        prepare_rust_graph_generation(
            self.connected_workspace,
            vec![RustGraphSource::new(self.source_slot, generation)],
            self.artifacts,
            self.definitions,
            self.resolution,
            RustGraphPreparationControl::new(cancelled, deadline),
        )
        .map_err(LocalRustGraphProjectionError::PersistencePreparation)
    }
}

impl fmt::Debug for PreparedLocalRustGraphProjection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedLocalRustGraphProjection")
            .field("connected_workspace", &self.connected_workspace)
            .field("source_slot", &self.source_slot)
            .field("artifact_count", &self.artifacts.len())
            .field("definition_count", &self.definitions.len())
            .field("resolution", &self.resolution)
            .finish()
    }
}

/// Prepares one complete single-source-slot graph projection before staging.
pub(crate) fn prepare_local_rust_graph_projection(
    repository: RepositoryIdentityDigest,
    prepared: &PreparedRustIndex,
    graph_artifacts: Box<[PreparedLocalRustGraphArtifact]>,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<PreparedLocalRustGraphProjection, LocalRustGraphProjectionError> {
    let connected_workspace = ConnectedWorkspaceId::for_single_repository(repository);
    let source_slot = SourceSlotId::for_repository(repository);
    prepare_local_rust_graph_projection_for_source_slot(
        connected_workspace,
        source_slot,
        prepared,
        graph_artifacts,
        cancelled,
        deadline,
    )
}

/// Prepares one complete graph projection for an explicit connected source slot.
pub(crate) fn prepare_local_rust_graph_projection_for_source_slot(
    connected_workspace: ConnectedWorkspaceId,
    source_slot: SourceSlotId,
    prepared: &PreparedRustIndex,
    graph_artifacts: Box<[PreparedLocalRustGraphArtifact]>,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<PreparedLocalRustGraphProjection, LocalRustGraphProjectionError> {
    check_projection_control(cancelled, deadline)?;
    let definitions = prepare_definitions(source_slot, prepared, cancelled, deadline)?;
    let (artifact_inputs, sites) =
        prepare_sites(source_slot, graph_artifacts, cancelled, deadline)?;
    let resolution = resolve_rust_graph_sites(
        &definitions,
        &sites,
        RustGraphResolutionLimits::DEFAULT,
        RustGraphResolutionControl::new(cancelled, deadline),
    )
    .map_err(LocalRustGraphProjectionError::Resolution)?;
    check_projection_control(cancelled, deadline)?;
    Ok(PreparedLocalRustGraphProjection {
        connected_workspace,
        source_slot,
        artifacts: artifact_inputs,
        definitions,
        resolution,
    })
}

fn prepare_definitions(
    source_slot: SourceSlotId,
    prepared: &PreparedRustIndex,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<Vec<RustGraphDefinitionOccurrence>, LocalRustGraphProjectionError> {
    let mut definitions = Vec::new();
    for file in prepared
        .files()
        .iter()
        .filter(|file| file.language() == SourceLanguage::Rust)
    {
        check_projection_control(cancelled, deadline)?;
        for (ordinal, fact) in file.analysis().facts().iter().enumerate() {
            check_projection_control(cancelled, deadline)?;
            definitions.push(
                RustGraphDefinitionOccurrence::try_new(
                    source_slot,
                    file.path().clone(),
                    file.artifact_digest(),
                    u64::try_from(ordinal)
                        .map_err(|_| LocalRustGraphProjectionError::CountOverflow)?,
                    fact.clone(),
                )
                .map_err(LocalRustGraphProjectionError::Definition)?,
            );
        }
    }
    Ok(definitions)
}

fn prepare_sites(
    source_slot: SourceSlotId,
    graph_artifacts: Box<[PreparedLocalRustGraphArtifact]>,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(Vec<GraphArtifactInput>, Vec<RustGraphSiteOccurrence>), LocalRustGraphProjectionError>
{
    let mut artifact_inputs = Vec::with_capacity(graph_artifacts.len());
    let mut sites = Vec::new();
    for artifact in graph_artifacts {
        check_projection_control(cancelled, deadline)?;
        let (path, key, analysis) = artifact.into_parts();
        let artifact_digest = hash_analysis_artifact_key(&key);
        for site in analysis.sites() {
            check_projection_control(cancelled, deadline)?;
            sites.push(
                RustGraphSiteOccurrence::try_new(
                    source_slot,
                    path.clone(),
                    artifact_digest,
                    site.clone(),
                )
                .map_err(LocalRustGraphProjectionError::Site)?,
            );
        }
        artifact_inputs.push((source_slot, path, key, analysis));
    }
    Ok((artifact_inputs, sites))
}

fn check_projection_control(
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<(), LocalRustGraphProjectionError> {
    if cancelled.load(Ordering::Acquire) {
        Err(LocalRustGraphProjectionError::Cancelled)
    } else if Instant::now() >= deadline {
        Err(LocalRustGraphProjectionError::DeadlineExceeded)
    } else {
        Ok(())
    }
}

/// Stable failure to build or persist a complete graph projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalRustGraphProjectionError {
    /// A declaration occurrence could not be represented exactly.
    Definition(RustGraphResolutionError),
    /// A raw graph-site occurrence could not be represented exactly.
    Site(RustGraphResolutionError),
    /// Complete generation-scoped resolution failed.
    Resolution(RustGraphResolutionError),
    /// Fixed-width occurrence accounting overflowed.
    CountOverflow,
    /// Cancellation was observed before complete output existed.
    Cancelled,
    /// The absolute monotonic deadline elapsed.
    DeadlineExceeded,
    /// Persistence projection validation failed.
    PersistencePreparation(RustGraphPreparationError),
}

impl fmt::Display for LocalRustGraphProjectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Definition(_) => "Rust graph definition occurrence is invalid",
            Self::Site(_) => "Rust graph site occurrence is invalid",
            Self::Resolution(_) => "Rust graph resolution failed",
            Self::CountOverflow => "Rust graph occurrence count overflowed",
            Self::Cancelled => "Rust graph projection cancelled",
            Self::DeadlineExceeded => "Rust graph projection deadline exceeded",
            Self::PersistencePreparation(_) => "Rust graph persistence projection is invalid",
        })
    }
}

impl Error for LocalRustGraphProjectionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Definition(source) | Self::Site(source) | Self::Resolution(source) => {
                Some(source)
            }
            Self::PersistencePreparation(source) => Some(source),
            Self::CountOverflow | Self::Cancelled | Self::DeadlineExceeded => None,
        }
    }
}
