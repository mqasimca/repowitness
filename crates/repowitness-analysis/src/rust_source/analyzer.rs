impl RustSourceAnalyzer {
    /// Creates an analyzer using the pinned Rust grammar.
    pub fn new() -> Result<Self, RustAnalysisError> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_rust::LANGUAGE.into())
            .map_err(|_| RustAnalysisError::GrammarUnavailable)?;
        Ok(Self { parser })
    }

    /// Analyzes immutable bytes without performing filesystem or database I/O.
    pub fn analyze(
        &mut self,
        source: &[u8],
        limits: RustAnalysisLimits,
        control: RustAnalysisControl<'_>,
    ) -> Result<RustSourceAnalysis, RustAnalysisError> {
        if let Some(outcome) = control.outcome() {
            return Err(outcome);
        }
        let source_bytes =
            u64::try_from(source.len()).map_err(|_| RustAnalysisError::SourceLimitExceeded)?;
        if source_bytes > limits.max_source_bytes {
            return Err(RustAnalysisError::SourceLimitExceeded);
        }

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
        let tree = self.parser.parse_with_options(
            &mut read,
            None,
            Some(ParseOptions::new().progress_callback(&mut progress)),
        );
        if let Some(outcome) = interrupted {
            self.parser.reset();
            return Err(outcome);
        }
        let tree = tree.ok_or(RustAnalysisError::ParseFailed)?;
        traverse_tree(&tree, source, limits, control)
    }
}

impl fmt::Debug for RustSourceAnalyzer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RustSourceAnalyzer")
            .field("language", &"Rust")
            .finish_non_exhaustive()
    }
}

fn traverse_tree(
    tree: &tree_sitter::Tree,
    source: &[u8],
    limits: RustAnalysisLimits,
    control: RustAnalysisControl<'_>,
) -> Result<RustSourceAnalysis, RustAnalysisError> {
    let mut facts = Vec::new();
    let mut visited_nodes = 0_u32;
    let mut syntax_error_nodes = 0_u32;
    let mut depth = 0_u16;
    let mut cursor = tree.walk();

    loop {
        if let Some(outcome) = control.outcome() {
            return Err(outcome);
        }
        visited_nodes = visited_nodes
            .checked_add(1)
            .ok_or(RustAnalysisError::NodeLimitExceeded)?;
        if visited_nodes > limits.max_syntax_nodes {
            return Err(RustAnalysisError::NodeLimitExceeded);
        }

        let node = cursor.node();
        if node.is_error() || node.is_missing() {
            syntax_error_nodes = syntax_error_nodes.saturating_add(1);
        }
        if let Some(kind) = symbol_kind(node) {
            if facts.len()
                >= usize::try_from(limits.max_symbol_facts)
                    .map_err(|_| RustAnalysisError::FactLimitExceeded)?
            {
                return Err(RustAnalysisError::FactLimitExceeded);
            }
            facts.push(extract_symbol_fact(node, kind, source, limits)?);
        }

        if cursor.goto_first_child() {
            depth = depth
                .checked_add(1)
                .ok_or(RustAnalysisError::DepthLimitExceeded)?;
            if depth > limits.max_syntax_depth {
                return Err(RustAnalysisError::DepthLimitExceeded);
            }
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                return RustSourceAnalysis::try_from_parts(
                    facts,
                    visited_nodes,
                    syntax_error_nodes,
                    limits,
                );
            }
            depth = depth
                .checked_sub(1)
                .ok_or(RustAnalysisError::InvalidSourceSpan)?;
        }
    }
}

fn symbol_kind(node: Node<'_>) -> Option<RustSymbolKind> {
    match node.kind() {
        "function_item" if inside_method_container(node) => Some(RustSymbolKind::Method),
        "function_signature_item" if inside_method_container(node) => Some(RustSymbolKind::Method),
        "function_item" => Some(RustSymbolKind::Function),
        "function_signature_item" => Some(RustSymbolKind::Function),
        "struct_item" => Some(RustSymbolKind::Struct),
        "enum_item" => Some(RustSymbolKind::Enum),
        "union_item" => Some(RustSymbolKind::Union),
        "trait_item" => Some(RustSymbolKind::Trait),
        "mod_item" => Some(RustSymbolKind::Module),
        "type_item" => Some(RustSymbolKind::TypeAlias),
        "const_item" => Some(RustSymbolKind::Constant),
        "static_item" => Some(RustSymbolKind::Static),
        "macro_definition" => Some(RustSymbolKind::Macro),
        _ => None,
    }
}

fn inside_method_container(node: Node<'_>) -> bool {
    let mut ancestor = node.parent();
    while let Some(current) = ancestor {
        match current.kind() {
            "function_item" | "function_signature_item" => return false,
            "impl_item" | "trait_item" => return true,
            _ => ancestor = current.parent(),
        }
    }
    false
}

