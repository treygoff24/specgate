use clap::{Args, ValueEnum};

use super::*;
use crate::policy::{
    PolicyDiffErrorEntry, PolicyDiffExit, PolicyDiffReport, PolicyDiffSummary,
    build_policy_diff_report, derive_policy_diff_exit, render_policy_diff_human,
    render_policy_diff_ndjson,
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

    let report = match build_policy_diff_report(&args.project_root, &args.base, &args.head) {
        Ok(report) => report,
        Err(error) => PolicyDiffReport::new(
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
    };

    let exit = derive_policy_diff_exit(&report);

    render_policy_diff_report(&report, format, exit)
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
        PolicyDiffFormat::Json => CliRunResult::json(exit.code(), report),
        PolicyDiffFormat::Ndjson => CliRunResult {
            exit_code: exit.code(),
            stdout: format!("{}\n", render_policy_diff_ndjson(report).join("\n")),
            stderr: String::new(),
        },
    }
}
