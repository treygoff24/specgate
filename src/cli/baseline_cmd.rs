use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use chrono::{Duration, Local, NaiveDate};
use clap::{Args, Subcommand, ValueEnum};
use serde::Serialize;

use crate::baseline::audit::{AuditReport, audit_baseline};
use crate::baseline::{
    BASELINE_FILE_VERSION, BaselineEntry, BaselineFile, BaselineGeneratedFrom,
    DEFAULT_BASELINE_PATH, baseline_identity, build_baseline_with_metadata, is_entry_expired,
    load_optional_baseline, normalize_baseline_file, refresh_baseline_with_metadata,
    write_baseline,
};
use crate::cli::{
    BaselineOutput, CliRunResult, CommonProjectArgs, EXIT_CODE_PASS, EXIT_CODE_POLICY_VIOLATIONS,
    PreparedAnalysisContext, compute_governance_hashes, normalize_repo_relative,
    prepare_analysis_context, resolve_against_root, runtime_error_json,
};
use crate::verdict::{PolicyViolation, sort_policy_violations};
use crate::{build_info, spec};

const CLI_SCHEMA_VERSION: &str = "2.2";

#[derive(Debug, Clone, Args)]
pub(crate) struct BaselineArgs {
    #[command(subcommand)]
    command: Option<BaselineCommand>,
    #[command(flatten)]
    legacy: BaselineLegacyArgs,
}

#[derive(Debug, Clone, Args)]
struct BaselineLegacyArgs {
    #[command(flatten)]
    common: CommonProjectArgs,
    /// Output baseline path.
    #[arg(long, default_value = DEFAULT_BASELINE_PATH)]
    output: PathBuf,
    /// Rebuild baseline from current violations (prunes stale entries, re-sorts, dedupes, and resets generated_from metadata to current tool version and git SHA).
    #[arg(long)]
    refresh: bool,
}

#[derive(Debug, Clone, Subcommand)]
enum BaselineCommand {
    /// Generate a baseline file for current violations.
    Generate(BaselineGenerateArgs),
    /// Add matching current violations to the baseline file.
    Add(BaselineAddArgs),
    /// List baseline entries with optional filters.
    List(BaselineListArgs),
    /// Audit baseline metadata coverage and expiry health.
    Audit(BaselineAuditArgs),
}

#[derive(Debug, Clone, Args)]
struct BaselineGenerateArgs {
    #[command(flatten)]
    common: CommonProjectArgs,
    /// Output baseline path.
    #[arg(long, default_value = DEFAULT_BASELINE_PATH)]
    output: PathBuf,
    /// Rebuild baseline from current violations.
    #[arg(long)]
    refresh: bool,
}

#[derive(Debug, Clone, Args)]
struct BaselineAddArgs {
    #[command(flatten)]
    common: CommonProjectArgs,
    /// Baseline path to update.
    #[arg(long, default_value = DEFAULT_BASELINE_PATH)]
    baseline: PathBuf,
    /// Rule ID to match from current violations.
    #[arg(long)]
    rule: String,
    /// Source module to match from current violations.
    #[arg(long)]
    from_module: String,
    /// Optional destination module filter.
    #[arg(long)]
    to_module: Option<String>,
    /// Optional normalized source file filter.
    #[arg(long)]
    from_file: Option<String>,
    /// Optional normalized destination file filter.
    #[arg(long)]
    to_file: Option<String>,
    /// Owner responsible for the new baseline entries.
    #[arg(long)]
    owner: Option<String>,
    /// Reason for suppressing the new baseline entries.
    #[arg(long)]
    reason: Option<String>,
    /// Optional expiry date in YYYY-MM-DD format.
    #[arg(long)]
    expires_at: Option<String>,
}

#[derive(Debug, Clone, Args)]
struct BaselineListArgs {
    #[command(flatten)]
    common: CommonProjectArgs,
    /// Baseline path to inspect.
    #[arg(long, default_value = DEFAULT_BASELINE_PATH)]
    baseline: PathBuf,
    /// Filter to a specific owner.
    #[arg(long)]
    owner: Option<String>,
    /// Show only expired entries.
    #[arg(long)]
    expired: bool,
    /// Show only entries expiring within the given number of days.
    #[arg(long)]
    expiring_within: Option<u32>,
    /// Group rendered output by a stable field.
    #[arg(long)]
    group_by: Option<BaselineGroupBy>,
    /// Output format.
    #[arg(long, default_value = "human")]
    format: BaselineDisplayFormat,
}

