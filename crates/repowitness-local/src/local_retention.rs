//! Explicit bounded local generation-retention maintenance.

mod execution;
mod model;

pub use execution::{apply_local_retention, plan_local_retention};
pub use model::{
    DEFAULT_LOCAL_RETENTION_TIMEOUT, LOCAL_RETENTION_PROFILE_VERSION, LocalRetentionApplyReport,
    LocalRetentionApplyRequest, LocalRetentionError, LocalRetentionErrorKind, LocalRetentionPins,
    LocalRetentionPlanReport, LocalRetentionPlanRequest, LocalRetentionPolicySummary,
    LocalRetentionRequestError, MAX_LOCAL_RETENTION_TIMEOUT,
};

#[cfg(test)]
mod tests;
