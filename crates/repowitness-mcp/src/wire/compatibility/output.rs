#![allow(
    missing_docs,
    reason = "public field names and enclosing comments form the versioned JSON schema"
)]

use schemars::JsonSchema;
use serde::Serialize;

use super::super::{
    CODE_SEARCH_TOOL_NAME, DIAGNOSTICS_TOOL_NAME, GRAPH_ARCHITECTURE_TOOL_NAME,
    GRAPH_SEARCH_TOOL_NAME, GRAPH_STATUS_TOOL_NAME, GraphStatusOutput, SYMBOL_GET_TOOL_NAME,
};
use super::{
    COMPATIBILITY_PROFILE_VERSION, GET_ARCHITECTURE_ALIAS_TOOL_NAME,
    GET_CODE_SNIPPET_ALIAS_TOOL_NAME, GET_GRAPH_SCHEMA_ALIAS_TOOL_NAME,
    INCUMBENT_COMPATIBLE_PROFILE, INCUMBENT_COMPATIBLE_SURFACE, INDEX_STATUS_ALIAS_TOOL_NAME,
    SEARCH_CODE_ALIAS_TOOL_NAME, SEARCH_GRAPH_ALIAS_TOOL_NAME, TRACE_PATH_ALIAS_TOOL_NAME,
};

/// Conservative compatibility assessment for one alias.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompatibilityLevels {
    pub name: String,
    pub request: String,
    pub response: String,
    pub behavior: String,
}

/// Versioned, path-free capability receipt for one compatibility alias.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompatibilityReceipt {
    pub schema_version: u16,
    pub profile: String,
    pub surface: String,
    pub alias: String,
    pub canonical_tool: String,
    pub compatibility: CompatibilityLevels,
    pub known_limitations: Vec<String>,
}

/// Namespaced canonical result and its compatibility receipt.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompatibilityNamespace<T> {
    pub receipt: CompatibilityReceipt,
    pub canonical: T,
}

/// Stable compatibility response envelope.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompatibilityOutput<T> {
    pub repowitness: CompatibilityNamespace<T>,
}

/// Advertised graph request limits for the compatibility subset.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompatibilityGraphSchemaLimits {
    pub maximum_depth: u32,
    pub supported_directions: Vec<String>,
    pub traversable_edge_kinds: Vec<String>,
}

/// Version-1 graph/profile capability receipt returned by `get_graph_schema`.
#[derive(Clone, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompatibilityGraphSchema {
    pub status: GraphStatusOutput,
    pub definition_kinds: Vec<String>,
    pub site_kinds: Vec<String>,
    pub edge_kinds: Vec<String>,
    pub limits: CompatibilityGraphSchemaLimits,
    pub aliases: Vec<CompatibilityReceipt>,
    pub profile_known_limitations: Vec<String>,
}

#[derive(Clone, Copy)]
pub(crate) enum CompatibilityAlias {
    GetArchitecture,
    GetCodeSnippet,
    GetGraphSchema,
    IndexStatus,
    SearchCode,
    SearchGraph,
    TracePath,
}

impl CompatibilityAlias {
    pub(crate) const ALL: [Self; 7] = [
        Self::GetArchitecture,
        Self::GetCodeSnippet,
        Self::GetGraphSchema,
        Self::IndexStatus,
        Self::SearchCode,
        Self::SearchGraph,
        Self::TracePath,
    ];

