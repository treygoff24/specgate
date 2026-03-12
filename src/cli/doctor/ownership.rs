use clap::Args;
use serde::Serialize;

use crate::cli::{
    CliRunResult, CommonProjectArgs, EXIT_CODE_PASS, EXIT_CODE_POLICY_VIOLATIONS, load_project,
    runtime_error_json,
};
use crate::graph::discovery::discover_source_files;
use crate::spec::config::StrictOwnershipLevel;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct OwnershipFindingSummary {
    has_error_findings: bool,
    has_warning_findings: bool,
}

impl OwnershipFindingSummary {
    fn from_report(report: &OwnershipReport) -> Self {
        Self {
            has_error_findings: !report.duplicate_module_ids.is_empty()
                || !report.invalid_globs.is_empty()
                || !report.contradictory_globs.is_empty(),
            has_warning_findings: !report.unclaimed_files.is_empty()
                || !report.overlapping_files.is_empty()
                || !report.orphaned_specs.is_empty(),
        }
    }

    fn has_any_findings(self) -> bool {
        self.has_error_findings || self.has_warning_findings
    }

    fn should_gate(self, level: StrictOwnershipLevel) -> bool {
        match level {
            StrictOwnershipLevel::Errors => self.has_error_findings,
            StrictOwnershipLevel::Warnings => self.has_any_findings(),
        }
    }
}

fn is_ownership_validation_issue(message: &str) -> bool {
    message.contains("duplicate module") || message.contains("invalid boundaries.path glob pattern")
}

