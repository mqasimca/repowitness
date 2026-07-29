impl RustGraphSiteAnalyzer {
    /// Creates an analyzer using the pinned Rust grammar.
    pub fn new() -> Result<Self, RustGraphAnalysisError> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_rust::LANGUAGE.into())
            .map_err(|_| RustGraphAnalysisError::GrammarUnavailable)?;
        Ok(Self { parser })
    }

    /// Extracts complete artifact-local sites without performing any I/O.
    pub fn analyze(
        &mut self,
        source: &[u8],
        limits: RustGraphAnalysisLimits,
        control: RustGraphAnalysisControl<'_>,
    ) -> Result<RustGraphSiteAnalysis, RustGraphAnalysisError> {
        admit_source(source, limits, control)?;
        let tree = parse_source(&mut self.parser, source, control)?;
        traverse_graph_sites(&tree, source, limits, control)
    }
}

impl fmt::Debug for RustGraphSiteAnalyzer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RustGraphSiteAnalyzer")
            .field("language", &"Rust")
            .finish_non_exhaustive()
    }
}

fn admit_source(
    source: &[u8],
    limits: RustGraphAnalysisLimits,
    control: RustGraphAnalysisControl<'_>,
) -> Result<(), RustGraphAnalysisError> {
    if let Some(outcome) = control.outcome() {
        return Err(outcome);
    }
    let source_bytes =
        u64::try_from(source.len()).map_err(|_| RustGraphAnalysisError::SourceLimitExceeded)?;
    if source_bytes > limits.max_source_bytes() {
        return Err(RustGraphAnalysisError::SourceLimitExceeded);
    }
    std::str::from_utf8(source).map_err(|_| RustGraphAnalysisError::InvalidSourceEncoding)?;
    Ok(())
}

fn parse_source(
    parser: &mut Parser,
    source: &[u8],
    control: RustGraphAnalysisControl<'_>,
) -> Result<tree_sitter::Tree, RustGraphAnalysisError> {
    let mut interrupted = None;
    let mut progress = |_: &tree_sitter::ParseState| {
        if let Some(outcome) = control.outcome() {
            interrupted = Some(outcome);
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    };
    let mut read = |offset: usize, _| source.get(offset..).unwrap_or_default();
    let tree = parser.parse_with_options(
        &mut read,
        None,
        Some(ParseOptions::new().progress_callback(&mut progress)),
    );
    if let Some(outcome) = interrupted {
        parser.reset();
        return Err(outcome);
    }
    match tree {
        Some(tree) => Ok(tree),
        None => {
            parser.reset();
            Err(RustGraphAnalysisError::ParseFailed)
        }
    }
}

fn traverse_graph_sites(
    tree: &tree_sitter::Tree,
    source: &[u8],
    limits: RustGraphAnalysisLimits,
    control: RustGraphAnalysisControl<'_>,
) -> Result<RustGraphSiteAnalysis, RustGraphAnalysisError> {
    let mut state = TraversalState::default();
    let mut cursor = tree.walk();

    loop {
        state.visit(cursor.node(), source, limits, control)?;
        if cursor.goto_first_child() {
            state.descend(limits)?;
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                return state.finish(source, control);
            }
            state.ascend()?;
        }
    }
}

#[derive(Default)]
struct TraversalState {
    sites: Vec<RustGraphSite>,
    visited_nodes: u32,
    syntax_error_nodes: u32,
    depth: u16,
    max_observed_depth: u16,
    owned_text_bytes: u64,
}

impl TraversalState {
    fn visit(
        &mut self,
        node: tree_sitter::Node<'_>,
        source: &[u8],
        limits: RustGraphAnalysisLimits,
        control: RustGraphAnalysisControl<'_>,
    ) -> Result<(), RustGraphAnalysisError> {
        if let Some(outcome) = control.outcome() {
            return Err(outcome);
        }
        self.visited_nodes = self
            .visited_nodes
            .checked_add(1)
            .ok_or(RustGraphAnalysisError::NodeLimitExceeded)?;
        if self.visited_nodes > limits.max_syntax_nodes() {
            return Err(RustGraphAnalysisError::NodeLimitExceeded);
        }
        if node.is_error() || node.is_missing() {
            self.syntax_error_nodes = self.syntax_error_nodes.saturating_add(1);
        }
        if let Some(site) = extraction::extract_site(node, source, limits, control)? {
            let max_sites = usize::try_from(limits.max_graph_sites())
                .map_err(|_| RustGraphAnalysisError::SiteLimitExceeded)?;
            if self.sites.len() >= max_sites {
                return Err(RustGraphAnalysisError::SiteLimitExceeded);
            }
            self.owned_text_bytes = self
                .owned_text_bytes
                .checked_add(extraction::owned_text_bytes(&site)?)
                .ok_or(RustGraphAnalysisError::OwnedTextLimitExceeded)?;
            if self.owned_text_bytes > limits.max_owned_text_bytes() {
                return Err(RustGraphAnalysisError::OwnedTextLimitExceeded);
            }
            self.sites.push(site);
        }
        Ok(())
    }

    fn descend(&mut self, limits: RustGraphAnalysisLimits) -> Result<(), RustGraphAnalysisError> {
        self.depth = self
            .depth
            .checked_add(1)
            .ok_or(RustGraphAnalysisError::DepthLimitExceeded)?;
        if self.depth > limits.max_syntax_depth() {
            return Err(RustGraphAnalysisError::DepthLimitExceeded);
        }
        self.max_observed_depth = self.max_observed_depth.max(self.depth);
        Ok(())
    }

    fn ascend(&mut self) -> Result<(), RustGraphAnalysisError> {
        self.depth = self
            .depth
            .checked_sub(1)
            .ok_or(RustGraphAnalysisError::InvalidSourceSpan)?;
        Ok(())
    }

    fn finish(
        mut self,
        source: &[u8],
        control: RustGraphAnalysisControl<'_>,
    ) -> Result<RustGraphSiteAnalysis, RustGraphAnalysisError> {
        for (index, site) in self.sites.iter_mut().enumerate() {
            if let Some(outcome) = control.outcome() {
                return Err(outcome);
            }
            let ordinal =
                u32::try_from(index).map_err(|_| RustGraphAnalysisError::SiteLimitExceeded)?;
            site.ordinal = RustGraphSiteOrdinal::new(ordinal);
            extraction::validate_site(site, source)?;
        }
        if let Some(outcome) = control.outcome() {
            return Err(outcome);
        }
        Ok(RustGraphSiteAnalysis {
            sites: self.sites,
            visited_nodes: self.visited_nodes,
            syntax_error_nodes: self.syntax_error_nodes,
            max_observed_depth: self.max_observed_depth,
            owned_text_bytes: self.owned_text_bytes,
        })
    }
}
