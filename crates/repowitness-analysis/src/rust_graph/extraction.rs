use repowitness_domain::{ByteOffset, ByteSpan};
use tree_sitter::Node;

use super::{
    RustGraphAnalysisControl, RustGraphAnalysisError, RustGraphAnalysisLimits,
    RustGraphEnclosingDefinition, RustGraphSite, RustGraphSiteEvidence, RustGraphSiteKind,
    RustGraphSiteOrdinal, RustSymbolKind,
};

pub(super) fn extract_site(
    node: Node<'_>,
    source: &[u8],
    limits: RustGraphAnalysisLimits,
    control: RustGraphAnalysisControl<'_>,
) -> Result<Option<RustGraphSite>, RustGraphAnalysisError> {
    if node.is_error() || node.is_missing() {
        return Ok(None);
    }
    if node.kind() == "identifier"
        && let Some(site) = extract_conditional_test_marker(node, source, limits, control)?
    {
        return Ok(Some(site));
    }
    match node.kind() {
        "use_declaration" => extract_field_site(
            node,
            "argument",
            RustGraphSiteKind::Import,
            RustGraphSiteEvidence::DirectSyntax,
            source,
            limits,
        ),
        "call_expression" => extract_field_site(
            node,
            "function",
            RustGraphSiteKind::Call,
            RustGraphSiteEvidence::DirectSyntax,
            source,
            limits,
        ),
        "macro_invocation" => extract_field_site(
            node,
            "macro",
            RustGraphSiteKind::MacroCall,
            RustGraphSiteEvidence::DirectSyntax,
            source,
            limits,
        ),
        "attribute_item" | "inner_attribute_item" => {
            extract_test_marker(node, source, limits, control)
        }
        _ if is_reference_candidate(node) && !is_excluded_reference(node) => build_site_from_node(
            node,
            node,
            RustGraphSiteKind::Reference,
            RustGraphSiteEvidence::SyntaxHeuristic,
            None,
            source,
            limits,
        )
        .map(Some),
        _ => Ok(None),
    }
}

fn extract_field_site(
    occurrence: Node<'_>,
    field: &str,
    kind: RustGraphSiteKind,
    evidence: RustGraphSiteEvidence,
    source: &[u8],
    limits: RustGraphAnalysisLimits,
) -> Result<Option<RustGraphSite>, RustGraphAnalysisError> {
    let Some(target) = occurrence.child_by_field_name(field) else {
        return if occurrence.has_error() {
            Ok(None)
        } else {
            Err(RustGraphAnalysisError::InvalidSyntaxShape)
        };
    };
    build_site_from_node(occurrence, target, kind, evidence, None, source, limits).map(Some)
}

fn extract_test_marker(
    item: Node<'_>,
    source: &[u8],
    limits: RustGraphAnalysisLimits,
    control: RustGraphAnalysisControl<'_>,
) -> Result<Option<RustGraphSite>, RustGraphAnalysisError> {
    let Some(attribute) = first_named_child_of_kind(item, "attribute") else {
        return if item.has_error() {
            Ok(None)
        } else {
            Err(RustGraphAnalysisError::InvalidSyntaxShape)
        };
    };
    let Some(path) = attribute.named_child(0) else {
        return if attribute.has_error() {
            Ok(None)
        } else {
            Err(RustGraphAnalysisError::InvalidSyntaxShape)
        };
    };
    let path_text = source_text(path, source)?;
    let associated = associated_definition(item, limits, control)?;
    if path_text == "test" || path_text.ends_with("::test") {
        return build_site_from_node(
            item,
            path,
            RustGraphSiteKind::TestMarker,
            RustGraphSiteEvidence::DirectSyntax,
            associated,
            source,
            limits,
        )
        .map(Some);
    }
    Ok(None)
}

fn first_named_child_of_kind<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find(|child| child.kind() == kind)
}

