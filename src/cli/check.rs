//! Check command integration surface.
//!
//! This module provides diff mode types for the check command.

use std::path::PathBuf;

use clap::Args;

use crate::baseline::DEFAULT_BASELINE_PATH;

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
    /// Output diff between current and baseline violations.
    #[arg(long)]
    pub diff: bool,
    /// Show only new violations in diff output.
    #[arg(long, requires = "diff")]
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

impl From<&CheckArgs> for DiffMode {
    fn from(args: &CheckArgs) -> Self {
        if args.diff_new_only {
            DiffMode::NewOnly
        } else if args.diff {
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
            diff: false,
            diff_new_only: false,
        };

        assert_eq!(DiffMode::from(&args), DiffMode::None);
    }

    #[test]
    fn diff_mode_from_args_full_when_diff_set() {
        let args = CheckArgs {
            project_root: PathBuf::from("."),
            output_mode: CheckOutputMode::Deterministic,
            metrics: false,
            baseline: PathBuf::from(DEFAULT_BASELINE_PATH),
            no_baseline: false,
            diff: true,
            diff_new_only: false,
        };

        assert_eq!(DiffMode::from(&args), DiffMode::Full);
    }

    #[test]
    fn diff_mode_from_args_new_only_when_both_set() {
        let args = CheckArgs {
            project_root: PathBuf::from("."),
            output_mode: CheckOutputMode::Deterministic,
            metrics: false,
            baseline: PathBuf::from(DEFAULT_BASELINE_PATH),
            no_baseline: false,
            diff: true,
            diff_new_only: true,
        };

        assert_eq!(DiffMode::from(&args), DiffMode::NewOnly);
    }
}