pub(super) fn handle_doctor_ownership(args: DoctorOwnershipArgs) -> CliRunResult {
    let loaded = match load_project(&args.common.project_root) {
        Ok(loaded) => loaded,
        Err(error) => return runtime_error_json("config", "failed to load project", vec![error]),
    };

    // Ownership-specific validation issues can still be rendered by the ownership
    // report path. Other spec validation failures still block analysis.
    if loaded.validation.has_errors() {
        let (ownership_errors, blocking_errors): (Vec<_>, Vec<_>) = loaded
            .validation
            .errors()
            .into_iter()
            .partition(|issue| is_ownership_validation_issue(&issue.message));

        if !blocking_errors.is_empty() {
            let details = blocking_errors
                .into_iter()
                .map(|issue| format!("{}: {}", issue.module, issue.message))
                .collect();
            return runtime_error_json(
                "validation",
                "spec validation failed; run `specgate validate` for details",
                details,
            );
        }

        let _ = ownership_errors;
    }

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
    let findings = OwnershipFindingSummary::from_report(&report);
    let has_findings = findings.has_any_findings();

    let exit_code = if loaded.config.strict_ownership
        && findings.should_gate(loaded.config.strict_ownership_level)
    {
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

    if report.invalid_globs.is_empty() {
        out.push_str("\nInvalid globs: none\n");
    } else {
        out.push_str("\nInvalid globs:\n");
        for ig in &report.invalid_globs {
            out.push_str(&format!(
                "  {} -> pattern \"{}\" error: {}\n",
                ig.module_id, ig.pattern, ig.error
            ));
        }
    }

    if report.contradictory_globs.is_empty() {
        out.push_str("\nContradictory ownership globs: none\n");
    } else {
        out.push_str("\nContradictory ownership globs:\n");
        for entry in &report.contradictory_globs {
            out.push_str(&format!(
                "  \"{}\" ({}, {}) vs \"{}\" ({}, {})\n",
                entry.glob_a,
                entry.module_a,
                entry.spec_path_a,
                entry.glob_b,
                entry.module_b,
                entry.spec_path_b,
            ));
            out.push_str(&format!("    {}\n", entry.description));
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use crate::cli::test_support::write_file;
    use crate::cli::{EXIT_CODE_PASS, EXIT_CODE_POLICY_VIOLATIONS, run};

    fn parse_json(source: &str) -> serde_json::Value {
        serde_json::from_str(source).expect("valid json")
    }

    #[test]
    fn doctor_ownership_detects_contradictory_globs_json() {
        let temp = TempDir::new().expect("tempdir");
        write_file(
            temp.path(),
            "modules/api.spec.yml",
            "version: \"2.2\"\nmodule: api\nboundaries:\n  path: src/shared/**\nconstraints: []\n",
        );
        write_file(
            temp.path(),
            "modules/ui.spec.yml",
            "version: \"2.2\"\nmodule: ui\nboundaries:\n  path: src/shared/**\nconstraints: []\n",
        );
        write_file(
            temp.path(),
            "specgate.config.yml",
            "spec_dirs:\n  - modules\nexclude: []\ntest_patterns: []\n",
        );
        write_file(temp.path(), "src/shared/utils.ts", "export const x = 1;\n");

        let result = run([
            "specgate",
            "doctor",
            "ownership",
            "--project-root",
            temp.path().to_str().expect("utf8 path"),
            "--format",
            "json",
        ]);

        assert_eq!(result.exit_code, EXIT_CODE_PASS);
        let output = parse_json(&result.stdout);
        assert_eq!(output["status"], "findings");
        let contradictory = output["report"]["contradictory_globs"]
            .as_array()
            .expect("contradictory_globs array");
        assert_eq!(
            contradictory.len(),
            1,
            "expected 1 contradictory glob pair: {contradictory:?}"
        );
        let entry = &contradictory[0];
        assert_eq!(entry["glob_a"], "src/shared/**");
        assert_eq!(entry["glob_b"], "src/shared/**");
        assert!(
            entry["description"]
                .as_str()
                .expect("description")
                .contains("overlapping ownership")
        );
    }

    #[test]
    fn doctor_ownership_contradictory_globs_gates_with_strict_ownership() {
        let temp = TempDir::new().expect("tempdir");
        write_file(
            temp.path(),
            "modules/api.spec.yml",
            "version: \"2.2\"\nmodule: api\nboundaries:\n  path: src/shared/**\nconstraints: []\n",
        );
        write_file(
            temp.path(),
            "modules/ui.spec.yml",
            "version: \"2.2\"\nmodule: ui\nboundaries:\n  path: src/shared/**\nconstraints: []\n",
        );
        write_file(
            temp.path(),
            "specgate.config.yml",
            "spec_dirs:\n  - modules\nexclude: []\ntest_patterns: []\nstrict_ownership: true\nstrict_ownership_level: errors\n",
        );
        write_file(temp.path(), "src/shared/utils.ts", "export const x = 1;\n");

        let result = run([
            "specgate",
            "doctor",
            "ownership",
            "--project-root",
            temp.path().to_str().expect("utf8 path"),
            "--format",
            "json",
        ]);

        assert_eq!(
            result.exit_code, EXIT_CODE_POLICY_VIOLATIONS,
            "contradictory globs should gate when strict_ownership + errors level"
        );
    }

    #[test]
    fn doctor_ownership_human_output_shows_contradictory_globs() {
        let temp = TempDir::new().expect("tempdir");
        write_file(
            temp.path(),
            "modules/api.spec.yml",
            "version: \"2.2\"\nmodule: api\nboundaries:\n  path: src/shared/**\nconstraints: []\n",
        );
        write_file(
            temp.path(),
            "modules/ui.spec.yml",
            "version: \"2.2\"\nmodule: ui\nboundaries:\n  path: src/shared/**\nconstraints: []\n",
        );
        write_file(
            temp.path(),
            "specgate.config.yml",
            "spec_dirs:\n  - modules\nexclude: []\ntest_patterns: []\n",
        );
        write_file(temp.path(), "src/shared/utils.ts", "export const x = 1;\n");

        let result = run([
            "specgate",
            "doctor",
            "ownership",
            "--project-root",
            temp.path().to_str().expect("utf8 path"),
            "--format",
            "human",
        ]);

        assert_eq!(result.exit_code, EXIT_CODE_PASS);
        assert!(
            result.stdout.contains("Contradictory ownership globs:"),
            "human output should contain contradictory section header"
        );
        assert!(
            result.stdout.contains("src/shared/**"),
            "human output should show the glob pattern"
        );
        assert!(
            result.stdout.contains("overlapping ownership"),
            "human output should include description"
        );
    }

    #[test]
    fn doctor_ownership_no_contradictions_clean_project() {
        let temp = TempDir::new().expect("tempdir");
        write_file(
            temp.path(),
            "modules/api.spec.yml",
            "version: \"2.2\"\nmodule: api\nboundaries:\n  path: src/api/**\nconstraints: []\n",
        );
        write_file(
            temp.path(),
            "modules/ui.spec.yml",
            "version: \"2.2\"\nmodule: ui\nboundaries:\n  path: src/ui/**\nconstraints: []\n",
        );
        write_file(
            temp.path(),
            "specgate.config.yml",
            "spec_dirs:\n  - modules\nexclude: []\ntest_patterns: []\n",
        );
        write_file(temp.path(), "src/api/index.ts", "export const api = 1;\n");
        write_file(temp.path(), "src/ui/button.tsx", "export const btn = 1;\n");

        let result = run([
            "specgate",
            "doctor",
            "ownership",
            "--project-root",
            temp.path().to_str().expect("utf8 path"),
            "--format",
            "json",
        ]);

        assert_eq!(result.exit_code, EXIT_CODE_PASS);
        let output = parse_json(&result.stdout);
        assert_eq!(output["status"], "ok");
        // contradictory_globs should be absent (skip_serializing_if = empty)
        assert!(
            output["report"]["contradictory_globs"].is_null(),
            "contradictory_globs should not appear in JSON when empty"
        );
    }

    #[test]
    fn doctor_ownership_structural_overlap_detected_without_files() {
        let temp = TempDir::new().expect("tempdir");
        // src/** structurally overlaps with src/api/**
        write_file(
            temp.path(),
            "modules/wide.spec.yml",
            "version: \"2.2\"\nmodule: wide\nboundaries:\n  path: src/**\nconstraints: []\n",
        );
        write_file(
            temp.path(),
            "modules/narrow.spec.yml",
            "version: \"2.2\"\nmodule: narrow\nboundaries:\n  path: src/api/**\nconstraints: []\n",
        );
        write_file(
            temp.path(),
            "specgate.config.yml",
            "spec_dirs:\n  - modules\nexclude: []\ntest_patterns: []\n",
        );
        // No source files at all — structural detection should still catch it.

        let result = run([
            "specgate",
            "doctor",
            "ownership",
            "--project-root",
            temp.path().to_str().expect("utf8 path"),
            "--format",
            "json",
        ]);

        let output = parse_json(&result.stdout);
        let contradictory = output["report"]["contradictory_globs"]
            .as_array()
            .expect("contradictory_globs array");
        assert_eq!(
            contradictory.len(),
            1,
            "structural overlap should be detected even without source files: {contradictory:?}"
        );
    }
}
