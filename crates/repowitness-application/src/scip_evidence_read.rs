//! Storage-neutral package-scoped SCIP precision-evidence reads.

mod use_case;

pub use repowitness_domain::ScipSymbol;

pub use use_case::{
    ScipEvidenceReadError, ScipEvidenceReadPort, ScipEvidenceReadPortResult,
    ScipEvidenceReadRequest, ScipEvidenceReadResult, ScipEvidenceReadSelection,
    ScipEvidenceReadSelectionError, scip_evidence_read,
};
