use crate::deterministic::normalize_repo_relative;

use super::*;

pub(super) fn handle_doctor(args: DoctorArgs) -> CliRunResult {
    match args.command {
        Some(DoctorCommand::Compare(compare_args)) => handle_doctor_compare(compare_args),
        None => handle_doctor_overview(args.common),
    }
}

fn handle_doctor_overview(args: CommonProjectArgs) -> CliRunResult {
    let loaded = match load_project(&args.project_root) {
        Ok(loaded) => loaded,
        Err(error) => return runtime_error_json("config", "failed to load project", vec![error]),
    };

    let validation_errors = loaded.validation.errors().len();
    let validation_warnings = loaded.validation.warnings().len();

    if validation_errors > 0 {
        let details = loaded
            .validation
            .errors()
            .into_iter()
            .map(|issue| format!("{}: {}", issue.module, issue.message))
            .collect();
        return runtime_error_json(
            "validation",
            "spec validation failed; run `specgate validate` for details",
            details,
        );
    }

    let artifacts = match analyze_project(&loaded, None) {
        Ok(artifacts) => artifacts,
        Err(error) => {
            return runtime_error_json("runtime", "failed to analyze project", vec![error]);
        }
    };

    let overlaps = artifacts
        .module_map_overlaps
        .iter()
        .map(|overlap| DoctorOverlapOutput {
            file: normalize_repo_relative(&loaded.project_root, &overlap.file),
            selected_module: overlap.selected_module.clone(),
            matched_modules: overlap.matched_modules.clone(),
        })
        .collect::<Vec<_>>();

    let status = if artifacts.layer_config_issues.is_empty() {
        "ok".to_string()
    } else {
        "error".to_string()
    };

    let output = DoctorOutput {
        schema_version: "2.2".to_string(),
        status,
        spec_count: loaded.specs.len(),
        validation_errors,
        validation_warnings,
        graph_nodes: artifacts.graph_nodes,
        graph_edges: artifacts.graph_edges,
        parse_warning_count: artifacts.parse_warning_count,
        policy_violation_count: artifacts.policy_violations.len(),
        layer_config_issues: artifacts.layer_config_issues,
        module_map_overlaps: overlaps,
    };

    let exit_code = if output.status == "ok" {
        EXIT_CODE_PASS
    } else {
        EXIT_CODE_RUNTIME_ERROR
    };

    CliRunResult::json(exit_code, &output)
}