    const fn name(self) -> &'static str {
        match self {
            Self::GetArchitecture => GET_ARCHITECTURE_ALIAS_TOOL_NAME,
            Self::GetCodeSnippet => GET_CODE_SNIPPET_ALIAS_TOOL_NAME,
            Self::GetGraphSchema => GET_GRAPH_SCHEMA_ALIAS_TOOL_NAME,
            Self::IndexStatus => INDEX_STATUS_ALIAS_TOOL_NAME,
            Self::SearchCode => SEARCH_CODE_ALIAS_TOOL_NAME,
            Self::SearchGraph => SEARCH_GRAPH_ALIAS_TOOL_NAME,
            Self::TracePath => TRACE_PATH_ALIAS_TOOL_NAME,
        }
    }

    const fn canonical_tool(self) -> &'static str {
        match self {
            Self::GetArchitecture => GRAPH_ARCHITECTURE_TOOL_NAME,
            Self::GetCodeSnippet => SYMBOL_GET_TOOL_NAME,
            Self::GetGraphSchema => GRAPH_STATUS_TOOL_NAME,
            Self::IndexStatus => DIAGNOSTICS_TOOL_NAME,
            Self::SearchCode => CODE_SEARCH_TOOL_NAME,
            Self::SearchGraph => GRAPH_SEARCH_TOOL_NAME,
            Self::TracePath => super::super::GRAPH_TRACE_TOOL_NAME,
        }
    }

    const fn known_limitations(self) -> &'static [&'static str] {
        match self {
            Self::GetArchitecture => &[
                "count_only_rust_syntax_graph",
                "no_packages_entry_points_tests_or_hotspots",
                "no_cross_language_graph",
            ],
            Self::GetCodeSnippet => &[
                "exact_selector_required",
                "no_fuzzy_symbol_lookup",
                "no_neighbor_expansion",
            ],
            Self::GetGraphSchema => &[
                "rust_syntax_graph_only",
                "no_open_query_language",
                "no_dynamic_or_cross_language_graph",
            ],
            Self::IndexStatus => &["single_fixed_repository", "no_background_task_inventory"],
            Self::SearchCode => &[
                "bounded_literal_terms_only",
                "single_fixed_repository",
                "pagination_unavailable",
            ],
            Self::SearchGraph => &[
                "exact_definition_name_only",
                "no_label_or_property_filters",
                "pagination_unavailable",
                "rust_syntax_graph_only",
            ],
            Self::TracePath => &[
                "exact_start_selector_required",
                "maximum_depth_five",
                "no_fuzzy_function_name",
                "rust_syntax_graph_only",
            ],
        }
    }

    fn receipt(self) -> CompatibilityReceipt {
        CompatibilityReceipt {
            schema_version: COMPATIBILITY_PROFILE_VERSION,
            profile: INCUMBENT_COMPATIBLE_PROFILE.to_owned(),
            surface: INCUMBENT_COMPATIBLE_SURFACE.to_owned(),
            alias: self.name().to_owned(),
            canonical_tool: self.canonical_tool().to_owned(),
            compatibility: CompatibilityLevels {
                name: "compatible".to_owned(),
                request: "subset".to_owned(),
                response: "extended".to_owned(),
                behavior: "not_assessed".to_owned(),
            },
            known_limitations: owned(self.known_limitations()),
        }
    }
}

pub(crate) fn compatibility_output<T>(
    alias: CompatibilityAlias,
    canonical: T,
) -> CompatibilityOutput<T> {
    CompatibilityOutput {
        repowitness: CompatibilityNamespace {
            receipt: alias.receipt(),
            canonical,
        },
    }
}

pub(crate) fn graph_schema_output(
    status: GraphStatusOutput,
) -> CompatibilityOutput<CompatibilityGraphSchema> {
    let schema = CompatibilityGraphSchema {
        status,
        definition_kinds: owned(&[
            "function",
            "method",
            "struct",
            "enum",
            "union",
            "trait",
            "module",
            "type_alias",
            "constant",
            "static",
            "macro",
        ]),
        site_kinds: owned(&["import", "reference", "call", "macro_call", "test_marker"]),
        edge_kinds: owned(&["import", "reference", "call"]),
        limits: CompatibilityGraphSchemaLimits {
            maximum_depth: 5,
            supported_directions: owned(&["outbound", "inbound"]),
            traversable_edge_kinds: owned(&["import", "reference", "call"]),
        },
        aliases: CompatibilityAlias::ALL
            .into_iter()
            .map(CompatibilityAlias::receipt)
            .collect(),
        profile_known_limitations: owned(&[
            "single_fixed_repository",
            "read_only_aliases",
            "pagination_unavailable",
            "rust_syntax_graph_only",
            "no_open_query_language",
        ]),
    };
    compatibility_output(CompatibilityAlias::GetGraphSchema, schema)
}

fn owned(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}
