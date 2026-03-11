//! Check command integration surface.
//!
//! This module provides diff mode types for the check command.
//!
//! ## CLI Semantics (Wave 0 Contract Lock)
//!
//! - `--baseline-diff`: Output diff between current and baseline violations (preferred)
//! - `--baseline-new-only`: Show only new violations in diff output (requires --baseline-diff)
//! - `--since <git-ref>`: Git blast-radius mode - only check modules/importers affected since ref
//!
//! ### Deprecated Flags (aliased with warning)
//!
//! - `--diff`: Deprecated alias for `--baseline-diff`
//! - `--diff-new-only`: Deprecated alias for `--baseline-new-only`

use std::collections::BTreeMap;
use std::io::{IsTerminal, stdout};
use std::path::Path;
use std::path::PathBuf;
use std::time::Instant;

use clap::Args;

use crate::baseline::{
    ClassifyOptions, DEFAULT_BASELINE_PATH, classify_violations_with_options,
    load_optional_baseline,
};
use crate::build_info;
use crate::cli::{
    CliRunResult, EXIT_CODE_PASS, EXIT_CODE_POLICY_VIOLATIONS, build_workspace_packages_info,
    compute_governance_hashes, compute_telemetry_summary, load_project_for_analysis,
    normalize_repo_relative, prepare_analysis_for_loaded, project_fingerprint, record_timing,
    resolve_against_root, runtime_error_json,
};
use crate::policy::{self, ChangeClassification, PolicyDiffReport};
use crate::spec::{Severity, config::StaleBaselinePolicy};
use crate::verdict::{
    self, AnonymizedTelemetryEvent, GovernanceContext, TelemetryEventName, VerdictBuildOptions,
    VerdictIdentity, VerdictMetrics, VerdictStatus,
};
use crate::verdict::{VerdictBuildRequest, build_verdict_from_request};

// Used for expiry-aware classification (current_date)
use chrono::Local;

/// Output format for the check command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
#[value(rename_all = "lower")]
pub enum OutputFormat {
    /// Human-readable format with icons and summary.
    Human,
    /// Single JSON object with all violations.
    Json,
    /// Newline-delimited JSON (one violation per line).
    Ndjson,
    /// SARIF 2.1.0 format for code scanning tools.
    Sarif,
}

impl OutputFormat {
    /// Returns the effective output format.
    /// If a format is explicitly specified, use it.
    /// Otherwise, defaults to human if stdout is a TTY, json otherwise.
    pub fn effective_format(explicit: Option<Self>) -> Self {
        explicit.unwrap_or_else(|| {
            if stdout().is_terminal() {
                Self::Human
            } else {
                Self::Json
            }
        })
    }
}

/// Check command arguments with diff mode support.
#[derive(Debug, Clone, Args)]
pub struct CheckArgs {
    /// Project root containing code + specs + optional specgate.config.yml.
    #[arg(long, default_value = ".")]
    pub project_root: PathBuf,
    /// Output mode (`deterministic` or `metrics`).
    #[arg(long, value_enum, default_value_t = CheckOutputMode::Deterministic)]
    pub output_mode: CheckOutputMode,
    /// Deprecated alias for `--output-mode metrics`.
    #[arg(long)]
    pub metrics: bool,
    /// Baseline file path.
    #[arg(long, default_value = DEFAULT_BASELINE_PATH)]
    pub baseline: PathBuf,
    /// Disable baseline classification.
    #[arg(long)]
    pub no_baseline: bool,
    /// Enable telemetry emission for this run (opt-in).
    #[arg(long, conflicts_with = "no_telemetry")]
    pub telemetry: bool,
    /// Force telemetry disabled for this run.
    #[arg(long, conflicts_with = "telemetry")]
    pub no_telemetry: bool,

    // Output format
    /// Output format (`human`, `json`, `ndjson`, or `sarif`).
    /// Defaults to `human` if stdout is a TTY, `json` otherwise.
    #[arg(long, value_enum)]
    pub format: Option<OutputFormat>,