fn extract_conditional_test_marker(
    target: Node<'_>,
    source: &[u8],
    limits: RustGraphAnalysisLimits,
    control: RustGraphAnalysisControl<'_>,
) -> Result<Option<RustGraphSite>, RustGraphAnalysisError> {
    if source_text(target, source)? != "test" {
        return Ok(None);
    }
    let mut ancestor = target.parent();
    while let Some(candidate) = ancestor {
        if candidate.kind() == "attribute" {
            return conditional_marker_in_attribute(candidate, target, source, limits, control);
        }
        if matches!(candidate.kind(), "attribute_item" | "inner_attribute_item") {
            break;
        }
        ancestor = candidate.parent();
    }
    Ok(None)
}

fn conditional_marker_in_attribute(
    attribute: Node<'_>,
    target: Node<'_>,
    source: &[u8],
    limits: RustGraphAnalysisLimits,
    control: RustGraphAnalysisControl<'_>,
) -> Result<Option<RustGraphSite>, RustGraphAnalysisError> {
    let Some(path) = attribute.named_child(0) else {
        return Ok(None);
    };
    if !matches!(source_text(path, source)?, "cfg" | "cfg_attr")
        || !node_within_field(attribute, "arguments", target)
    {
        return Ok(None);
    }
    let Some(item) = attribute.parent() else {
        return Ok(None);
    };
    if !matches!(item.kind(), "attribute_item" | "inner_attribute_item") {
        return Ok(None);
    }
    build_site_from_node(
        item,
        target,
        RustGraphSiteKind::TestMarker,
        RustGraphSiteEvidence::SyntaxHeuristic,
        associated_definition(item, limits, control)?,
        source,
        limits,
    )
    .map(Some)
}

fn associated_definition<'tree>(
    attribute_item: Node<'tree>,
    limits: RustGraphAnalysisLimits,
    control: RustGraphAnalysisControl<'_>,
) -> Result<Option<Node<'tree>>, RustGraphAnalysisError> {
    if attribute_item.kind() == "inner_attribute_item" {
        return Ok(nearest_definition(attribute_item.parent()));
    }
    let mut sibling = attribute_item.next_named_sibling();
    let mut scanned = 0_u32;
    while let Some(candidate) = sibling {
        if let Some(outcome) = control.outcome() {
            return Err(outcome);
        }
        scanned = scanned
            .checked_add(1)
            .ok_or(RustGraphAnalysisError::NodeLimitExceeded)?;
        if scanned > limits.max_syntax_nodes() {
            return Err(RustGraphAnalysisError::NodeLimitExceeded);
        }
        match candidate.kind() {
            "attribute_item" | "line_comment" | "block_comment" => {
                sibling = candidate.next_named_sibling();
            }
            _ if definition_kind(candidate).is_some() => return Ok(Some(candidate)),
            _ => return Ok(None),
        }
    }
    Ok(None)
}

fn build_site_from_node(
    occurrence: Node<'_>,
    target: Node<'_>,
    kind: RustGraphSiteKind,
    evidence: RustGraphSiteEvidence,
    associated_definition: Option<Node<'_>>,
    source: &[u8],
    limits: RustGraphAnalysisLimits,
) -> Result<RustGraphSite, RustGraphAnalysisError> {
    build_site_from_span(
        occurrence,
        source_span(target, source)?,
        kind,
        evidence,
        associated_definition,
        source,
        limits,
    )
}

fn build_site_from_span(
    occurrence: Node<'_>,
    target_span: ByteSpan,
    kind: RustGraphSiteKind,
    evidence: RustGraphSiteEvidence,
    associated_definition: Option<Node<'_>>,
    source: &[u8],
    limits: RustGraphAnalysisLimits,
) -> Result<RustGraphSite, RustGraphAnalysisError> {
    let occurrence_span = source_span(occurrence, source)?;
    let raw_target = source_text_at_span(target_span, source)?;
    if raw_target.is_empty() {
        return Err(RustGraphAnalysisError::InvalidSourceSpan);
    }
    if raw_target.len() > usize::from(limits.max_path_bytes()) {
        return Err(RustGraphAnalysisError::PathLimitExceeded);
    }
    let enclosing_definition =
        enclosing_definition(occurrence, associated_definition, source, limits)?;
    let site = RustGraphSite {
        ordinal: RustGraphSiteOrdinal::new(0),
        kind,
        evidence,
        occurrence_span,
        target_span,
        raw_target: raw_target.to_owned(),
        enclosing_definition,
    };
    validate_site(&site, source)?;
    Ok(site)
}

