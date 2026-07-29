use super::model::{ConfigurationField, ConfigurationValidationError};

/// Built-in minimum newest-generation floor retained for every source slot.
pub const DEFAULT_CONFIGURATION_RETAINED_GENERATIONS_PER_SOURCE_SLOT: u64 = 2;
/// Built-in maximum generation candidates admitted by one retention pass.
pub const DEFAULT_CONFIGURATION_RETENTION_GENERATION_CANDIDATES: u64 = 64;
/// Built-in maximum shared logical row work admitted by one retention pass.
pub const DEFAULT_CONFIGURATION_RETENTION_ROWS: u64 = 1_000_000;
/// Built-in maximum estimated bytes admitted by one retention pass.
pub const DEFAULT_CONFIGURATION_RETENTION_BYTES: u64 = 512 * 1024 * 1024;

/// Absolute retained-generation floor ceiling accepted by configuration version 1.
pub const MAX_CONFIGURATION_RETAINED_GENERATIONS_PER_SOURCE_SLOT: u64 = 4_096;
/// Absolute generation-candidate ceiling accepted by configuration version 1.
pub const MAX_CONFIGURATION_RETENTION_GENERATION_CANDIDATES: u64 = 4_096;
/// Absolute estimated-row ceiling accepted by configuration version 1.
pub const MAX_CONFIGURATION_RETENTION_ROWS: u64 = 100_000_000;
/// Absolute estimated-byte ceiling accepted by configuration version 1.
pub const MAX_CONFIGURATION_RETENTION_BYTES: u64 = 16 * 1024 * 1024 * 1024;

/// Validated monotonic generation-retention requests from one provenance layer.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RetentionConfigurationOverrides {
    retained_generations_per_source_slot: Option<u64>,
    max_generation_candidates: Option<u64>,
    max_rows: Option<u64>,
    max_bytes: Option<u64>,
}

impl RetentionConfigurationOverrides {
    /// Validates every supplied retention value against the version-1 hard bounds.
    pub fn try_new(
        retained_generations_per_source_slot: Option<u64>,
        max_generation_candidates: Option<u64>,
        max_rows: Option<u64>,
        max_bytes: Option<u64>,
    ) -> Result<Self, ConfigurationValidationError> {
        validate_positive_maximum(
            retained_generations_per_source_slot,
            MAX_CONFIGURATION_RETAINED_GENERATIONS_PER_SOURCE_SLOT,
            ConfigurationField::RetainedGenerationsPerSourceSlot,
        )?;
        validate_positive_maximum(
            max_generation_candidates,
            MAX_CONFIGURATION_RETENTION_GENERATION_CANDIDATES,
            ConfigurationField::RetentionGenerationCandidates,
        )?;
        validate_positive_maximum(
            max_rows,
            MAX_CONFIGURATION_RETENTION_ROWS,
            ConfigurationField::RetentionRows,
        )?;
        validate_positive_maximum(
            max_bytes,
            MAX_CONFIGURATION_RETENTION_BYTES,
            ConfigurationField::RetentionBytes,
        )?;
        Ok(Self {
            retained_generations_per_source_slot,
            max_generation_candidates,
            max_rows,
            max_bytes,
        })
    }

    pub(super) const fn retained_generations_per_source_slot(self) -> Option<u64> {
        self.retained_generations_per_source_slot
    }

    pub(super) const fn max_generation_candidates(self) -> Option<u64> {
        self.max_generation_candidates
    }

    pub(super) const fn max_rows(self) -> Option<u64> {
        self.max_rows
    }

    pub(super) const fn max_bytes(self) -> Option<u64> {
        self.max_bytes
    }
}

fn validate_positive_maximum(
    value: Option<u64>,
    maximum: u64,
    field: ConfigurationField,
) -> Result<(), ConfigurationValidationError> {
    match value {
        Some(0) => Err(ConfigurationValidationError::Zero(field)),
        Some(value) if value > maximum => Err(ConfigurationValidationError::AboveMaximum(field)),
        Some(_) | None => Ok(()),
    }
}