    // Baseline diff mode (preferred naming)
    /// Output diff between current and baseline violations.
    #[arg(long)]
    pub baseline_diff: bool,
    /// Show only new violations in diff output.
    #[arg(long, requires = "baseline_diff")]
    pub baseline_new_only: bool,

    // Git blast-radius mode
    /// Git reference for blast-radius mode. Only check modules and their importers
    /// that have changed since this ref (e.g., HEAD~1, main, abc123).
    #[arg(long, value_name = "GIT_REF")]
    pub since: Option<String>,

    /// Fail check when policy widening is detected between `--since` and `HEAD`.
    #[arg(long)]
    pub deny_widenings: bool,

    // Deprecated aliases (kept for backwards compatibility)
    /// Deprecated: Use --baseline-diff instead.
    #[arg(long, hide = true)]
    pub diff: bool,
    /// Deprecated: Use --baseline-new-only instead.
    #[arg(long, hide = true, requires = "diff")]
    pub diff_new_only: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
#[value(rename_all = "lower")]
pub enum CheckOutputMode {
    Deterministic,
    Metrics,
}

/// Diff mode configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffMode {
    /// No diff output.
    None,
    /// Show all violations with diff formatting.
    Full,
    /// Show only new violations in diff format.
    NewOnly,
}

impl CheckArgs {
    /// Returns true if deprecated diff flags are being used.
    pub fn uses_deprecated_diff_flags(&self) -> bool {
        self.diff || self.diff_new_only
    }

    /// Returns deprecation warning message if deprecated flags are used.
    pub fn deprecation_warning(&self) -> Option<String> {
        let mut warnings = Vec::new();

        if self.diff {
            warnings.push("--diff is deprecated; use --baseline-diff");
        }
        if self.diff_new_only {
            warnings.push("--diff-new-only is deprecated; use --baseline-new-only");
        }

        if warnings.is_empty() {
            None
        } else {
            Some(format!("warning: {}", warnings.join(", ")))
        }
    }
}

impl From<&CheckArgs> for DiffMode {
    fn from(args: &CheckArgs) -> Self {
        // Check both new and deprecated flags
        let wants_full = args.baseline_diff || args.diff;
        let wants_new_only = args.baseline_new_only || args.diff_new_only;

        if wants_new_only {
            DiffMode::NewOnly
        } else if wants_full {
            DiffMode::Full
        } else {
            DiffMode::None
        }
    }
}

#[derive(Debug, Clone, Default)]
struct DenyWideningsResult {
    has_widening: bool,
    widening_details: Vec<String>,
    limitations: Vec<String>,
}

fn evaluate_deny_widenings(args: &CheckArgs) -> Result<DenyWideningsResult, CliRunResult> {
    if !args.deny_widenings {
        return Ok(DenyWideningsResult::default());
    }

    let Some(base_ref) = args.since.as_deref() else {
        return Err(runtime_error_json(
            "governance",
            "--deny-widenings requires --since <git-ref>",
            vec!["provide --since <base-ref> (for example origin/main or HEAD~1)".to_string()],
        ));
    };

    let report = match policy::build_policy_diff_report(&args.project_root, base_ref, "HEAD") {
        Ok(report) => report,
        Err(error) => {
            return Err(runtime_error_json(
                "governance",
                "failed to evaluate policy widenings",
                vec![format!("{}: {}", error.code(), error.message())],
            ));
        }
    };

    if !report.errors.is_empty() {
        return Err(runtime_error_json(
            "governance",
            "failed to evaluate policy widenings",
            report
                .errors
                .iter()
                .map(|error| {
                    if let Some(spec_path) = &error.spec_path {
                        format!("{} [{}]: {}", error.code, spec_path, error.message)
                    } else {
                        format!("{}: {}", error.code, error.message)
                    }
                })
                .collect(),
        ));
    }

    let has_widening = report.summary.has_widening;
    let limitations = report.summary.limitations.clone();

    Ok(DenyWideningsResult {
        has_widening,
        widening_details: collect_widening_details(&report),
        limitations,
    })
}