fn handle_doctor_compare(args: DoctorCompareArgs) -> CliRunResult {
    let loaded = match load_project(&args.common.project_root) {
        Ok(loaded) => loaded,
        Err(error) => return runtime_error_json("config", "failed to load project", vec![error]),
    };

    if loaded.validation.has_errors() {
        let details = loaded
            .validation
            .errors()
            .into_iter()
            .map(|issue| format!("{}: {}", issue.module, issue.message))
            .collect();
        return runtime_error_json(
            "validation",
            "spec validation failed; run `specgate validate` for details",
            details,
        );
    }

    let artifacts = match analyze_project(&loaded, None) {
        Ok(artifacts) => artifacts,
        Err(error) => {
            return runtime_error_json("runtime", "failed to analyze project", vec![error]);
        }
    };

    if !artifacts.layer_config_issues.is_empty() {
        return runtime_error_json(
            "config",
            "invalid enforce-layer rule configuration",
            artifacts.layer_config_issues,
        );
    }

    let focus = match build_doctor_compare_focus(&loaded, &artifacts, &args) {
        Ok(focus) => focus,
        Err(error) => {
            return runtime_error_json(
                "doctor.compare",
                "invalid compare focus options",
                vec![error],
            );
        }
    };

    let trace_source = match load_trace_source(&loaded.project_root, &args) {
        Ok(trace_source) => trace_source,
        Err(error) => {
            return runtime_error_json(
                "doctor.compare",
                "failed to load tsc parity trace",
                vec![error],
            );
        }
    };

    let legacy_trace_allowed = doctor_compare_beta_channel_enabled(&loaded.config);
    let compare_specgate_edges = filter_edges_for_focus(&artifacts.edge_pairs, focus.as_ref());
    let specgate_resolution = focus
        .as_ref()
        .map(|focus| focus.specgate_resolution.clone());
    let structured_snapshot_in = args.structured_snapshot_in.as_ref().map(|path| {
        normalize_repo_relative(
            &loaded.project_root,
            &resolve_against_root(&loaded.project_root, path),
        )
    });

    let Some(trace_source_payload) = trace_source.payload else {
        let output = DoctorCompareOutput {
            schema_version: "2.2".to_string(),
            status: "skipped".to_string(),
            parity_verdict: "SKIPPED".to_string(),
            parser_mode: args.parser_mode.as_str().to_string(),
            trace_parser: None,
            configured: trace_source.configured,
            reason: trace_source.reason,
            specgate_edge_count: compare_specgate_edges.len(),
            trace_edge_count: 0,
            missing_in_specgate: Vec::new(),
            extra_in_specgate: Vec::new(),
            mismatch_category: None,
            actionable_mismatch_hint: None,
            structured_snapshot_in,
            structured_snapshot_out: None,
            specgate_resolution,
            tsc_trace_resolution: None,
            focus: focus.as_ref().map(|focus| focus.output.clone()),
        };

        return CliRunResult::json(EXIT_CODE_PASS, &output);
    };

    let parsed_trace = match parse_trace_data(
        &loaded.project_root,
        &trace_source_payload,
        args.parser_mode,
        legacy_trace_allowed,
    ) {
        Ok(trace) => trace,
        Err(error) => {
            return runtime_error_json(
                "doctor.compare",
                "failed to parse trace edges",
                vec![error],
            );
        }
    };
    let parsed_trace_data = parsed_trace.data;

    let structured_snapshot_out = match &args.structured_snapshot_out {
        Some(output_path) => {
            match write_structured_snapshot(&loaded.project_root, output_path, &parsed_trace_data) {
                Ok(path) => Some(path),
                Err(error) => {
                    return runtime_error_json(
                        "doctor.compare",
                        "failed to write structured snapshot output",
                        vec![error],
                    );
                }
            }
        }
        None => None,
    };

    let compare_trace_edges = filter_edges_for_focus(&parsed_trace_data.edges, focus.as_ref());

    let missing_in_specgate = compare_trace_edges
        .difference(&compare_specgate_edges)
        .map(|(from, to)| format!("{from} -> {to}"))
        .collect::<Vec<_>>();

    let extra_in_specgate = compare_specgate_edges
        .difference(&compare_trace_edges)
        .map(|(from, to)| format!("{from} -> {to}"))
        .collect::<Vec<_>>();

    let status = if missing_in_specgate.is_empty() && extra_in_specgate.is_empty() {
        "match"
    } else {
        "mismatch"
    };

    let tsc_trace_resolution = focus
        .as_ref()
        .map(|focus| derive_tsc_focus_resolution(&parsed_trace_data, focus));
    let mismatch_category = classify_doctor_compare_mismatch(
        status,
        focus.as_ref(),
        specgate_resolution.as_ref(),
        tsc_trace_resolution.as_ref(),
        &missing_in_specgate,
        &extra_in_specgate,
    );

    let actionable_mismatch_hint = build_actionable_mismatch_hint(
        status,
        focus.as_ref(),
        specgate_resolution.as_ref(),
        tsc_trace_resolution.as_ref(),
        &missing_in_specgate,
        &extra_in_specgate,
    );

    let output = DoctorCompareOutput {
        schema_version: "2.2".to_string(),
        status: status.to_string(),
        parity_verdict: parity_verdict_for_status(status).to_string(),
        parser_mode: args.parser_mode.as_str().to_string(),
        trace_parser: Some(parsed_trace.parser_kind.as_str().to_string()),
        configured: true,
        reason: trace_source.reason,
        specgate_edge_count: compare_specgate_edges.len(),
        trace_edge_count: compare_trace_edges.len(),
        missing_in_specgate,
        extra_in_specgate,
        mismatch_category,
        actionable_mismatch_hint,
        structured_snapshot_in,
        structured_snapshot_out,
        specgate_resolution,
        tsc_trace_resolution,
        focus: focus.as_ref().map(|focus| focus.output.clone()),
    };

    let exit_code = if status == "mismatch" {
        EXIT_CODE_DOCTOR_MISMATCH
    } else {
        EXIT_CODE_PASS
    };

    CliRunResult::json(exit_code, &output)
}
