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

use std::io::{stdout, IsTerminal};
use std::path::PathBuf;

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
    /// Output format (`human`, `json`, or `ndjson`).
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
    }

    #[test]
    fn output_format_enum_values_match_clap() {
        // Verify the enum values are lowercase as expected by clap
        let human: OutputFormat = clap::ValueEnum::from_str("human", true).unwrap();
        let json: OutputFormat = clap::ValueEnum::from_str("json", true).unwrap();
        let ndjson: OutputFormat = clap::ValueEnum::from_str("ndjson", true).unwrap();

        assert_eq!(human, OutputFormat::Human);
        assert_eq!(json, OutputFormat::Json);
        assert_eq!(ndjson, OutputFormat::Ndjson);
    }
}
