use crate::spec::config::{ReleaseChannel, SpecConfig};

use super::focus::DoctorCompareFocus;
use super::trace_types::{ParsedTraceData, TraceResultKind};
use super::types::DoctorCompareResolutionOutput;

pub(super) fn derive_tsc_focus_resolution(
    parsed_trace: &ParsedTraceData,
    focus: &DoctorCompareFocus,
) -> DoctorCompareResolutionOutput {
    if let Some(record) = parsed_trace.resolutions.iter().rev().find(|record| {
        record.from == focus.output.from && record.import_specifier == focus.output.import_specifier
    }) {
        return DoctorCompareResolutionOutput {
            source: "tsc_trace".to_string(),
            result_kind: record.result_kind.clone(),
            resolved_to: record.resolved_to.clone(),
            package_name: record.package_name.clone(),
            trace: if record.trace.is_empty() {
                vec!["matched trace record without explicit step lines".to_string()]
            } else {
                record.trace.clone()
            },
        };
    }

    if let Some(expected_edge) = &focus.edge {
        if parsed_trace.edges.contains(expected_edge) {
            return DoctorCompareResolutionOutput {
                source: "tsc_trace".to_string(),
                result_kind: TraceResultKind::FirstParty,
                resolved_to: Some(expected_edge.1.clone()),
                package_name: None,
                trace: vec![
                    "no explicit trace stanza matched `--from/--import`; inferred from edge parity"
                        .to_string(),
                ],
            };
        }
    }

    if let Some((_, to)) = parsed_trace
        .edges
        .iter()
        .find(|(from, _)| *from == focus.output.from)
    {
        return DoctorCompareResolutionOutput {
            source: "tsc_trace".to_string(),
            result_kind: TraceResultKind::FirstParty,
            resolved_to: Some(to.clone()),
            package_name: None,
            trace: vec![
                "trace did not carry import-specifier context; using first edge from the same source file"
                    .to_string(),
            ],
        };
    }

    DoctorCompareResolutionOutput {
        source: "tsc_trace".to_string(),
        result_kind: TraceResultKind::NotObserved,
        resolved_to: None,
        package_name: None,
        trace: vec![
            "no matching edge or trace stanza found for `--from/--import` in the supplied trace"
                .to_string(),
        ],
    }
}

pub(super) fn parity_verdict_for_status(status: &str) -> &'static str {
    match status {
        "match" => "MATCH",
        "mismatch" => "DIFF",
        _ => "SKIPPED",
    }
}

pub(super) fn classify_doctor_compare_mismatch(
    status: &str,
    focus: Option<&DoctorCompareFocus>,
    specgate_resolution: Option<&DoctorCompareResolutionOutput>,
    tsc_trace_resolution: Option<&DoctorCompareResolutionOutput>,
    missing_in_specgate: &[String],
    extra_in_specgate: &[String],
) -> Option<String> {
    if status != "mismatch" {
        return None;
    }

    let Some(focus) = focus else {
        return Some("edge_set_diff".to_string());
    };

    let (Some(specgate_resolution), Some(tsc_trace_resolution)) =
        (specgate_resolution, tsc_trace_resolution)
    else {
        return Some("focused_unknown".to_string());
    };

    let heuristic_tag =
        classify_focus_mismatch_tag(focus, specgate_resolution, tsc_trace_resolution);

    let category = match (
        &specgate_resolution.result_kind,
        &tsc_trace_resolution.result_kind,
    ) {
        _ if heuristic_tag.is_some() => heuristic_tag.expect("checked is_some"),
        (TraceResultKind::FirstParty, TraceResultKind::FirstParty)
            if specgate_resolution.resolved_to != tsc_trace_resolution.resolved_to =>
        {
            "focused_target_mismatch"
        }
        (
            TraceResultKind::Unresolvable | TraceResultKind::NotObserved,
            TraceResultKind::FirstParty,
        ) => "focused_specgate_missing_resolution",
        (
            TraceResultKind::FirstParty,
            TraceResultKind::Unresolvable | TraceResultKind::NotObserved,
        ) => "focused_tsc_missing_resolution",
        (TraceResultKind::ThirdParty, TraceResultKind::FirstParty)
        | (TraceResultKind::FirstParty, TraceResultKind::ThirdParty) => {
            "focused_classification_mismatch"
        }
        _ if !missing_in_specgate.is_empty() || !extra_in_specgate.is_empty() => {
            "focused_edge_set_diff"
        }
        _ => "focused_resolution_mismatch",
    };

    Some(category.to_string())
}

