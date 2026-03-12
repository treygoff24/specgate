use serde::Serialize;

use super::trace_types::TraceResultKind;
use crate::spec::Severity;
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) layer_config_issues: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) category_config_issues: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) unique_export_config_issues: Vec<String>,
    pub(super) module_map_overlaps: Vec<DoctorOverlapOutput>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) findings: Vec<DoctorFindingOutput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) workspace_packages: Option<Vec<WorkspacePackageInfo>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) tsconfig_filename_override: Option<String>,
}

#[derive(Debug, Serialize)]
pub(super) struct DoctorFindingOutput {
    pub(super) rule: String,
    pub(super) severity: Severity,
    pub(super) module: String,
    pub(super) message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) spec_path: Option<String>,
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
    #[serde(serialize_with = "serialize_compare_status")]
    pub(super) status: CompareStatus,
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
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_optional_mismatch_category"
    )]
    pub(super) mismatch_category: Option<MismatchCategory>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CompareStatus {
    Match,
    Mismatch,
    Skipped,
}

impl CompareStatus {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Match => "match",
            Self::Mismatch => "mismatch",
            Self::Skipped => "skipped",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MismatchCategory {
    EdgeSetDiff,
    FocusedUnknown,
    FocusedTargetMismatch,
    FocusedSpecgateMissingResolution,
    FocusedTscMissingResolution,
    FocusedClassificationMismatch,
    FocusedEdgeSetDiff,
    FocusedResolutionMismatch,
    ExtensionAlias,
    ConditionNames,
    Paths,
    Exports,
}

impl MismatchCategory {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::EdgeSetDiff => "edge_set_diff",
            Self::FocusedUnknown => "focused_unknown",
            Self::FocusedTargetMismatch => "focused_target_mismatch",
            Self::FocusedSpecgateMissingResolution => "focused_specgate_missing_resolution",
            Self::FocusedTscMissingResolution => "focused_tsc_missing_resolution",
            Self::FocusedClassificationMismatch => "focused_classification_mismatch",
            Self::FocusedEdgeSetDiff => "focused_edge_set_diff",
            Self::FocusedResolutionMismatch => "focused_resolution_mismatch",
            Self::ExtensionAlias => "extension_alias",
            Self::ConditionNames => "condition_names",
            Self::Paths => "paths",
            Self::Exports => "exports",
        }
    }
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
    #[serde(serialize_with = "serialize_focus_resolution_kind")]
    pub(super) resolution_kind: FocusResolutionKind,
    pub(super) in_specgate_graph: bool,
    pub(super) specgate_trace: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FocusResolutionKind {
    FirstParty,
    ThirdParty,
    Unresolvable,
}

impl FocusResolutionKind {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::FirstParty => "first_party",
            Self::ThirdParty => "third_party",
            Self::Unresolvable => "unresolvable",
        }
    }
}

pub(super) fn serialize_compare_status<S>(
    status: &CompareStatus,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(status.as_str())
}

pub(super) fn serialize_optional_mismatch_category<S>(
    category: &Option<MismatchCategory>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match category {
        Some(category) => serializer.serialize_some(category.as_str()),
        None => serializer.serialize_none(),
    }
}

pub(super) fn serialize_focus_resolution_kind<S>(
    kind: &FocusResolutionKind,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(kind.as_str())
}
