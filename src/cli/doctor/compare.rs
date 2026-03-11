use crate::cli::{
    CliRunResult, EXIT_CODE_DOCTOR_MISMATCH, EXIT_CODE_PASS, prepare_analysis_context,
    resolve_against_root, runtime_error_json,
};
use crate::deterministic::normalize_repo_relative;

use super::DoctorCompareArgs;
use super::focus::{build_doctor_compare_focus, filter_edges_for_focus};
use super::parity::{
    build_actionable_mismatch_hint, classify_doctor_compare_mismatch, derive_tsc_focus_resolution,
    doctor_compare_beta_channel_enabled, parity_verdict_for_status,
};
use super::trace_io::{load_trace_source, write_structured_snapshot};
use super::trace_parser::parse_trace_data;
use super::types::{CompareStatus, DoctorCompareOutput};

pub(super) fn handle_doctor_compare(args: DoctorCompareArgs) -> CliRunResult {
    let prepared = match prepare_analysis_context(&args.common.project_root, None) {
        Ok(prepared) => prepared,
        Err(error) => return error,
    };
    let loaded = &prepared.loaded;
    let artifacts = &prepared.artifacts;

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
            status: CompareStatus::Skipped,
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
        CompareStatus::Match
    } else {
        CompareStatus::Mismatch
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
        status,
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

    let exit_code = if status == CompareStatus::Mismatch {
        EXIT_CODE_DOCTOR_MISMATCH
    } else {
        EXIT_CODE_PASS
    };

    CliRunResult::json(exit_code, &output)
}