pub(super) fn classify_focus_mismatch_tag(
    focus: &DoctorCompareFocus,
    specgate_resolution: &DoctorCompareResolutionOutput,
    tsc_trace_resolution: &DoctorCompareResolutionOutput,
) -> Option<&'static str> {
    let specifier = focus.output.import_specifier.as_str();
    let is_relative = specifier.starts_with("./") || specifier.starts_with("../");

    if is_relative && matches_js_runtime_extension(specifier) {
        return Some("extension_alias");
    }

    if !is_relative {
        if resolution_path_looks_types(specgate_resolution.resolved_to.as_deref())
            || resolution_path_looks_types(tsc_trace_resolution.resolved_to.as_deref())
        {
            return Some("condition_names");
        }

        if specifier.starts_with('@') || specifier.contains('/') {
            return Some("paths");
        }

        return Some("exports");
    }

    None
}

pub(super) fn matches_js_runtime_extension(specifier: &str) -> bool {
    [".js", ".mjs", ".cjs", ".jsx"]
        .iter()
        .any(|suffix| specifier.ends_with(suffix))
}

pub(super) fn resolution_path_looks_types(path: Option<&str>) -> bool {
    let Some(path) = path else {
        return false;
    };
    let normalized = path.to_ascii_lowercase();
    normalized.ends_with(".d.ts")
        || normalized.contains("/types/")
        || normalized.contains("/@types/")
        || normalized.contains("index.d.ts")
}

pub(super) fn build_actionable_mismatch_hint(
    status: &str,
    focus: Option<&DoctorCompareFocus>,
    specgate_resolution: Option<&DoctorCompareResolutionOutput>,
    tsc_trace_resolution: Option<&DoctorCompareResolutionOutput>,
    missing_in_specgate: &[String],
    extra_in_specgate: &[String],
) -> Option<String> {
    if status != "mismatch" {
        return None;
    }

    let shared_guidance = "check tsconfig selection/baseUrl/paths, monorepo project references, `moduleResolution` condition sets, package `exports`, and symlink handling (`preserveSymlinks`)";

    let Some(_focus) = focus else {
        return Some(format!(
            "Edge sets differ. Re-run with `--from <file> --import <specifier>` for targeted diagnosis, then {shared_guidance}."
        ));
    };

    let Some(specgate_resolution) = specgate_resolution else {
        return Some(format!(
            "Focused parity mismatch detected; {shared_guidance}."
        ));
    };

    let Some(tsc_trace_resolution) = tsc_trace_resolution else {
        return Some(format!(
            "Focused parity mismatch detected; {shared_guidance}."
        ));
    };

    match (
        &specgate_resolution.result_kind,
        &tsc_trace_resolution.result_kind,
    ) {
        (TraceResultKind::FirstParty, TraceResultKind::FirstParty)
            if specgate_resolution.resolved_to != tsc_trace_resolution.resolved_to =>
        {
            Some(format!(
                "Both resolvers found first-party targets, but they disagree on the resolved file. Compare path alias precedence and project reference roots; then {shared_guidance}."
            ))
        }
        (
            TraceResultKind::Unresolvable | TraceResultKind::NotObserved,
            TraceResultKind::FirstParty,
        ) => Some(format!(
            "TypeScript resolved this import, but Specgate did not. Verify this command uses the same root tsconfig and project-reference graph; then {shared_guidance}."
        )),
        (
            TraceResultKind::FirstParty,
            TraceResultKind::Unresolvable | TraceResultKind::NotObserved,
        ) => Some(format!(
            "Specgate resolved a first-party edge that TypeScript did not report. Ensure the trace comes from the same build target and includes the importing file; then {shared_guidance}."
        )),
        (TraceResultKind::ThirdParty, TraceResultKind::FirstParty)
        | (TraceResultKind::FirstParty, TraceResultKind::ThirdParty) => Some(format!(
            "Resolver classification differs (first-party vs third-party). Inspect package `exports` conditions, path aliases, and symlinked workspace package links; then {shared_guidance}."
        )),
        _ if !missing_in_specgate.is_empty() || !extra_in_specgate.is_empty() => Some(format!(
            "Focused parity mismatch detected; {shared_guidance}."
        )),
        _ => Some(format!(
            "Focused parity mismatch detected; {shared_guidance}."
        )),
    }
}

pub(super) fn doctor_compare_beta_channel_enabled(config: &SpecConfig) -> bool {
    config.release_channel == ReleaseChannel::Beta
}
