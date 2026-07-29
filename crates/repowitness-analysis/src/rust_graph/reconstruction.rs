use super::{
    RustGraphAnalysisControl, RustGraphAnalysisError, RustGraphAnalysisLimits, RustGraphSite,
    RustGraphSiteAnalysis, extraction,
};

impl RustGraphSiteAnalysis {
    /// Reconstructs and validates one complete artifact at a persistence boundary.
    pub fn try_from_parts(
        sites: Vec<RustGraphSite>,
        visited_nodes: u32,
        syntax_error_nodes: u32,
        max_observed_depth: u16,
        owned_text_bytes: u64,
        limits: RustGraphAnalysisLimits,
    ) -> Result<Self, RustGraphAnalysisError> {
        Self::try_from_parts_with_checkpoint(
            sites,
            visited_nodes,
            syntax_error_nodes,
            max_observed_depth,
            owned_text_bytes,
            limits,
            || Ok(()),
        )
    }

    /// Reconstructs an artifact with cooperative cancellation and a deadline.
    pub fn try_from_parts_with_control(
        sites: Vec<RustGraphSite>,
        visited_nodes: u32,
        syntax_error_nodes: u32,
        max_observed_depth: u16,
        owned_text_bytes: u64,
        limits: RustGraphAnalysisLimits,
        control: RustGraphAnalysisControl<'_>,
    ) -> Result<Self, RustGraphAnalysisError> {
        Self::try_from_parts_with_checkpoint(
            sites,
            visited_nodes,
            syntax_error_nodes,
            max_observed_depth,
            owned_text_bytes,
            limits,
            || control.outcome().map_or(Ok(()), Err),
        )
    }

    fn try_from_parts_with_checkpoint(
        sites: Vec<RustGraphSite>,
        visited_nodes: u32,
        syntax_error_nodes: u32,
        max_observed_depth: u16,
        owned_text_bytes: u64,
        limits: RustGraphAnalysisLimits,
        mut checkpoint: impl FnMut() -> Result<(), RustGraphAnalysisError>,
    ) -> Result<Self, RustGraphAnalysisError> {
        checkpoint()?;
        validate_metadata(
            sites.len(),
            visited_nodes,
            syntax_error_nodes,
            max_observed_depth,
            limits,
        )?;
        let mut calculated_owned_text_bytes = 0_u64;
        for (index, site) in sites.iter().enumerate() {
            checkpoint()?;
            let expected_ordinal =
                u32::try_from(index).map_err(|_| RustGraphAnalysisError::InvalidAnalysisShape)?;
            if site.ordinal().get() != expected_ordinal {
                return Err(RustGraphAnalysisError::InvalidAnalysisShape);
            }
            calculated_owned_text_bytes = calculated_owned_text_bytes
                .checked_add(extraction::owned_text_bytes(site)?)
                .ok_or(RustGraphAnalysisError::OwnedTextLimitExceeded)?;
        }
        checkpoint()?;
        if calculated_owned_text_bytes != owned_text_bytes
            || owned_text_bytes > limits.max_owned_text_bytes()
        {
            return Err(RustGraphAnalysisError::InvalidAnalysisShape);
        }
        Ok(Self {
            sites,
            visited_nodes,
            syntax_error_nodes,
            max_observed_depth,
            owned_text_bytes,
        })
    }
}

fn validate_metadata(
    site_count: usize,
    visited_nodes: u32,
    syntax_error_nodes: u32,
    max_observed_depth: u16,
    limits: RustGraphAnalysisLimits,
) -> Result<(), RustGraphAnalysisError> {
    if !limits.is_valid()
        || visited_nodes == 0
        || visited_nodes > limits.max_syntax_nodes()
        || syntax_error_nodes > visited_nodes
        || max_observed_depth > limits.max_syntax_depth()
        || u32::try_from(site_count)
            .ok()
            .is_none_or(|count| count > limits.max_graph_sites())
    {
        Err(RustGraphAnalysisError::InvalidAnalysisShape)
    } else {
        Ok(())
    }
}
