use std::collections::BTreeSet;

use clap::{Args, ValueEnum};

use super::*;
use crate::policy::{
    ChangeClassification, ModulePolicyDiff, PolicyDiffErrorEntry, PolicyDiffExit, PolicyDiffReport,
    PolicyDiffSummary, classify_fail_closed_operations,
    classify_spec_snapshot_pairs_with_path_coverage, discover_and_load_spec_snapshots,
    render_policy_diff_human, render_policy_diff_ndjson,
};

/// Compare `.spec.yml` policy files across git refs and classify changes as widening,
/// narrowing, or structural.
#[derive(Debug, Clone, Args)]
pub(crate) struct PolicyDiffArgs {
    /// Project root containing the git repository to compare.
    #[arg(long, default_value = ".")]
    pub project_root: PathBuf,

    /// Base git ref for policy diffing.
    #[arg(long)]
    pub base: String,

    /// Head git ref for policy diffing.
    #[arg(long, default_value = "HEAD")]
    pub head: String,

    /// Output format (`human`, `json`, or `ndjson`).
    #[arg(long, value_enum, default_value_t = PolicyDiffFormat::Human)]
    pub format: PolicyDiffFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "lower")]
pub(crate) enum PolicyDiffFormat {
    Human,
    Json,
    Ndjson,
}

pub(super) fn handle_policy_diff(args: PolicyDiffArgs) -> CliRunResult {
    let format = args.format;

    let discovered =
        match discover_and_load_spec_snapshots(&args.project_root, &args.base, &args.head) {
            Ok(discovered) => discovered,
            Err(error) => {
                return render_policy_diff_report(
                    &PolicyDiffReport::new(
                        args.base,
                        args.head,
                        Vec::new(),
                        PolicyDiffSummary::default(),
                        vec![PolicyDiffErrorEntry {
                            code: error.code().to_string(),
                            message: error.message().to_string(),
                            spec_path: None,
                        }],
                    ),
                    format,
                    PolicyDiffExit::RuntimeError,
                );
            }
        };

    let mut diffs: Vec<ModulePolicyDiff> = Vec::new();
    diffs.extend(classify_fail_closed_operations(
        &discovered.discovered.fail_closed_operations,
    ));

    let mut semantic_diffs = classify_spec_snapshot_pairs_with_path_coverage(
        &args.project_root,
        &args.base,
        &args.head,
        &discovered.loaded.snapshots,
    );
    diffs.append(&mut semantic_diffs);

    let mut summary = summarize_policy_diffs(&diffs);
    if !discovered.loaded.errors.is_empty() {
        diffs.clear();
        summary = PolicyDiffSummary::default();
    }

    let report = PolicyDiffReport::new(
        args.base,
        args.head,
        diffs,
        summary,
        discovered.loaded.errors,
    );

    let exit = if !report.errors.is_empty() {
        PolicyDiffExit::RuntimeError
    } else if report.summary.has_widening {
        PolicyDiffExit::Widening
    } else {
        PolicyDiffExit::Clean
    };

    render_policy_diff_report(&report, format, exit)
}

fn summarize_policy_diffs(diffs: &[ModulePolicyDiff]) -> PolicyDiffSummary {
    let mut summary = PolicyDiffSummary::default();
    let mut modules = BTreeSet::new();
    let mut limitations = BTreeSet::new();

    for diff in diffs {
        modules.insert(diff.module.clone());

        for change in &diff.changes {
            match change.classification {
                ChangeClassification::Widening => {
                    summary.widening_changes += 1;
                    summary.has_widening = true;
                }
                ChangeClassification::Narrowing => {
                    summary.narrowing_changes += 1;
                }
                ChangeClassification::Structural => {
                    summary.structural_changes += 1;
                }
            }

            if change.detail.contains("path_coverage_unbounded_mvp") {
                limitations.insert("path_coverage_unbounded_mvp".to_string());
            }
        }
    }

    summary.modules_changed = modules.len();
    summary.limitations = limitations.into_iter().collect();

    summary
}

fn render_policy_diff_report(
    report: &PolicyDiffReport,
    format: PolicyDiffFormat,
    exit: PolicyDiffExit,
) -> CliRunResult {
    match format {
        PolicyDiffFormat::Human => CliRunResult {
            exit_code: exit.code(),
            stdout: format!("{}\n", render_policy_diff_human(report)),
            stderr: String::new(),
        },
        PolicyDiffFormat::Json => {
            let mut report = report.clone();
            report.sort_deterministic();
            CliRunResult::json(exit.code(), &report)
        }
        PolicyDiffFormat::Ndjson => CliRunResult {
            exit_code: exit.code(),
            stdout: format!("{}\n", render_policy_diff_ndjson(report).join("\n")),
            stderr: String::new(),
        },
    }
}