fn extract_symbol_fact(
    node: Node<'_>,
    kind: RustSymbolKind,
    source: &[u8],
    limits: RustAnalysisLimits,
) -> Result<RustSymbolFact, RustAnalysisError> {
    let name_node = node
        .child_by_field_name("name")
        .ok_or(RustAnalysisError::InvalidSourceSpan)?;
    let name = source_text(name_node, source)?;
    if name.len() > usize::from(limits.max_symbol_name_bytes) {
        return Err(RustAnalysisError::NameLimitExceeded);
    }
    let qualified_name = qualified_name(node, name, source, limits)?;
    let mut fact = RustSymbolFact::try_new(
        kind,
        name.to_owned(),
        qualified_name,
        source_span(name_node, source)?,
        source_span(node, source)?,
        limits,
    )?;
    fact.correspondence = Some(
        fingerprint_rust_occurrence(source, &fact)
            .map_err(|_| RustAnalysisError::InvalidSourceSpan)?,
    );
    Ok(fact)
}

fn validate_reusable_fact(
    fact: &RustSymbolFact,
    source: &[u8],
    limits: RustAnalysisLimits,
) -> Result<(), RustAnalysisError> {
    validate_fact_structure(fact, limits)?;
    let name_start = usize::try_from(fact.name_span.start().get())
        .map_err(|_| RustAnalysisError::InvalidAnalysisArtifact)?;
    let name_end = usize::try_from(fact.name_span.end().get())
        .map_err(|_| RustAnalysisError::InvalidAnalysisArtifact)?;
    let declaration_end = usize::try_from(fact.declaration_span.end().get())
        .map_err(|_| RustAnalysisError::InvalidAnalysisArtifact)?;
    if declaration_end > source.len()
        || source.get(name_start..name_end) != Some(fact.name.as_bytes())
    {
        return Err(RustAnalysisError::InvalidAnalysisArtifact);
    }
    if let Some(expected) = fact.correspondence {
        let actual = fingerprint_rust_occurrence(source, fact)
            .map_err(|_| RustAnalysisError::InvalidAnalysisArtifact)?;
        if actual != expected {
            return Err(RustAnalysisError::InvalidAnalysisArtifact);
        }
    }
    Ok(())
}

fn validate_fact_structure(
    fact: &RustSymbolFact,
    limits: RustAnalysisLimits,
) -> Result<(), RustAnalysisError> {
    if fact.name.is_empty()
        || fact.qualified_name.is_empty()
        || fact.name.len() > usize::from(limits.max_symbol_name_bytes())
        || fact.qualified_name.len() > usize::from(limits.max_qualified_name_bytes())
        || fact.name_span.start() < fact.declaration_span.start()
        || fact.name_span.end() > fact.declaration_span.end()
    {
        return Err(RustAnalysisError::InvalidAnalysisArtifact);
    }
    Ok(())
}

fn qualified_name(
    node: Node<'_>,
    name: &str,
    source: &[u8],
    limits: RustAnalysisLimits,
) -> Result<String, RustAnalysisError> {
    let mut containers = Vec::new();
    let mut ancestor = node.parent();
    while let Some(current) = ancestor {
        let container = match current.kind() {
            "impl_item" => current.child_by_field_name("type"),
            "trait_item" | "mod_item" | "function_item" | "function_signature_item" => {
                current.child_by_field_name("name")
            }
            _ => None,
        };
        if let Some(container) = container {
            let text = source_text(container, source)?;
            if text.len() > usize::from(limits.max_symbol_name_bytes) {
                return Err(RustAnalysisError::NameLimitExceeded);
            }
            containers.push(text);
        }
        ancestor = current.parent();
    }

    let required_bytes = containers
        .iter()
        .try_fold(name.len(), |total, component| {
            total
                .checked_add(component.len())
                .and_then(|sum| sum.checked_add(2))
        })
        .ok_or(RustAnalysisError::QualifiedNameLimitExceeded)?;
    if required_bytes > usize::from(limits.max_qualified_name_bytes) {
        return Err(RustAnalysisError::QualifiedNameLimitExceeded);
    }
    let mut qualified = String::with_capacity(required_bytes);
    for container in containers.iter().rev() {
        qualified.push_str(container);
        qualified.push_str("::");
    }
    qualified.push_str(name);
    Ok(qualified)
}

fn source_text<'a>(node: Node<'_>, source: &'a [u8]) -> Result<&'a str, RustAnalysisError> {
    let range = node.byte_range();
    let bytes = source
        .get(range)
        .ok_or(RustAnalysisError::InvalidSourceSpan)?;
    std::str::from_utf8(bytes).map_err(|_| RustAnalysisError::InvalidIdentifierEncoding)
}

fn source_span(node: Node<'_>, source: &[u8]) -> Result<ByteSpan, RustAnalysisError> {
    let range = node.byte_range();
    if range.end > source.len() {
        return Err(RustAnalysisError::InvalidSourceSpan);
    }
    let start = u64::try_from(range.start).map_err(|_| RustAnalysisError::InvalidSourceSpan)?;
    let end = u64::try_from(range.end).map_err(|_| RustAnalysisError::InvalidSourceSpan)?;
    ByteSpan::try_new(ByteOffset::new(start), ByteOffset::new(end))
        .map_err(|_| RustAnalysisError::InvalidSourceSpan)
}
