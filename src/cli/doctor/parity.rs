use super::focus::DoctorCompareFocus;
use super::trace_types::{ParsedTraceData, TraceResultKind};
use super::types::{CompareStatus, DoctorCompareResolutionOutput, MismatchCategory};

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

    let mut source_edges = parsed_trace
        .edges
        .iter()
        .filter(|(from, _)| *from == focus.output.from)
        .peekable();

    if let Some((_, to)) = source_edges.next() {
        if source_edges.peek().is_none() {
            return DoctorCompareResolutionOutput {
                source: "tsc_trace".to_string(),
                result_kind: TraceResultKind::FirstParty,
                resolved_to: Some(to.clone()),
                package_name: None,
                trace: vec![
                    "trace did not carry import-specifier context; inferred from the sole edge from the same source file"
                        .to_string(),
                ],
            };
        }

        return DoctorCompareResolutionOutput {
            source: "tsc_trace".to_string(),
            result_kind: TraceResultKind::NotObserved,
            resolved_to: None,
            package_name: None,
            trace: vec![
                "trace did not carry import-specifier context and multiple edges share the same source file; refusing to guess which edge matches `--from/--import`"
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

pub(super) fn parity_verdict_for_status(status: CompareStatus) -> &'static str {
    match status {
        CompareStatus::Match => "MATCH",
        CompareStatus::Mismatch => "DIFF",
        CompareStatus::Skipped => "SKIPPED",
    }
}

pub(super) fn classify_doctor_compare_mismatch(
    status: CompareStatus,
    focus: Option<&DoctorCompareFocus>,
    specgate_resolution: Option<&DoctorCompareResolutionOutput>,
    tsc_trace_resolution: Option<&DoctorCompareResolutionOutput>,
    missing_in_specgate: &[String],
    extra_in_specgate: &[String],
) -> Option<MismatchCategory> {
    if status != CompareStatus::Mismatch {
        return None;
    }

    let Some(focus) = focus else {
        return Some(MismatchCategory::EdgeSetDiff);
    };

    let (Some(specgate_resolution), Some(tsc_trace_resolution)) =
        (specgate_resolution, tsc_trace_resolution)
    else {
        return Some(MismatchCategory::FocusedUnknown);
    };

    let category = match (
        &specgate_resolution.result_kind,
        &tsc_trace_resolution.result_kind,
    ) {
        (TraceResultKind::FirstParty, TraceResultKind::FirstParty)
            if specgate_resolution.resolved_to != tsc_trace_resolution.resolved_to =>
        {
            MismatchCategory::FocusedTargetMismatch
        }
        (
            TraceResultKind::Unresolvable | TraceResultKind::NotObserved,
            TraceResultKind::FirstParty,
        ) => MismatchCategory::FocusedSpecgateMissingResolution,
        (
            TraceResultKind::FirstParty,
            TraceResultKind::Unresolvable | TraceResultKind::NotObserved,
        ) => MismatchCategory::FocusedTscMissingResolution,
        (TraceResultKind::ThirdParty, TraceResultKind::FirstParty)
        | (TraceResultKind::FirstParty, TraceResultKind::ThirdParty) => {
            MismatchCategory::FocusedClassificationMismatch
        }
        _ if !missing_in_specgate.is_empty() || !extra_in_specgate.is_empty() => {
            MismatchCategory::FocusedEdgeSetDiff
        }
        _ => MismatchCategory::FocusedResolutionMismatch,
    };

    Some(match category {
        MismatchCategory::FocusedTargetMismatch => {
            classify_focus_mismatch_tag(focus, specgate_resolution, tsc_trace_resolution)
                .unwrap_or(category)
        }
        _ => category,
    })
}

pub(super) fn classify_focus_mismatch_tag(
    focus: &DoctorCompareFocus,
    specgate_resolution: &DoctorCompareResolutionOutput,
    tsc_trace_resolution: &DoctorCompareResolutionOutput,
) -> Option<MismatchCategory> {
    let specifier = focus.output.import_specifier.as_str();
    let is_relative = specifier.starts_with("./") || specifier.starts_with("../");

    if is_relative && matches_js_runtime_extension(specifier) {
        return Some(MismatchCategory::ExtensionAlias);
    }

    if !is_relative {
        if resolution_path_looks_types(specgate_resolution.resolved_to.as_deref())
            || resolution_path_looks_types(tsc_trace_resolution.resolved_to.as_deref())
        {
            return Some(MismatchCategory::ConditionNames);
        }

        if specifier.starts_with('@') || specifier.contains('/') {
            return Some(MismatchCategory::Paths);
        }

        return Some(MismatchCategory::Exports);
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
    status: CompareStatus,
    focus: Option<&DoctorCompareFocus>,
    specgate_resolution: Option<&DoctorCompareResolutionOutput>,
    tsc_trace_resolution: Option<&DoctorCompareResolutionOutput>,
    _missing_in_specgate: &[String],
    _extra_in_specgate: &[String],
) -> Option<String> {
    if status != CompareStatus::Mismatch {
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
        _ => Some(format!(
            "Focused parity mismatch detected; {shared_guidance}."
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::super::types::{DoctorCompareFocusOutput, FocusResolutionKind};
    use super::*;

    fn focus_with_edge(
        import_specifier: &str,
        edge: Option<(&str, &str)>,
        specgate_result_kind: TraceResultKind,
        specgate_resolved_to: Option<&str>,
    ) -> DoctorCompareFocus {
        DoctorCompareFocus {
            edge: edge.map(|(from, to)| (from.to_string(), to.to_string())),
            output: DoctorCompareFocusOutput {
                from: "src/app/main.ts".to_string(),
                import_specifier: import_specifier.to_string(),
                resolved_to: specgate_resolved_to.map(ToString::to_string),
                resolution_kind: match specgate_result_kind {
                    TraceResultKind::FirstParty => FocusResolutionKind::FirstParty,
                    TraceResultKind::ThirdParty => FocusResolutionKind::ThirdParty,
                    TraceResultKind::Unresolvable | TraceResultKind::NotObserved => {
                        FocusResolutionKind::Unresolvable
                    }
                },
                in_specgate_graph: edge.is_some(),
                specgate_trace: vec!["specgate".to_string()],
            },
            specgate_resolution: DoctorCompareResolutionOutput {
                source: "specgate".to_string(),
                result_kind: specgate_result_kind,
                resolved_to: specgate_resolved_to.map(ToString::to_string),
                package_name: None,
                trace: vec!["specgate".to_string()],
            },
        }
    }

    fn trace_data(
        edges: &[(&str, &str)],
        resolutions: Vec<super::super::trace_types::TraceResolutionRecord>,
    ) -> ParsedTraceData {
        ParsedTraceData {
            edges: edges
                .iter()
                .map(|(from, to)| (from.to_string(), to.to_string()))
                .collect::<BTreeSet<_>>(),
            resolutions,
        }
    }

    fn resolution(
        from: &str,
        import_specifier: &str,
        result_kind: TraceResultKind,
        resolved_to: Option<&str>,
    ) -> super::super::trace_types::TraceResolutionRecord {
        super::super::trace_types::TraceResolutionRecord {
            from: from.to_string(),
            import_specifier: import_specifier.to_string(),
            result_kind,
            resolved_to: resolved_to.map(ToString::to_string),
            package_name: None,
            trace: vec!["trace".to_string()],
        }
    }

    #[test]
    fn derive_tsc_focus_resolution_refuses_ambiguous_same_source_edge_fallback() {
        let focus = focus_with_edge(
            "@pkg/foo",
            Some(("src/app/main.ts", "src/core/index.ts")),
            TraceResultKind::FirstParty,
            Some("src/core/index.ts"),
        );
        let parsed_trace = trace_data(
            &[
                ("src/app/main.ts", "src/core/one.ts"),
                ("src/app/main.ts", "src/core/two.ts"),
            ],
            Vec::new(),
        );

        let resolution = derive_tsc_focus_resolution(&parsed_trace, &focus);

        assert_eq!(resolution.result_kind, TraceResultKind::NotObserved);
        assert!(resolution.resolved_to.is_none());
        assert!(
            resolution.trace[0].contains("refusing to guess"),
            "expected ambiguity guardrail, got {:?}",
            resolution.trace
        );
    }

    #[test]
    fn classify_doctor_compare_mismatch_preserves_classification_mismatch_for_package_imports() {
        let focus = focus_with_edge("left-pad", None, TraceResultKind::ThirdParty, None);
        let tsc_trace_resolution = DoctorCompareResolutionOutput {
            source: "tsc_trace".to_string(),
            result_kind: TraceResultKind::FirstParty,
            resolved_to: Some("src/core/index.ts".to_string()),
            package_name: None,
            trace: vec!["trace".to_string()],
        };

        let category = classify_doctor_compare_mismatch(
            CompareStatus::Mismatch,
            Some(&focus),
            Some(&focus.specgate_resolution),
            Some(&tsc_trace_resolution),
            &[],
            &[],
        );

        assert_eq!(
            category,
            Some(MismatchCategory::FocusedClassificationMismatch)
        );
    }

    #[test]
    fn classify_doctor_compare_mismatch_keeps_target_heuristics_for_package_imports() {
        let focus = focus_with_edge(
            "left-pad",
            None,
            TraceResultKind::FirstParty,
            Some("src/core/index.ts"),
        );
        let tsc_trace_resolution = DoctorCompareResolutionOutput {
            source: "tsc_trace".to_string(),
            result_kind: TraceResultKind::FirstParty,
            resolved_to: Some("src/core/alt.ts".to_string()),
            package_name: None,
            trace: vec!["trace".to_string()],
        };

        let category = classify_doctor_compare_mismatch(
            CompareStatus::Mismatch,
            Some(&focus),
            Some(&focus.specgate_resolution),
            Some(&tsc_trace_resolution),
            &[],
            &[],
        );

        assert_eq!(category, Some(MismatchCategory::Exports));
    }

    #[test]
    fn derive_tsc_focus_resolution_prefers_explicit_matching_resolution_record() {
        let focus = focus_with_edge(
            "../core/index",
            Some(("src/app/main.ts", "src/core/index.ts")),
            TraceResultKind::FirstParty,
            Some("src/core/index.ts"),
        );
        let parsed_trace = trace_data(
            &[("src/app/main.ts", "src/core/other.ts")],
            vec![resolution(
                "src/app/main.ts",
                "../core/index",
                TraceResultKind::FirstParty,
                Some("src/core/index.ts"),
            )],
        );

        let tsc_resolution = derive_tsc_focus_resolution(&parsed_trace, &focus);

        assert_eq!(tsc_resolution.result_kind, TraceResultKind::FirstParty);
        assert_eq!(
            tsc_resolution.resolved_to.as_deref(),
            Some("src/core/index.ts")
        );
    }
}