fn enclosing_definition(
    site: Node<'_>,
    associated: Option<Node<'_>>,
    source: &[u8],
    limits: RustGraphAnalysisLimits,
) -> Result<Option<RustGraphEnclosingDefinition>, RustGraphAnalysisError> {
    let definition = associated.or_else(|| nearest_definition(site.parent()));
    definition
        .map(|node| definition_descriptor(node, source, limits))
        .transpose()
}

fn nearest_definition(mut node: Option<Node<'_>>) -> Option<Node<'_>> {
    while let Some(candidate) = node {
        if definition_kind(candidate).is_some() {
            return Some(candidate);
        }
        node = candidate.parent();
    }
    None
}

fn definition_descriptor(
    node: Node<'_>,
    source: &[u8],
    limits: RustGraphAnalysisLimits,
) -> Result<RustGraphEnclosingDefinition, RustGraphAnalysisError> {
    let kind = definition_kind(node).ok_or(RustGraphAnalysisError::InvalidSyntaxShape)?;
    let name_node = node
        .child_by_field_name("name")
        .ok_or(RustGraphAnalysisError::InvalidSyntaxShape)?;
    let name = source_text(name_node, source)?;
    if name.is_empty() {
        return Err(RustGraphAnalysisError::InvalidSyntaxShape);
    }
    if name.len() > usize::from(limits.max_name_bytes()) {
        return Err(RustGraphAnalysisError::NameLimitExceeded);
    }
    let qualified_name = qualified_definition_name(node, name, source, limits)?;
    Ok(RustGraphEnclosingDefinition {
        kind,
        name: name.to_owned(),
        qualified_name,
        name_span: source_span(name_node, source)?,
        declaration_span: source_span(node, source)?,
    })
}

fn definition_kind(node: Node<'_>) -> Option<RustSymbolKind> {
    match node.kind() {
        "function_item" | "function_signature_item" if inside_method_container(node) => {
            Some(RustSymbolKind::Method)
        }
        "function_item" | "function_signature_item" => Some(RustSymbolKind::Function),
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

fn qualified_definition_name(
    node: Node<'_>,
    name: &str,
    source: &[u8],
    limits: RustGraphAnalysisLimits,
) -> Result<String, RustGraphAnalysisError> {
    let mut components = definition_containers(node, source, limits)?;
    components.reverse();
    let required = components.iter().try_fold(name.len(), |total, component| {
        total
            .checked_add(component.len())
            .and_then(|sum| sum.checked_add(2))
    });
    let required = required.ok_or(RustGraphAnalysisError::PathLimitExceeded)?;
    if required > usize::from(limits.max_path_bytes()) {
        return Err(RustGraphAnalysisError::PathLimitExceeded);
    }
    let mut qualified = String::with_capacity(required);
    for component in components {
        qualified.push_str(component);
        qualified.push_str("::");
    }
    qualified.push_str(name);
    Ok(qualified)
}

fn definition_containers<'source>(
    node: Node<'_>,
    source: &'source [u8],
    limits: RustGraphAnalysisLimits,
) -> Result<Vec<&'source str>, RustGraphAnalysisError> {
    let mut components = Vec::new();
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
            let component = source_text(container, source)?;
            if component.len() > usize::from(limits.max_name_bytes()) {
                return Err(RustGraphAnalysisError::NameLimitExceeded);
            }
            components.push(component);
        }
        ancestor = current.parent();
    }
    Ok(components)
}

