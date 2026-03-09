use clap::Args;
use serde::Serialize;

use crate::cli::{
    CliRunResult, CommonProjectArgs, EXIT_CODE_PASS, EXIT_CODE_POLICY_VIOLATIONS, load_project,
    runtime_error_json,
};
use crate::graph::discovery::discover_source_files;
use crate::spec::ownership::{OwnershipReport, validate_ownership};

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
#[value(rename_all = "lower")]
pub(crate) enum OwnershipOutputFormat {
    Human,
    Json,
}

#[derive(Debug, Clone, Args)]
pub(crate) struct DoctorOwnershipArgs {
    #[command(flatten)]
    pub(super) common: CommonProjectArgs,
    /// Output format: human or json
    #[arg(long, default_value = "human")]
    pub(super) format: OwnershipOutputFormat,
}

#[derive(Debug, Serialize)]
struct OwnershipOutput {
    schema_version: String,
    status: String,
    report: OwnershipReport,
}

pub(super) fn handle_doctor_ownership(args: DoctorOwnershipArgs) -> CliRunResult {
    let loaded = match load_project(&args.common.project_root) {
        Ok(loaded) => loaded,
        Err(error) => return runtime_error_json("config", "failed to load project", vec![error]),
    };

    let discovery = match discover_source_files(&loaded.project_root, &loaded.config.exclude) {
        Ok(d) => d,
        Err(error) => {
            return runtime_error_json(
                "discovery",
                "failed to discover source files",
                vec![error.to_string()],
            );
        }
    };

    let report = validate_ownership(&loaded.project_root, &loaded.specs, &discovery.files);

    let has_findings = !report.unclaimed_files.is_empty()
        || !report.overlapping_files.is_empty()
        || !report.orphaned_specs.is_empty()
        || !report.duplicate_module_ids.is_empty();

    let exit_code = if loaded.config.strict_ownership && has_findings {
        EXIT_CODE_POLICY_VIOLATIONS
    } else {
        EXIT_CODE_PASS
    };

    match args.format {
        OwnershipOutputFormat::Json => {
            let status = if has_findings { "findings" } else { "ok" }.to_string();
            let output = OwnershipOutput {
                schema_version: "1.0".to_string(),
                status,
                report,
            };
            CliRunResult::json(exit_code, &output)
        }
        OwnershipOutputFormat::Human => {
            let text = render_human(&report);
            CliRunResult {
                exit_code,
                stdout: text,
                stderr: String::new(),
            }
        }
    }
}

fn render_human(report: &OwnershipReport) -> String {
    let mut out = String::new();

    out.push_str("Ownership Report:\n");
    out.push_str(&format!("  Source files: {}\n", report.total_source_files));
    out.push_str(&format!("  Claimed: {}\n", report.claimed_files));
    out.push_str(&format!("  Unclaimed: {}\n", report.unclaimed_files.len()));

    if report.unclaimed_files.is_empty() {
        out.push_str("\nUnclaimed files: none\n");
    } else {
        out.push_str("\nUnclaimed files:\n");
        for f in &report.unclaimed_files {
            out.push_str(&format!("  {f}\n"));
        }
    }

    if report.overlapping_files.is_empty() {
        out.push_str("\nOverlapping ownership: none\n");
    } else {
        out.push_str("\nOverlapping ownership:\n");
        for entry in &report.overlapping_files {
            let claimants = entry.claimed_by.join(", ");
            out.push_str(&format!("  {} -> claimed by: {claimants}\n", entry.file));
        }
    }

    if report.orphaned_specs.is_empty() {
        out.push_str("\nOrphaned specs: none\n");
    } else {
        out.push_str("\nOrphaned specs:\n");
        for spec in &report.orphaned_specs {
            out.push_str(&format!(
                "  {} ({}) -> path \"{}\" matches 0 files\n",
                spec.module_id, spec.spec_path, spec.path_glob
            ));
        }
    }

    if report.duplicate_module_ids.is_empty() {
        out.push_str("\nDuplicate module IDs: none\n");
    } else {
        out.push_str("\nDuplicate module IDs:\n");
        for dup in &report.duplicate_module_ids {
            let paths = dup.spec_paths.join(", ");
            out.push_str(&format!("  {} -> {paths}\n", dup.module_id));
        }
    }

    out
}