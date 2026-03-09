use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::deterministic::{normalize_path, normalize_repo_relative};

pub(super) const TRACE_JSON_MAX_DEPTH: usize = 256;
pub(super) const TRACE_JSON_MAX_VISITED_NODES: usize = 1_000_000;
pub(super) const TRACE_STEPS_MAX_LINES: usize = 48;
pub(crate) const STRUCTURED_TRACE_SCHEMA_VERSION: &str = "1";

#[derive(Debug, Clone)]
pub(super) struct ParsedTraceData {
    pub(super) edges: BTreeSet<(String, String)>,
    pub(super) resolutions: Vec<TraceResolutionRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum TraceResultKind {
    FirstParty,
    ThirdParty,
    Unresolvable,
    NotObserved,
}

impl TraceResultKind {
    pub(super) fn as_str(&self) -> &'static str {
        match self {
            Self::FirstParty => "first_party",
            Self::ThirdParty => "third_party",
            Self::Unresolvable => "unresolvable",
            Self::NotObserved => "not_observed",
        }
    }

    pub(super) fn from_str(s: &str) -> Self {
        match s {
            "first_party" => Self::FirstParty,
            "third_party" => Self::ThirdParty,
            "unresolvable" => Self::Unresolvable,
            "not_observed" => Self::NotObserved,
            _ => Self::NotObserved,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct TraceResolutionRecord {
    pub(super) from: String,
    pub(super) import_specifier: String,
    pub(super) result_kind: TraceResultKind,
    pub(super) resolved_to: Option<String>,
    pub(super) package_name: Option<String>,
    pub(super) trace: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TraceParserKind {
    StructuredSnapshot,
    LegacyTraceText,
}

impl TraceParserKind {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::StructuredSnapshot => "structured_snapshot",
            Self::LegacyTraceText => "legacy_trace_text",
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct ParsedTraceResult {
    pub(super) data: ParsedTraceData,
    pub(super) parser_kind: TraceParserKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct StructuredTraceSnapshot {
    #[serde(default = "structured_trace_schema_version")]
    pub(super) schema_version: String,
    #[serde(default)]
    pub(super) edges: Vec<StructuredTraceEdge>,
    #[serde(default)]
    pub(super) resolutions: Vec<StructuredTraceResolution>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct StructuredTraceEdge {
    pub(super) from: String,
    pub(super) to: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct StructuredTraceResolution {
    pub(super) from: String,
    #[serde(alias = "import", alias = "specifier")]
    pub(super) import_specifier: String,
    #[serde(default, alias = "resolution_kind")]
    pub(super) result_kind: Option<String>,
    #[serde(default, alias = "resolvedTo", alias = "to")]
    pub(super) resolved_to: Option<String>,
    #[serde(default, alias = "packageName")]
    pub(super) package_name: Option<String>,
    #[serde(default)]
    pub(super) trace: Vec<String>,
}

impl StructuredTraceResolution {
    pub(super) fn to_trace_result_kind(&self) -> TraceResultKind {
        self.result_kind
            .as_deref()
            .map(TraceResultKind::from_str)
            .unwrap_or_else(|| {
                infer_trace_result_kind(self.resolved_to.as_deref(), self.package_name.as_deref())
            })
    }
}

pub(super) fn structured_trace_schema_version() -> String {
    STRUCTURED_TRACE_SCHEMA_VERSION.to_string()
}

pub(super) fn has_structured_snapshot_shape(value: &serde_json::Value) -> bool {
    let Some(map) = value.as_object() else {
        return false;
    };

    matches!(map.get("edges"), Some(serde_json::Value::Array(_)))
        || matches!(map.get("resolutions"), Some(serde_json::Value::Array(_)))
}

pub(super) fn infer_trace_result_kind(
    resolved_to: Option<&str>,
    package_name: Option<&str>,
) -> TraceResultKind {
    if package_name.is_some() {
        return TraceResultKind::ThirdParty;
    }

    match resolved_to {
        Some(target) if path_contains_node_modules(target) => TraceResultKind::ThirdParty,
        Some(_) => TraceResultKind::FirstParty,
        None => TraceResultKind::Unresolvable,
    }
}

pub(super) fn json_string_field<'a>(
    map: &'a serde_json::Map<String, serde_json::Value>,
    keys: &[&str],
) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| map.get(*key).and_then(serde_json::Value::as_str))
}

pub(super) fn normalize_trace_path(project_root: &Path, raw: &str) -> String {
    let path = Path::new(raw);
    if path.is_absolute() {
        let canonical = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        normalize_repo_relative(project_root, &canonical)
    } else {
        normalize_path(path)
    }
}

pub(super) fn path_contains_node_modules(raw: &str) -> bool {
    Path::new(raw)
        .components()
        .any(|component| component.as_os_str() == "node_modules")
}