#[derive(Debug, Clone, Args)]
struct BaselineAuditArgs {
    #[command(flatten)]
    common: CommonProjectArgs,
    /// Baseline path to inspect.
    #[arg(long, default_value = DEFAULT_BASELINE_PATH)]
    baseline: PathBuf,
    /// Output format.
    #[arg(long, default_value = "human")]
    format: BaselineDisplayFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum BaselineDisplayFormat {
    Human,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum BaselineGroupBy {
    Owner,
    Rule,
}

#[derive(Debug, Serialize)]
struct BaselineAddOutput {
    schema_version: String,
    status: String,
    baseline_path: String,
    matched_violation_count: usize,
    added_count: usize,
    entry_count: usize,
}

#[derive(Debug, Serialize)]
struct BaselineListOutput {
    schema_version: String,
    status: String,
    baseline_path: String,
    entry_count: usize,
    filtered_count: usize,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    groups: BTreeMap<String, usize>,
    entries: Vec<BaselineEntry>,
}

#[derive(Debug, Serialize)]
struct BaselineAuditOutput {
    schema_version: String,
    status: String,
    baseline_path: String,
    require_metadata: bool,
    metadata_gaps: bool,
    report: AuditReport,
}

struct BaselineProjectContext {
    prepared: PreparedAnalysisContext,
    generated_from: BaselineGeneratedFrom,
}

impl From<BaselineLegacyArgs> for BaselineGenerateArgs {
    fn from(args: BaselineLegacyArgs) -> Self {
        Self {
            common: args.common,
            output: args.output,
            refresh: args.refresh,
        }
    }
}

pub(super) fn handle_baseline(args: BaselineArgs) -> CliRunResult {
    match args.command {
        Some(BaselineCommand::Generate(args)) => handle_baseline_generate(args),
        Some(BaselineCommand::Add(args)) => handle_baseline_add(args),
        Some(BaselineCommand::List(args)) => handle_baseline_list(args),
        Some(BaselineCommand::Audit(args)) => handle_baseline_audit(args),
        None => handle_baseline_generate(args.legacy.into()),
    }
}

fn handle_baseline_generate(args: BaselineGenerateArgs) -> CliRunResult {
    let context = match load_baseline_project_context(&args.common) {
        Ok(context) => context,
        Err(error) => return error,
    };

    let baseline_path = resolve_against_root(&context.prepared.loaded.project_root, &args.output);
    let (baseline, stale_entries_pruned) = if args.refresh {
        let existing = match load_optional_baseline(&baseline_path) {
            Ok(existing) => existing,
            Err(error) => {
                return runtime_error_json(
                    "baseline",
                    "failed to load baseline file for refresh",
                    vec![error.to_string()],
                );
            }
        };

        if let Some(existing) = existing.as_ref() {
            let refreshed = refresh_baseline_with_metadata(
                &context.prepared.loaded.project_root,
                &context.prepared.artifacts.policy_violations,
                Some(existing),
                context.generated_from.clone(),
            );
            (refreshed.baseline, refreshed.stale_entries_pruned)
        } else {
            (
                build_baseline_with_metadata(
                    &context.prepared.loaded.project_root,
                    &context.prepared.artifacts.policy_violations,
                    context.generated_from.clone(),
                ),
                0usize,
            )
        }
    } else {
        (
            build_baseline_with_metadata(
                &context.prepared.loaded.project_root,
                &context.prepared.artifacts.policy_violations,
                context.generated_from,
            ),
            0usize,
        )
    };

    if let Err(error) = write_baseline(&baseline_path, &baseline) {
        return runtime_error_json(
            "baseline",
            "failed to write baseline file",
            vec![error.to_string()],
        );
    }

    let output = BaselineOutput {
        schema_version: CLI_SCHEMA_VERSION.to_string(),
        status: "ok".to_string(),
        baseline_path: normalize_repo_relative(
            &context.prepared.loaded.project_root,
            &baseline_path,
        ),
        entry_count: baseline.entries.len(),
        source_violation_count: context.prepared.artifacts.policy_violations.len(),
        refreshed: args.refresh,
        stale_entries_pruned,
    };

    CliRunResult::json(EXIT_CODE_PASS, &output)
}

fn handle_baseline_add(args: BaselineAddArgs) -> CliRunResult {
    let context = match load_baseline_project_context(&args.common) {
        Ok(context) => context,
        Err(error) => return error,
    };

    if context.prepared.loaded.config.baseline.require_metadata
        && (!has_non_blank_arg(args.owner.as_deref()) || !has_non_blank_arg(args.reason.as_deref()))
    {
        return runtime_error_json(
            "baseline",
            "baseline metadata is required by config; pass --owner and --reason",
            Vec::new(),
        );
    }

    let baseline_path = resolve_against_root(&context.prepared.loaded.project_root, &args.baseline);
    let mut baseline = match load_optional_baseline(&baseline_path) {
        Ok(Some(existing)) => existing,
        Ok(None) => BaselineFile {
            version: BASELINE_FILE_VERSION.to_string(),
            generated_from: context.generated_from.clone(),
            entries: Vec::new(),
        },
        Err(error) => {
            return runtime_error_json(
                "baseline",
                "failed to load baseline file",
                vec![error.to_string()],
            );
        }
    };

    let matching_violations = filter_matching_violations(
        &context.prepared.loaded.project_root,
        &context.prepared.artifacts.policy_violations,
        &args,
    );

    if matching_violations.is_empty() {
        return runtime_error_json(
            "baseline",
            "no current violations matched baseline add filters",
            vec![
                format!("rule={}", args.rule),
                format!("from_module={}", args.from_module),
            ],
        );
    }

    let mut additions = build_baseline_with_metadata(
        &context.prepared.loaded.project_root,
        &matching_violations,
        context.generated_from.clone(),
    )
    .entries;
    for entry in &mut additions {
        entry.owner = args.owner.clone();
        entry.reason = args.reason.clone();
        entry.expires_at = args.expires_at.clone();
    }

    let matched_violation_count = additions.len();
    let existing_identities = baseline
        .entries
        .iter()
        .map(baseline_identity)
        .collect::<BTreeSet<_>>();
    additions.retain(|entry| !existing_identities.contains(&baseline_identity(entry)));
    let added_count = additions.len();

    if added_count > 0 {
        baseline.version = BASELINE_FILE_VERSION.to_string();
        baseline.generated_from = context.generated_from;
        baseline.entries.extend(additions);

        if let Err(error) = write_baseline(&baseline_path, &baseline) {
            return runtime_error_json(
                "baseline",
                "failed to write baseline file",
                vec![error.to_string()],
            );
        }

        baseline = match load_optional_baseline(&baseline_path) {
            Ok(Some(written)) => written,
            Ok(None) => {
                return runtime_error_json(
                    "baseline",
                    "baseline file disappeared after write",
                    vec![normalize_repo_relative(
                        &context.prepared.loaded.project_root,
                        &baseline_path,
                    )],
                );
            }
            Err(error) => {
                return runtime_error_json(
                    "baseline",
                    "failed to reload baseline file",
                    vec![error.to_string()],
                );
            }
        };
    }

    let output = BaselineAddOutput {
        schema_version: CLI_SCHEMA_VERSION.to_string(),
        status: "ok".to_string(),
        baseline_path: normalize_repo_relative(
            &context.prepared.loaded.project_root,
            &baseline_path,
        ),
        matched_violation_count,
        added_count,
        entry_count: baseline.entries.len(),
    };

    CliRunResult::json(EXIT_CODE_PASS, &output)
}

fn has_non_blank_arg(value: Option<&str>) -> bool {
    value.is_some_and(|value| !value.trim().is_empty())
}

fn handle_baseline_list(args: BaselineListArgs) -> CliRunResult {
    let (project_root, _config) = match load_project_root_and_config(&args.common) {
        Ok(context) => context,
        Err(error) => return error,
    };
    let (baseline_path, baseline) = match load_required_baseline(&project_root, &args.baseline) {
        Ok(loaded) => loaded,
        Err(error) => return error,
    };

    let today = Local::now().date_naive();
    let entry_count = baseline.entries.len();
    let mut entries = baseline
        .entries
        .into_iter()
        .filter(|entry| {
            args.owner
                .as_ref()
                .is_none_or(|owner| entry.owner.as_deref() == Some(owner.as_str()))
        })
        .filter(|entry| !args.expired || is_entry_expired(entry, &today.to_string()))
        .filter(|entry| {
            args.expiring_within
                .is_none_or(|days| entry_expires_within(entry, today, i64::from(days)))
        })
        .collect::<Vec<_>>();

    let groups = build_groups(&entries, args.group_by);
    let filtered_count = entries.len();
    let normalized_path = normalize_repo_relative(&project_root, &baseline_path);

    match args.format {
        BaselineDisplayFormat::Json => {
            let output = BaselineListOutput {
                schema_version: CLI_SCHEMA_VERSION.to_string(),
                status: "ok".to_string(),
                baseline_path: normalized_path,
                entry_count,
                filtered_count,
                groups,
                entries,
            };
            CliRunResult::json(EXIT_CODE_PASS, &output)
        }
        BaselineDisplayFormat::Human => {
            entries.sort_by(|a, b| a.fingerprint.cmp(&b.fingerprint));
            CliRunResult {
                exit_code: EXIT_CODE_PASS,
                stdout: render_baseline_list_human(&normalized_path, &entries, &groups),
                stderr: String::new(),
            }
        }
    }
}

fn handle_baseline_audit(args: BaselineAuditArgs) -> CliRunResult {
    let (project_root, config) = match load_project_root_and_config(&args.common) {
        Ok(context) => context,
        Err(error) => return error,
    };
    let (baseline_path, baseline) = match load_required_baseline(&project_root, &args.baseline) {
        Ok(loaded) => loaded,
        Err(error) => return error,
    };

    let report = audit_baseline(&baseline, &Local::now().format("%Y-%m-%d").to_string());
    let metadata_gaps = report.has_metadata_gaps();
    let exit_code = if config.baseline.require_metadata && metadata_gaps {
        EXIT_CODE_POLICY_VIOLATIONS
    } else {
        EXIT_CODE_PASS
    };
    let status = if exit_code == EXIT_CODE_PASS {
        "ok"
    } else {
        "fail"
    };
    let normalized_path = normalize_repo_relative(&project_root, &baseline_path);

    match args.format {
        BaselineDisplayFormat::Json => {
            let output = BaselineAuditOutput {
                schema_version: CLI_SCHEMA_VERSION.to_string(),
                status: status.to_string(),
                baseline_path: normalized_path,
                require_metadata: config.baseline.require_metadata,
                metadata_gaps,
                report,
            };
            CliRunResult::json(exit_code, &output)
        }
        BaselineDisplayFormat::Human => CliRunResult {
            exit_code,
            stdout: render_baseline_audit_human(
                &normalized_path,
                &report,
                config.baseline.require_metadata,
                metadata_gaps,
            ),
            stderr: String::new(),
        },
    }
}

fn load_baseline_project_context(
    common: &CommonProjectArgs,
) -> std::result::Result<BaselineProjectContext, CliRunResult> {
    let prepared = prepare_analysis_context(&common.project_root, None)?;

    let governance = match compute_governance_hashes(&prepared.loaded) {
        Ok(governance) => governance,
        Err(error) => {
            return Err(runtime_error_json(
                "governance",
                "failed to compute deterministic governance hashes",
                vec![error],
            ));
        }
    };

    let generated_from = BaselineGeneratedFrom {
        tool_version: build_info::tool_version().to_string(),
        git_sha: build_info::git_sha().to_string(),
        config_hash: governance.config_hash,
        spec_hash: governance.spec_hash,
    };

    Ok(BaselineProjectContext {
        prepared,
        generated_from,
    })
}

fn load_project_root_and_config(
    common: &CommonProjectArgs,
) -> std::result::Result<(PathBuf, crate::spec::SpecConfig), CliRunResult> {
    let project_root =
        std::fs::canonicalize(&common.project_root).unwrap_or_else(|_| common.project_root.clone());
    let config = spec::load_config(&project_root).map_err(|error| {
        runtime_error_json("config", "failed to load project", vec![error.to_string()])
    })?;

    Ok((project_root, config))
}

fn load_required_baseline(
    project_root: &Path,
    baseline: &Path,
) -> std::result::Result<(PathBuf, BaselineFile), CliRunResult> {
    let baseline_path = resolve_against_root(project_root, baseline);
    match load_optional_baseline(&baseline_path) {
        Ok(Some(baseline)) => Ok((baseline_path, normalize_baseline_file(&baseline))),
        Ok(None) => Err(runtime_error_json(
            "baseline",
            "baseline file not found",
            vec![normalize_repo_relative(project_root, &baseline_path)],
        )),
        Err(error) => Err(runtime_error_json(
            "baseline",
            "failed to load baseline file",
            vec![error.to_string()],
        )),
    }
}

fn filter_matching_violations(
    project_root: &Path,
    violations: &[PolicyViolation],
    args: &BaselineAddArgs,
) -> Vec<PolicyViolation> {
    let mut matching = violations
        .iter()
        .filter(|violation| violation.rule == args.rule)
        .filter(|violation| violation.from_module.as_deref() == Some(args.from_module.as_str()))
        .filter(|violation| {
            args.to_module
                .as_ref()
                .is_none_or(|module| violation.to_module.as_deref() == Some(module.as_str()))
        })
        .filter(|violation| {
            args.from_file.as_ref().is_none_or(|from_file| {
                normalize_repo_relative(project_root, &violation.from_file) == *from_file
            })
        })
        .filter(|violation| {
            args.to_file.as_ref().is_none_or(|to_file| {
                violation
                    .to_file
                    .as_ref()
                    .map(|path| normalize_repo_relative(project_root, path))
                    .as_deref()
                    == Some(to_file.as_str())
            })
        })
        .cloned()
        .collect::<Vec<_>>();

    sort_policy_violations(&mut matching);
    matching
}

fn entry_expires_within(entry: &BaselineEntry, today: NaiveDate, days: i64) -> bool {
    if is_entry_expired(entry, &today.to_string()) {
        return false;
    }

    let Some(expires_at) = entry.expires_at.as_deref() else {
        return false;
    };
    let Ok(expires_at) = NaiveDate::parse_from_str(expires_at, "%Y-%m-%d") else {
        return false;
    };

    expires_at <= today + Duration::days(days)
}

fn build_groups(
    entries: &[BaselineEntry],
    group_by: Option<BaselineGroupBy>,
) -> BTreeMap<String, usize> {
    let Some(group_by) = group_by else {
        return BTreeMap::new();
    };

    let mut groups = BTreeMap::new();
    for entry in entries {
        let key = match group_by {
            BaselineGroupBy::Owner => entry
                .owner
                .clone()
                .unwrap_or_else(|| "<no owner>".to_string()),
            BaselineGroupBy::Rule => entry.rule.clone(),
        };
        *groups.entry(key).or_insert(0) += 1;
    }
    groups
}

fn render_baseline_list_human(
    baseline_path: &str,
    entries: &[BaselineEntry],
    groups: &BTreeMap<String, usize>,
) -> String {
    let mut lines = vec![
        format!("Baseline: {baseline_path}"),
        format!("Entries: {}", entries.len()),
    ];

    if !groups.is_empty() {
        lines.push("".to_string());
        lines.push("Groups:".to_string());
        for (key, count) in groups {
            lines.push(format!("  {key}: {count}"));
        }
    } else {
        for entry in entries {
            lines.push(format!(
                "- {} {} owner={}",
                entry.rule,
                entry.from_module.as_deref().unwrap_or("-"),
                entry.owner.as_deref().unwrap_or("<none>"),
            ));
        }
    }

    format!("{}\n", lines.join("\n"))
}

fn render_baseline_audit_human(
    baseline_path: &str,
    report: &AuditReport,
    require_metadata: bool,
    metadata_gaps: bool,
) -> String {
    let mut lines = vec![
        format!("Baseline Audit: {baseline_path}"),
        format!("Total entries: {}", report.total_entries),
        String::new(),
        "By owner:".to_string(),
    ];

    if report.by_owner.is_empty() {
        lines.push("  <no owner>: 0".to_string());
    } else {
        for (owner, stats) in &report.by_owner {
            lines.push(format!(
                "  {owner}: {} entries ({} expired)",
                stats.total, stats.expired
            ));
        }
    }

    if report.entries_without_owner > 0 {
        lines.push(format!(
            "  <no owner>: {} entries",
            report.entries_without_owner
        ));
    }

    lines.extend([
        String::new(),
        "Expiry status:".to_string(),
        format!("  Expired: {}", report.expired),
        format!("  Expiring < 30d: {}", report.expiring_within_30d),
        format!("  No expiry set: {}", report.no_expiry),
        format!("  Active: {}", report.active),
        String::new(),
        "Metadata coverage:".to_string(),
        format!(
            "  Has owner: {}/{}",
            report.has_owner_count, report.total_entries
        ),
        format!(
            "  Has reason: {}/{}",
            report.has_reason_count, report.total_entries
        ),
        format!(
            "  Has added_at: {}/{}",
            report.has_added_at_count, report.total_entries
        ),
    ]);

    if require_metadata {
        lines.push(String::new());
        lines.push(format!(
            "Require metadata: {}",
            if metadata_gaps { "fail" } else { "ok" }
        ));
    }

    format!("{}\n", lines.join("\n"))
}
