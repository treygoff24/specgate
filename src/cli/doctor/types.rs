use serde::Serialize;

use super::trace_types::TraceResultKind;
use crate::verdict::WorkspacePackageInfo;

#[derive(Debug, Serialize)]
pub(super) struct DoctorOutput {
    pub(super) schema_version: String,
    pub(super) status: String,
    pub(super) spec_count: usize,
    pub(super) validation_errors: usize,
    pub(super) validation_warnings: usize,
    pub(super) graph_nodes: usize,
    pub(super) graph_edges: usize,
    pub(super) parse_warning_count: usize,
    pub(super) policy_violation_count: usize,
    pub(super) layer_config_issues: Vec<String>,
    pub(super) module_map_overlaps: Vec<DoctorOverlapOutput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) workspace_packages: Option<Vec<WorkspacePackageInfo>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) tsconfig_filename_override: Option<String>,
}

#[derive(Debug, Serialize)]
pub(super) struct DoctorOverlapOutput {
    pub(super) file: String,
    pub(super) selected_module: String,
    pub(super) matched_modules: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(super) struct DoctorCompareOutput {
    pub(super) schema_version: String,
    pub(super) status: String,
    pub(super) parity_verdict: String,
    pub(super) parser_mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) trace_parser: Option<String>,
    pub(super) configured: bool,
    pub(super) reason: Option<String>,
    pub(super) specgate_edge_count: usize,
    pub(super) trace_edge_count: usize,
    pub(super) missing_in_specgate: Vec<String>,
    pub(super) extra_in_specgate: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) mismatch_category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) actionable_mismatch_hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) structured_snapshot_in: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) structured_snapshot_out: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) specgate_resolution: Option<DoctorCompareResolutionOutput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) tsc_trace_resolution: Option<DoctorCompareResolutionOutput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) focus: Option<DoctorCompareFocusOutput>,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct DoctorCompareResolutionOutput {
    pub(super) source: String,
    #[serde(serialize_with = "serialize_trace_result_kind")]
    pub(super) result_kind: TraceResultKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) resolved_to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) package_name: Option<String>,
    pub(super) trace: Vec<String>,
}

pub(super) fn serialize_trace_result_kind<S>(
    kind: &TraceResultKind,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(kind.as_str())
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct DoctorCompareFocusOutput {
    pub(super) from: String,
    pub(super) import_specifier: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) resolved_to: Option<String>,
    pub(super) resolution_kind: String,
    pub(super) in_specgate_graph: bool,
    pub(super) specgate_trace: Vec<String>,
}