fn collect_widening_details(report: &PolicyDiffReport) -> Vec<String> {
    let mut details = Vec::new();

    for diff in &report.diffs {
        for change in &diff.changes {
            if change.classification == ChangeClassification::Widening {
                details.push(format!(
                    "module={} field={} detail={}",
                    diff.module, change.field, change.detail
                ));
            }
        }
    }

    for change in &report.config_changes {
        if change.classification == ChangeClassification::Widening {
            details.push(format!(
                "config field={} before={} after={}",
                change.field_path, change.before, change.after
            ));
        }
    }

    details
}

pub(super) fn handle_check(args: CheckArgs) -> CliRunResult {
    // Emit deprecation warning if using deprecated flags
    let deprecation_warning = args.deprecation_warning();

    // Extract diff mode before processing
    let diff_mode = DiffMode::from(&args);

    // If diff mode is enabled, use the diff handler
    if diff_mode != DiffMode::None {
        let mut result = handle_check_with_diff(args, diff_mode);
        if let Some(warning) = deprecation_warning {
            result.stderr = format!("{warning}\n{}", result.stderr);
        }
        return result;
    }

    let mut timings = BTreeMap::new();
    let total_start = Instant::now();

    let load_start = Instant::now();
    let loaded = match load_project_for_analysis(&args.project_root) {
        Ok(loaded) => loaded,
        Err(error) => return error,
    };
    record_timing(&mut timings, "load_project", load_start);

    let deny_widenings = match evaluate_deny_widenings(&args) {
        Ok(result) => result,
        Err(error_result) => return error_result,
    };

    let analyze_start = Instant::now();
    let prepared = match prepare_analysis_for_loaded(loaded, args.since.as_deref()) {
        Ok(prepared) => prepared,
        Err(error) => return error,
    };
    record_timing(&mut timings, "analyze_project", analyze_start);
    let loaded = prepared.loaded;
    let artifacts = prepared.artifacts;
    let blast_radius = prepared.blast_radius;
    let baseline_start = Instant::now();
    let baseline = if args.no_baseline {
        None
    } else {
        let baseline_path = resolve_against_root(&loaded.project_root, &args.baseline);
        match load_optional_baseline(&baseline_path) {
            Ok(baseline) => baseline,
            Err(error) => {
                return runtime_error_json(
                    "baseline",
                    "failed to load baseline file",
                    vec![error.to_string()],
                );
            }
        }
    };
    record_timing(&mut timings, "load_baseline", baseline_start);

    // Filter violations by blast radius if specified
    let policy_violations = if let Some(radius) = &blast_radius {
        artifacts
            .policy_violations
            .iter()
            .filter(|v| {
                let from_file = v
                    .from_file
                    .to_str()
                    .map(|s| normalize_repo_relative(&loaded.project_root, Path::new(s)));
                let module = v.from_module.as_deref();
                radius.contains_file(from_file.as_deref().unwrap_or(""), module)
            })
            .cloned()
            .collect()
    } else {
        artifacts.policy_violations.clone()
    };

    let classify_start = Instant::now();
    let current_date = Local::now().format("%Y-%m-%d").to_string();
    let classification = classify_violations_with_options(
        &loaded.project_root,
        &policy_violations,
        baseline.as_ref(),
        &ClassifyOptions { current_date },
    );
    record_timing(&mut timings, "classify_baseline", classify_start);

    let governance = match compute_governance_hashes(&loaded) {
        Ok(governance) => governance,
        Err(error) => {
            return runtime_error_json(
                "governance",
                "failed to compute deterministic governance hashes",
                vec![error],
            );
        }
    };

    let include_metrics = args.metrics || args.output_mode == CheckOutputMode::Metrics;
    let metrics = if include_metrics {
        Some(VerdictMetrics {
            timings_ms: timings,
            total_ms: total_start.elapsed().as_millis(),
        })
    } else {
        None
    };

    let output_mode = if include_metrics {
        "metrics".to_string()
    } else if blast_radius.is_some() {
        "blast_radius".to_string()
    } else {
        "deterministic".to_string()
    };

    let mut spec_files_changed = Vec::new();
    if let Some(radius) = &blast_radius {
        spec_files_changed = radius
            .changed_files
            .iter()
            .filter(|f| f.ends_with(".spec.yml"))
            .cloned()
            .collect();
    }

    let telemetry_enabled = if args.no_telemetry {
        false
    } else if args.telemetry {
        true
    } else {
        loaded.config.telemetry
    };

    let config_hash_for_telemetry = governance.config_hash.clone();
    let spec_hash_for_telemetry = governance.spec_hash.clone();

    let telemetry_summary = compute_telemetry_summary(
        &classification.violations,
        artifacts.suppressed_violations,
        classification.stale_count,
        classification.expired_count,
    );
    let fail_on_stale =
        loaded.config.stale_baseline == StaleBaselinePolicy::Fail && classification.stale_count > 0;
    let telemetry_status = if telemetry_summary.new_error_violations > 0 || fail_on_stale {
        VerdictStatus::Fail
    } else {
        VerdictStatus::Pass
    };

    let telemetry_status = if deny_widenings.has_widening {
        VerdictStatus::Fail
    } else {
        telemetry_status
    };

    let telemetry_event = if telemetry_enabled {
        Some(AnonymizedTelemetryEvent {
            schema_version: "1".to_string(),
            event: TelemetryEventName::CheckCompleted,
            project_fingerprint: project_fingerprint(&loaded.project_root),
            config_hash: config_hash_for_telemetry.clone(),
            spec_hash: spec_hash_for_telemetry.clone(),
            status: telemetry_status,
            summary: telemetry_summary,
        })
    } else {
        None
    };

    let mut verdict = build_verdict_from_request(VerdictBuildRequest {
        project_root: &loaded.project_root,
        violations: &classification.violations,
        suppressed_violations: artifacts.suppressed_violations,
        metrics,
        identity: VerdictIdentity {
            tool_version: build_info::tool_version().to_string(),
            git_sha: build_info::git_sha().to_string(),
            config_hash: governance.config_hash,
            spec_hash: governance.spec_hash,
            output_mode,
            spec_files_changed,
            rule_deltas: deny_widenings.widening_details.clone(),
            policy_change_detected: deny_widenings.has_widening,
        },
        governance: GovernanceContext {
            stale_baseline_entries: classification.stale_count,
            expired_baseline_entries: classification.expired_count,
            rule_deltas: deny_widenings.widening_details.clone(),
            policy_change_detected: deny_widenings.has_widening,
        },
        options: VerdictBuildOptions {
            stale_baseline_policy: loaded.config.stale_baseline,
            telemetry: telemetry_event,
        },
    });

    if deny_widenings.has_widening {
        verdict.status = VerdictStatus::Fail;
    }

    // Wire workspace discovery into verdict (non-fatal — empty means None)
    verdict.workspace_packages =
        build_workspace_packages_info(&loaded.project_root, &loaded.config);

    // Wire edge classification into verdict
    verdict.edge_classification = Some(artifacts.edge_classification);
    verdict.edges = artifacts.verdict_edges;
    verdict.unresolved_edges = artifacts.unresolved_edges;

    let exit_code = match verdict.status {
        VerdictStatus::Pass => EXIT_CODE_PASS,
        VerdictStatus::Fail => EXIT_CODE_POLICY_VIOLATIONS,
    };

    // Determine output format and dispatch to appropriate formatter
    let format = OutputFormat::effective_format(args.format);
    let mut result = match format {
        OutputFormat::Human => CliRunResult {
            exit_code,
            stdout: format!("{}\n", verdict::format::format_verdict_human(&verdict)),
            stderr: String::new(),
        },
        OutputFormat::Json => CliRunResult::json(exit_code, &verdict),
        OutputFormat::Ndjson => CliRunResult {
            exit_code,
            stdout: verdict::format::format_verdict_ndjson(&verdict),
            stderr: String::new(),
        },
        OutputFormat::Sarif => CliRunResult {
            exit_code,
            stdout: verdict::format::format_verdict_sarif(&verdict),
            stderr: String::new(),
        },
    };

    if let Some(warning) = deprecation_warning {
        result.stderr = format!("{warning}\n");
    }

    if deny_widenings.has_widening && !deny_widenings.widening_details.is_empty() {
        let details = deny_widenings
            .widening_details
            .iter()
            .map(|detail| format!("  - {detail}"))
            .collect::<Vec<_>>()
            .join("\n");
        result
            .stderr
            .push_str(&format!("policy widenings detected:\n{details}\n"));
    }

    if !deny_widenings.limitations.is_empty() {
        let details = deny_widenings
            .limitations
            .iter()
            .map(|detail| format!("  - {detail}"))
            .collect::<Vec<_>>()
            .join("\n");
        result
            .stderr
            .push_str(&format!("policy diff limitations detected:\n{details}\n"));
    }

    result
}