fn is_reference_candidate(node: Node<'_>) -> bool {
    if !matches!(
        node.kind(),
        "identifier"
            | "shorthand_field_identifier"
            | "type_identifier"
            | "scoped_identifier"
            | "scoped_type_identifier"
            | "field_expression"
            | "generic_function"
    ) {
        return false;
    }
    !node
        .parent()
        .is_some_and(|parent| is_reference_candidate(parent))
}

fn is_excluded_reference(node: Node<'_>) -> bool {
    let mut ancestor = node.parent();
    while let Some(current) = ancestor {
        if excludes_reference(current, node) {
            return true;
        }
        ancestor = current.parent();
    }
    false
}

fn excludes_reference(ancestor: Node<'_>, node: Node<'_>) -> bool {
    match ancestor.kind() {
        "attribute"
        | "attribute_item"
        | "inner_attribute_item"
        | "token_tree"
        | "visibility_modifier"
        | "token_binding_pattern"
        | "token_repetition_pattern"
        | "token_tree_pattern" => true,
        "match_pattern" if node_within_field(ancestor, "condition", node) => false,
        kind if kind.ends_with("_pattern") => !is_pattern_reference(node),
        "use_declaration" => node_within_field(ancestor, "argument", node),
        "call_expression" => node_within_field(ancestor, "function", node),
        "macro_invocation" => node_within_field(ancestor, "macro", node),
        "let_declaration" | "let_condition" | "parameter" | "variadic_parameter"
        | "for_expression" => {
            node_within_field(ancestor, "pattern", node) && !is_pattern_reference(node)
        }
        "closure_parameters" => node.parent() == Some(ancestor) && !is_pattern_reference(node),
        "type_parameter" | "const_parameter" | "lifetime_parameter" => {
            node_within_field(ancestor, "name", node)
        }
        "enum_variant" => node_within_field(ancestor, "name", node),
        "extern_crate_declaration" => {
            node_within_field(ancestor, "name", node) || node_within_field(ancestor, "alias", node)
        }
        "field_declaration" => node_within_field(ancestor, "name", node),
        _ if definition_kind(ancestor).is_some() => node_within_field(ancestor, "name", node),
        _ => false,
    }
}

fn is_pattern_reference(node: Node<'_>) -> bool {
    if matches!(node.kind(), "scoped_identifier" | "scoped_type_identifier") {
        return true;
    }
    let mut ancestor = node.parent();
    while let Some(current) = ancestor {
        match current.kind() {
            "struct_pattern" | "tuple_struct_pattern"
                if node_within_field(current, "type", node) =>
            {
                return true;
            }
            "range_pattern"
                if node_within_field(current, "left", node)
                    || node_within_field(current, "right", node) =>
            {
                return true;
            }
            "token_binding_pattern" | "token_repetition_pattern" | "token_tree_pattern" => {
                return false;
            }
            _ => {}
        }
        ancestor = current.parent();
    }
    false
}

fn node_within_field(ancestor: Node<'_>, field: &str, node: Node<'_>) -> bool {
    ancestor
        .child_by_field_name(field)
        .is_some_and(|field_node| range_contains(field_node.byte_range(), node.byte_range()))
}

fn range_contains(outer: std::ops::Range<usize>, inner: std::ops::Range<usize>) -> bool {
    outer.start <= inner.start && inner.end <= outer.end
}

pub(super) fn owned_text_bytes(site: &RustGraphSite) -> Result<u64, RustGraphAnalysisError> {
    let mut total = u64::try_from(site.raw_target.len())
        .map_err(|_| RustGraphAnalysisError::OwnedTextLimitExceeded)?;
    if let Some(enclosing) = &site.enclosing_definition {
        total = total
            .checked_add(
                u64::try_from(enclosing.name.len())
                    .map_err(|_| RustGraphAnalysisError::OwnedTextLimitExceeded)?,
            )
            .and_then(|value| {
                u64::try_from(enclosing.qualified_name.len())
                    .ok()
                    .and_then(|length| value.checked_add(length))
            })
            .ok_or(RustGraphAnalysisError::OwnedTextLimitExceeded)?;
    }
    Ok(total)
}

