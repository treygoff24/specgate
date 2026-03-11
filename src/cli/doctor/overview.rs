use crate::cli::{
    CliRunResult, CommonProjectArgs, EXIT_CODE_PASS, EXIT_CODE_RUNTIME_ERROR, analyze_project,
    build_workspace_packages_info, load_project, runtime_error_json,
};
use crate::deterministic::normalize_repo_relative;

use super::canonical::canonical_import_findings;
use super::types::{DoctorOutput, DoctorOverlapOutput};

pub(super) fn handle_doctor_overview(args: CommonProjectArgs) -> CliRunResult {
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

    let workspace_packages = build_workspace_packages_info(&loaded.project_root, &loaded.config);
    let findings = match canonical_import_findings(&loaded) {
        Ok(findings) => findings,
        Err(error) => {
            return runtime_error_json(
                "runtime",
                "failed to evaluate doctor findings",
                vec![error],
            );
        }
    };

    let tsconfig_filename_override = if loaded.config.tsconfig_filename != "tsconfig.json" {
        Some(loaded.config.tsconfig_filename.clone())
    } else {
        None
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
        findings,
        workspace_packages,
        tsconfig_filename_override,
    };

    let exit_code = if output.status == "ok" {
        EXIT_CODE_PASS
    } else {
        EXIT_CODE_RUNTIME_ERROR
    };

    CliRunResult::json(exit_code, &output)
}
