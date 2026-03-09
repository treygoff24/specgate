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

use std::io::{IsTerminal, stdout};
use std::path::PathBuf;

use super::*;
use clap::Args;

use crate::baseline::DEFAULT_BASELINE_PATH;

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
    let loaded = match load_project(&args.project_root) {
        Ok(loaded) => loaded,
        Err(error) => {
            return runtime_error_json("config", "failed to load project", vec![error]);
        }
    };
    record_timing(&mut timings, "load_project", load_start);

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

    let blast_edge_pairs = match derive_blast_edge_pairs(&loaded, args.since.as_deref()) {
        Ok(edge_pairs) => edge_pairs,
        Err(error) => {
            return runtime_error_json("git", "failed to compute blast edge pairs", vec![error]);
        }
    };

    // Handle --since blast-radius mode - compute affected modules before analysis
    let blast_radius = match build_blast_radius(&loaded, args.since.as_deref(), &blast_edge_pairs) {
        Ok(radius) => radius,
        Err(error) => {
            return runtime_error_json("git", "failed to compute blast radius", vec![error]);
        }
    };

    // Compute affected modules from blast radius for contract rule scoping
    let affected_modules: Option<BTreeSet<String>> = blast_radius.as_ref().map(|radius| {
        radius
            .affected_modules
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>()
    });

    let analyze_start = Instant::now();
    let artifacts = match analyze_project(&loaded, affected_modules.as_ref()) {
        Ok(artifacts) => artifacts,
        Err(error) => {
            return runtime_error_json("runtime", "failed to analyze project", vec![error]);
        }
    };
    record_timing(&mut timings, "analyze_project", analyze_start);

    if !artifacts.layer_config_issues.is_empty() {
        return runtime_error_json(
            "config",
            "invalid enforce-layer rule configuration",
            artifacts.layer_config_issues,
        );
    }
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
    let (classified, stale_baseline_entries) =
        classify_violations_with_stale(&loaded.project_root, &policy_violations, baseline.as_ref());
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
        &classified,
        artifacts.suppressed_violations,
        stale_baseline_entries,
    );
    let fail_on_stale =
        loaded.config.stale_baseline == StaleBaselinePolicy::Fail && stale_baseline_entries > 0;
    let telemetry_status = if telemetry_summary.new_error_violations > 0 || fail_on_stale {
        VerdictStatus::Fail
    } else {
        VerdictStatus::Pass
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

    let mut verdict = build_verdict_with_options(
        &loaded.project_root,
        &classified,
        artifacts.suppressed_violations,
        metrics,
        VerdictIdentity {
            tool_version: build_info::tool_version().to_string(),
            git_sha: build_info::git_sha().to_string(),
            config_hash: governance.config_hash,
            spec_hash: governance.spec_hash,
            output_mode,
            spec_files_changed,
            rule_deltas: Vec::new(),
            policy_change_detected: false,
        },
        GovernanceContext {
            stale_baseline_entries,
            expired_baseline_entries: 0,
            rule_deltas: Vec::new(),
            policy_change_detected: false,
        },
        VerdictBuildOptions {
            stale_baseline_policy: loaded.config.stale_baseline,
            telemetry: telemetry_event,
        },
    );

    // Wire workspace discovery into verdict (non-fatal — empty means None)
    verdict.workspace_packages =
        build_workspace_packages_info(&loaded.project_root, &loaded.config);

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
    result
}

/// Enhanced check handler with diff mode support.
///
/// This function provides diff mode output for comparing violations against baseline.
/// In diff mode, violations are formatted in a git-style diff format where:
/// - New violations are prefixed with `+`
/// - Baseline violations are prefixed with ` ` (space)
pub(super) fn handle_check_with_diff(args: CheckArgs, diff_mode: DiffMode) -> CliRunResult {
    let loaded = match load_project(&args.project_root) {
        Ok(loaded) => loaded,
        Err(error) => {
            return runtime_error_json("config", "failed to load project", vec![error]);
        }
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

    let blast_edge_pairs = match derive_blast_edge_pairs(&loaded, args.since.as_deref()) {
        Ok(edge_pairs) => edge_pairs,
        Err(error) => {
            return runtime_error_json("git", "failed to compute blast edge pairs", vec![error]);
        }
    };

    // Handle --since blast-radius mode - compute affected modules before analysis
    let blast_radius = match build_blast_radius(&loaded, args.since.as_deref(), &blast_edge_pairs) {
        Ok(radius) => radius,
        Err(error) => {
            return runtime_error_json("git", "failed to compute blast radius", vec![error]);
        }
    };

    // Compute affected modules from blast radius for contract rule scoping
    let affected_modules: Option<BTreeSet<String>> = blast_radius.as_ref().map(|radius| {
        radius
            .affected_modules
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>()
    });

    let artifacts = match analyze_project(&loaded, affected_modules.as_ref()) {
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

    let (classified, stale_baseline_entries) =
        classify_violations_with_stale(&loaded.project_root, &policy_violations, baseline.as_ref());

    // Filter based on diff mode
    let filtered: Vec<_> = match diff_mode {
        DiffMode::NewOnly => classified
            .iter()
            .filter(|v| matches!(v.disposition, verdict::ViolationDisposition::New))
            .cloned()
            .collect(),
        DiffMode::Full => classified,
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
    if stale_baseline_entries > 0 {
        lines.push(format!(
            "Stale baseline entries: {stale_baseline_entries} (consider pruning with `specgate baseline --refresh`)"
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
        loaded.config.stale_baseline == StaleBaselinePolicy::Fail && stale_baseline_entries > 0;

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
            "  stale_baseline_entries: {stale_baseline_entries}"
        ));
    }

    CliRunResult {
        exit_code,
        stdout: lines.join("\n") + "\n",
        stderr: String::new(),
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