pub(super) fn validate_site(
    site: &RustGraphSite,
    source: &[u8],
) -> Result<(), RustGraphAnalysisError> {
    let occurrence = checked_span_range(site.occurrence_span, source)?;
    let target = checked_span_range(site.target_span, source)?;
    if site.raw_target.is_empty()
        || !range_contains(occurrence, target.clone())
        || source.get(target) != Some(site.raw_target.as_bytes())
    {
        return Err(RustGraphAnalysisError::InvalidSourceSpan);
    }
    if let Some(enclosing) = &site.enclosing_definition {
        validate_enclosing_definition(enclosing, source)?;
    }
    Ok(())
}

fn validate_enclosing_definition(
    definition: &RustGraphEnclosingDefinition,
    source: &[u8],
) -> Result<(), RustGraphAnalysisError> {
    let declaration = checked_span_range(definition.declaration_span, source)?;
    let name = checked_span_range(definition.name_span, source)?;
    if definition.name.is_empty()
        || definition.qualified_name.is_empty()
        || !range_contains(declaration, name.clone())
        || source.get(name) != Some(definition.name.as_bytes())
    {
        return Err(RustGraphAnalysisError::InvalidSourceSpan);
    }
    Ok(())
}

fn source_text<'source>(
    node: Node<'_>,
    source: &'source [u8],
) -> Result<&'source str, RustGraphAnalysisError> {
    let range = checked_range(node, source)?;
    let bytes = source
        .get(range)
        .ok_or(RustGraphAnalysisError::InvalidSourceSpan)?;
    std::str::from_utf8(bytes).map_err(|_| RustGraphAnalysisError::InvalidSourceEncoding)
}

fn source_text_at_span(span: ByteSpan, source: &[u8]) -> Result<&str, RustGraphAnalysisError> {
    let range = checked_span_range(span, source)?;
    let bytes = source
        .get(range)
        .ok_or(RustGraphAnalysisError::InvalidSourceSpan)?;
    std::str::from_utf8(bytes).map_err(|_| RustGraphAnalysisError::InvalidSourceEncoding)
}

fn source_span(node: Node<'_>, source: &[u8]) -> Result<ByteSpan, RustGraphAnalysisError> {
    let range = checked_range(node, source)?;
    source_span_from_range(range)
}

fn source_span_from_range(
    range: std::ops::Range<usize>,
) -> Result<ByteSpan, RustGraphAnalysisError> {
    let start =
        u64::try_from(range.start).map_err(|_| RustGraphAnalysisError::InvalidSourceSpan)?;
    let end = u64::try_from(range.end).map_err(|_| RustGraphAnalysisError::InvalidSourceSpan)?;
    ByteSpan::try_new(ByteOffset::new(start), ByteOffset::new(end))
        .map_err(|_| RustGraphAnalysisError::InvalidSourceSpan)
}

fn checked_range(
    node: Node<'_>,
    source: &[u8],
) -> Result<std::ops::Range<usize>, RustGraphAnalysisError> {
    let range = node.byte_range();
    if range.end > source.len() {
        return Err(RustGraphAnalysisError::InvalidSourceSpan);
    }
    Ok(range)
}

fn checked_span_range(
    span: ByteSpan,
    source: &[u8],
) -> Result<std::ops::Range<usize>, RustGraphAnalysisError> {
    let start = usize::try_from(span.start().get())
        .map_err(|_| RustGraphAnalysisError::InvalidSourceSpan)?;
    let end =
        usize::try_from(span.end().get()).map_err(|_| RustGraphAnalysisError::InvalidSourceSpan)?;
    if end > source.len() || start > end {
        return Err(RustGraphAnalysisError::InvalidSourceSpan);
    }
    Ok(start..end)
}
