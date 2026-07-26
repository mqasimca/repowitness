//! Categorical resolution outcomes for claims.

/// The categorical outcome of resolving a claim against available evidence.
///
/// A status does not imply a numeric confidence. Evidence and coverage remain
/// necessary to interpret every material result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResolutionStatus {
    /// Direct evidence establishes the claim within the reported scope.
    Confirmed,
    /// The claim follows from attributed indirect evidence.
    Inferred,
    /// Available evidence supports more than one material interpretation.
    Ambiguous,
    /// The requested relationship could not be resolved from available inputs.
    Unresolved,
    /// Missing or conflicting inputs prevent a categorical determination.
    Indeterminate,
}