/// Enhanced check handler with diff mode support.
///
/// This function provides diff mode output for comparing violations against baseline.
/// In diff mode, violations are formatted in a git-style diff format where:
/// - New violations are prefixed with `+`
/// - Baseline violations are prefixed with ` ` (space)
pub(super) fn handle_check_with_diff(args: CheckArgs, diff_mode: DiffMode) -> CliRunResult {
    let loaded = match load_project_for_analysis(&args.project_root) {
        Ok(loaded) => loaded,
        Err(error) => return error,
    };

    let deny_widenings = match evaluate_deny_widenings(&args) {
        Ok(result) => result,
        Err(error_result) => return error_result,
    };

    let prepared = match prepare_analysis_for_loaded(loaded, args.since.as_deref()) {
        Ok(prepared) => prepared,
        Err(error) => return error,
    };
    let loaded = prepared.loaded;
    let artifacts = prepared.artifacts;
    let blast_radius = prepared.blast_radius;

    let policy_violations = if let Some(radius) = &blast_radius {
        artifacts
            .policy_violations
            .iter()
            .filter(|v| {
                let from_file = v
                    .from_file
                    .to_str()
                    .map(|s| normalize_repo_relative(&loaded.project_root, Path::new(s)));
                let module = v.from_module.as_deref();
                radius.contains_file(from_file.as_deref().unwrap_or(""), module)
            })
            .cloned()
            .collect()
    } else {
        artifacts.policy_violations.clone()
    };

    let baseline = if args.no_baseline {
        None
    } else {
        let baseline_path = resolve_against_root(&loaded.project_root, &args.baseline);
        match load_optional_baseline(&baseline_path) {
            Ok(baseline) => baseline,
            Err(error) => {
                return runtime_error_json(
                    "baseline",
                    "failed to load baseline file",
                    vec![error.to_string()],
                );
            }
        }
    };

    let current_date = Local::now().format("%Y-%m-%d").to_string();
    let classification = classify_violations_with_options(
        &loaded.project_root,
        &policy_violations,
        baseline.as_ref(),
        &ClassifyOptions { current_date },
    );

    // Filter based on diff mode
    let filtered: Vec<_> = match diff_mode {
        DiffMode::NewOnly => classification
            .violations
            .iter()
            .filter(|v| matches!(v.disposition, verdict::ViolationDisposition::New))
            .cloned()
            .collect(),
        DiffMode::Full => classification.violations,
        DiffMode::None => {
            // This branch is unreachable because handle_check guards against
            // calling handle_check_with_diff when diff_mode is None.
            unreachable!("handle_check_with_diff called with DiffMode::None")
        }
    };

    // Format output using diff formatter
    let mut lines: Vec<String> = Vec::new();
    for entry in &filtered {
        lines.push(verdict::format::format_violation_diff(
            &loaded.project_root,
            entry,
        ));
    }

    // Compute stats for summary
    let stats = verdict::format::ViolationStats::from_violations(&filtered);
    lines.push(String::new());
    lines.push(format!("Summary: {}", stats.format_human()));

    // Add stale baseline entry count if non-zero
    if classification.stale_count > 0 {
        lines.push(format!(
            "Stale baseline entries: {} (consider pruning with `specgate baseline --refresh`)",
            classification.stale_count
        ));
        if loaded.config.stale_baseline == StaleBaselinePolicy::Fail {
            lines.push(
                "Stale baseline policy is `fail`; run `specgate baseline --refresh` after review."
                    .to_string(),
            );
        }
    }

    // Determine exit code based on new errors
    let has_new_errors = filtered.iter().any(|v| {
        matches!(v.disposition, verdict::ViolationDisposition::New)
            && v.violation.severity == Severity::Error
    });
    let stale_policy_failure =
        loaded.config.stale_baseline == StaleBaselinePolicy::Fail && classification.stale_count > 0;

    let exit_code = if has_new_errors || stale_policy_failure {
        EXIT_CODE_POLICY_VIOLATIONS
    } else {
        EXIT_CODE_PASS
    };

    // Add governance field for machine-readable output when stale policy triggers failure
    if stale_policy_failure {
        lines.push(String::new());
        lines.push("governance:".to_string());
        lines.push(format!(
            "  stale_baseline_policy: {}",
            loaded.config.stale_baseline.as_str()
        ));
        lines.push(format!(
            "  stale_baseline_entries: {}",
            classification.stale_count
        ));
    }

    if deny_widenings.has_widening && !deny_widenings.widening_details.is_empty() {
        lines.push(String::new());
        lines.push("Policy widenings detected:".to_string());
        for detail in &deny_widenings.widening_details {
            lines.push(format!("  - {detail}"));
        }
    }

    let stderr = if deny_widenings.limitations.is_empty() {
        String::new()
    } else {
        let details = deny_widenings
            .limitations
            .iter()
            .map(|detail| format!("  - {detail}"))
            .collect::<Vec<_>>()
            .join("\n");
        format!("policy diff limitations detected:\n{details}\n")
    };

    CliRunResult {
        exit_code: if deny_widenings.has_widening {
            EXIT_CODE_POLICY_VIOLATIONS
        } else {
            exit_code
        },
        stdout: lines.join("\n") + "\n",
        stderr,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_mode_from_args_none_by_default() {
        let args = CheckArgs {
            project_root: PathBuf::from("."),
            output_mode: CheckOutputMode::Deterministic,
            metrics: false,
            baseline: PathBuf::from(DEFAULT_BASELINE_PATH),
            no_baseline: false,
            telemetry: false,
            no_telemetry: false,
            format: None,
            baseline_diff: false,
            baseline_new_only: false,
            since: None,
            deny_widenings: false,
            diff: false,
            diff_new_only: false,
        };

        assert_eq!(DiffMode::from(&args), DiffMode::None);
        assert!(!args.uses_deprecated_diff_flags());
        assert!(args.deprecation_warning().is_none());
    }

    #[test]
    fn diff_mode_from_args_full_when_baseline_diff_set() {
        let args = CheckArgs {
            project_root: PathBuf::from("."),
            output_mode: CheckOutputMode::Deterministic,
            metrics: false,
            baseline: PathBuf::from(DEFAULT_BASELINE_PATH),
            no_baseline: false,
            telemetry: false,
            no_telemetry: false,
            format: None,
            baseline_diff: true,
            baseline_new_only: false,
            since: None,
            deny_widenings: false,
            diff: false,
            diff_new_only: false,
        };

        assert_eq!(DiffMode::from(&args), DiffMode::Full);
        assert!(!args.uses_deprecated_diff_flags());
    }

    #[test]
    fn diff_mode_from_args_new_only_when_both_set() {
        let args = CheckArgs {
            project_root: PathBuf::from("."),
            output_mode: CheckOutputMode::Deterministic,
            metrics: false,
            baseline: PathBuf::from(DEFAULT_BASELINE_PATH),
            no_baseline: false,
            telemetry: false,
            no_telemetry: false,
            format: None,
            baseline_diff: true,
            baseline_new_only: true,
            since: None,
            deny_widenings: false,
            diff: false,
            diff_new_only: false,
        };

        assert_eq!(DiffMode::from(&args), DiffMode::NewOnly);
    }

    #[test]
    fn deprecated_diff_flag_works_with_warning() {
        let args = CheckArgs {
            project_root: PathBuf::from("."),
            output_mode: CheckOutputMode::Deterministic,
            metrics: false,
            baseline: PathBuf::from(DEFAULT_BASELINE_PATH),
            no_baseline: false,
            telemetry: false,
            no_telemetry: false,
            format: None,
            baseline_diff: false,
            baseline_new_only: false,
            since: None,
            deny_widenings: false,
            diff: true,
            diff_new_only: false,
        };

        assert_eq!(DiffMode::from(&args), DiffMode::Full);
        assert!(args.uses_deprecated_diff_flags());
        let warning = args.deprecation_warning().unwrap();
        assert!(warning.contains("--diff is deprecated"));
        assert!(warning.contains("--baseline-diff"));
    }

    #[test]
    fn deprecated_new_only_flag_works_with_warning() {
        let args = CheckArgs {
            project_root: PathBuf::from("."),
            output_mode: CheckOutputMode::Deterministic,
            metrics: false,
            baseline: PathBuf::from(DEFAULT_BASELINE_PATH),
            no_baseline: false,
            telemetry: false,
            no_telemetry: false,
            format: None,
            baseline_diff: false,
            baseline_new_only: false,
            since: None,
            deny_widenings: false,
            diff: true,
            diff_new_only: true,
        };

        assert_eq!(DiffMode::from(&args), DiffMode::NewOnly);
        assert!(args.uses_deprecated_diff_flags());
        let warning = args.deprecation_warning().unwrap();
        assert!(warning.contains("--diff-new-only is deprecated"));
        assert!(warning.contains("--baseline-new-only"));
    }

    #[test]
    fn since_flag_is_optional_string() {
        let args = CheckArgs {
            project_root: PathBuf::from("."),
            output_mode: CheckOutputMode::Deterministic,
            metrics: false,
            baseline: PathBuf::from(DEFAULT_BASELINE_PATH),
            no_baseline: false,
            telemetry: false,
            no_telemetry: false,
            format: None,
            baseline_diff: false,
            baseline_new_only: false,
            since: Some("HEAD~1".to_string()),
            deny_widenings: false,
            diff: false,
            diff_new_only: false,
        };

        assert_eq!(args.since, Some("HEAD~1".to_string()));
    }

    #[test]
    fn output_format_effective_format_uses_explicit_when_provided() {
        assert_eq!(
            OutputFormat::effective_format(Some(OutputFormat::Human)),
            OutputFormat::Human
        );
        assert_eq!(
            OutputFormat::effective_format(Some(OutputFormat::Json)),
            OutputFormat::Json
        );
        assert_eq!(
            OutputFormat::effective_format(Some(OutputFormat::Ndjson)),
            OutputFormat::Ndjson
        );
        assert_eq!(
            OutputFormat::effective_format(Some(OutputFormat::Sarif)),
            OutputFormat::Sarif
        );
    }

    #[test]
    fn output_format_enum_values_match_clap() {
        // Verify the enum values are lowercase as expected by clap
        let human: OutputFormat = clap::ValueEnum::from_str("human", true).unwrap();
        let json: OutputFormat = clap::ValueEnum::from_str("json", true).unwrap();
        let ndjson: OutputFormat = clap::ValueEnum::from_str("ndjson", true).unwrap();
        let sarif: OutputFormat = clap::ValueEnum::from_str("sarif", true).unwrap();

        assert_eq!(human, OutputFormat::Human);
        assert_eq!(json, OutputFormat::Json);
        assert_eq!(ndjson, OutputFormat::Ndjson);
        assert_eq!(sarif, OutputFormat::Sarif);
    }
}
