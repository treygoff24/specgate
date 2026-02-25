use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Instant;

use clap::{Args, Parser, Subcommand};
use serde::Serialize;

use crate::baseline::{
    DEFAULT_BASELINE_PATH, build_baseline, classify_violations, load_optional_baseline,
    write_baseline,
};
use crate::deterministic::{normalize_path, normalize_repo_relative};
use crate::graph::DependencyGraph;
use crate::resolver::{ModuleMapOverlap, ModuleResolver};
use crate::rules::boundary::evaluate_boundary_rules;
use crate::rules::{
    DEPENDENCY_FORBIDDEN_RULE_ID, DEPENDENCY_NOT_ALLOWED_RULE_ID, DependencyRule, RuleContext,
    RuleViolation, RuleWithResolver, evaluate_enforce_layer, evaluate_no_circular_deps,
    is_canonical_import_rule_id,
};
use crate::spec::{self, Severity, SpecConfig, SpecFile, ValidationLevel, ValidationReport};
use crate::verdict::{self, PolicyViolation, VerdictMetrics, VerdictStatus, build_verdict};

pub const EXIT_CODE_PASS: i32 = 0;
pub const EXIT_CODE_POLICY_VIOLATIONS: i32 = 1;
pub const EXIT_CODE_RUNTIME_ERROR: i32 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliRunResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl CliRunResult {
    fn json<T: Serialize>(exit_code: i32, payload: &T) -> Self {
        match serde_json::to_string_pretty(payload) {
            Ok(json) => Self {
                exit_code,
                stdout: format!("{json}\n"),
                stderr: String::new(),
            },
            Err(error) => Self {
                exit_code: EXIT_CODE_RUNTIME_ERROR,
                stdout: String::new(),
                stderr: format!("failed to serialize CLI JSON output: {error}\n"),
            },
        }
    }

    fn clap_error(error: clap::Error) -> Self {
        Self {
            exit_code: error.exit_code() as i32,
            stdout: String::new(),
            stderr: format!("{error}"),
        }
    }
}

#[derive(Debug, Parser)]
#[command(name = "specgate")]
#[command(version)]
#[command(about = "Machine-checkable architectural intent for TypeScript projects")]
pub struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run the full policy pipeline and emit verdict JSON.
    Check(CheckArgs),
    /// Validate spec/config files only.
    Validate(CommonProjectArgs),
    /// Diagnostics and parity checks.
    Doctor(DoctorArgs),
    /// Generate a baseline file for current violations.
    Baseline(BaselineArgs),
}

#[derive(Debug, Clone, Args)]
struct CommonProjectArgs {
    /// Project root containing code + specs + optional specgate.config.yml.
    #[arg(long, default_value = ".")]
    project_root: PathBuf,
}

#[derive(Debug, Clone, Args)]
struct CheckArgs {
    #[command(flatten)]
    common: CommonProjectArgs,
    /// Include timing metadata in output.
    #[arg(long)]
    metrics: bool,
    /// Baseline file path.
    #[arg(long, default_value = DEFAULT_BASELINE_PATH)]
    baseline: PathBuf,
    /// Disable baseline classification even if a baseline file exists.
    #[arg(long)]
    no_baseline: bool,
}

#[derive(Debug, Clone, Args)]
struct BaselineArgs {
    #[command(flatten)]
    common: CommonProjectArgs,
    /// Output baseline path.
    #[arg(long, default_value = DEFAULT_BASELINE_PATH)]
    output: PathBuf,
}

#[derive(Debug, Clone, Args)]
struct DoctorArgs {
    #[command(flatten)]
    common: CommonProjectArgs,
    #[command(subcommand)]
    command: Option<DoctorCommand>,
}

#[derive(Debug, Clone, Subcommand)]
enum DoctorCommand {
    /// Compare Specgate dependency edges with a configured trace source.
    Compare(DoctorCompareArgs),
}

#[derive(Debug, Clone, Args)]
struct DoctorCompareArgs {
    #[command(flatten)]
    common: CommonProjectArgs,
    /// JSON file containing trace edges (supports { edges: [{from,to}] } or [{from,to}]).
    #[arg(long)]
    tsc_trace: Option<PathBuf>,
    /// Command that emits compatible JSON to stdout.
    #[arg(long)]
    tsc_command: Option<String>,
}

#[derive(Debug, Clone)]
struct LoadedProject {
    project_root: PathBuf,
    config: SpecConfig,
    specs: Vec<SpecFile>,
    validation: ValidationReport,
}

#[derive(Debug, Clone)]
struct AnalysisArtifacts {
    policy_violations: Vec<PolicyViolation>,
    layer_config_issues: Vec<String>,
    module_map_overlaps: Vec<ModuleMapOverlap>,
    parse_warning_count: usize,
    graph_nodes: usize,
    graph_edges: usize,
    suppressed_violations: usize,
    edge_pairs: BTreeSet<(String, String)>,
}

#[derive(Debug, Serialize)]
struct ErrorOutput {
    schema_version: String,
    status: String,
    code: String,
    message: String,
    details: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ValidateOutput {
    schema_version: String,
    status: String,
    spec_count: usize,
    error_count: usize,
    warning_count: usize,
    issues: Vec<ValidateIssueOutput>,
}

#[derive(Debug, Serialize)]
struct ValidateIssueOutput {
    level: String,
    module: String,
    message: String,
    spec_path: Option<String>,
}

#[derive(Debug, Serialize)]
struct BaselineOutput {
    schema_version: String,
    status: String,
    baseline_path: String,
    entry_count: usize,
    source_violation_count: usize,
}

#[derive(Debug, Serialize)]
struct DoctorOutput {
    schema_version: String,
    status: String,
    spec_count: usize,
    validation_errors: usize,
    validation_warnings: usize,
    graph_nodes: usize,
    graph_edges: usize,
    parse_warning_count: usize,
    policy_violation_count: usize,
    layer_config_issues: Vec<String>,
    module_map_overlaps: Vec<DoctorOverlapOutput>,
}

#[derive(Debug, Serialize)]
struct DoctorOverlapOutput {
    file: String,
    selected_module: String,
    matched_modules: Vec<String>,
}

#[derive(Debug, Serialize)]
struct DoctorCompareOutput {
    schema_version: String,
    status: String,
    configured: bool,
    reason: Option<String>,
    specgate_edge_count: usize,
    trace_edge_count: usize,
    missing_in_specgate: Vec<String>,
    extra_in_specgate: Vec<String>,
}

pub fn run<I, T>(args: I) -> CliRunResult
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(error) => return CliRunResult::clap_error(error),
    };

    match cli.command {
        Command::Validate(args) => handle_validate(args),
        Command::Check(args) => handle_check(args),
        Command::Baseline(args) => handle_baseline(args),
        Command::Doctor(args) => handle_doctor(args),
    }
}

fn handle_validate(args: CommonProjectArgs) -> CliRunResult {
    let loaded = match load_project(&args.project_root) {
        Ok(loaded) => loaded,
        Err(error) => return runtime_error_json("config", "failed to load project", vec![error]),
    };

    let mut issues = loaded
        .validation
        .issues
        .iter()
        .map(|issue| ValidateIssueOutput {
            level: match issue.level {
                ValidationLevel::Error => "error".to_string(),
                ValidationLevel::Warning => "warning".to_string(),
            },
            module: issue.module.clone(),
            message: issue.message.clone(),
            spec_path: issue
                .spec_path
                .as_ref()
                .map(|path| normalize_repo_relative(&loaded.project_root, path)),
        })
        .collect::<Vec<_>>();

    issues.sort_by(|a, b| {
        a.module
            .cmp(&b.module)
            .then_with(|| a.level.cmp(&b.level))
            .then_with(|| a.spec_path.cmp(&b.spec_path))
            .then_with(|| a.message.cmp(&b.message))
    });

    let error_count = loaded.validation.errors().len();
    let warning_count = loaded.validation.warnings().len();

    let output = ValidateOutput {
        schema_version: "2.2".to_string(),
        status: if error_count == 0 {
            "ok".to_string()
        } else {
            "error".to_string()
        },
        spec_count: loaded.specs.len(),
        error_count,
        warning_count,
        issues,
    };

    let exit_code = if error_count == 0 {
        EXIT_CODE_PASS
    } else {
        EXIT_CODE_RUNTIME_ERROR
    };

    CliRunResult::json(exit_code, &output)
}

fn handle_check(args: CheckArgs) -> CliRunResult {
    let mut timings = BTreeMap::new();
    let total_start = Instant::now();

    let load_start = Instant::now();
    let loaded = match load_project(&args.common.project_root) {
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

    let analyze_start = Instant::now();
    let artifacts = match analyze_project(&loaded) {
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

    let classify_start = Instant::now();
    let classified = classify_violations(
        &loaded.project_root,
        &artifacts.policy_violations,
        baseline.as_ref(),
    );
    record_timing(&mut timings, "classify_baseline", classify_start);

    let metrics = if args.metrics {
        Some(VerdictMetrics {
            timings_ms: timings,
            total_ms: total_start.elapsed().as_millis(),
        })
    } else {
        None
    };

    let verdict = build_verdict(
        &loaded.project_root,
        &classified,
        artifacts.suppressed_violations,
        metrics,
    );

    let exit_code = match verdict.status {
        VerdictStatus::Pass => EXIT_CODE_PASS,
        VerdictStatus::Fail => EXIT_CODE_POLICY_VIOLATIONS,
    };

    CliRunResult::json(exit_code, &verdict)
}

fn handle_baseline(args: BaselineArgs) -> CliRunResult {
    let loaded = match load_project(&args.common.project_root) {
        Ok(loaded) => loaded,
        Err(error) => return runtime_error_json("config", "failed to load project", vec![error]),
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

    let artifacts = match analyze_project(&loaded) {
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

    let baseline = build_baseline(&loaded.project_root, &artifacts.policy_violations);
    let baseline_path = resolve_against_root(&loaded.project_root, &args.output);

    if let Err(error) = write_baseline(&baseline_path, &baseline) {
        return runtime_error_json(
            "baseline",
            "failed to write baseline file",
            vec![error.to_string()],
        );
    }

    let output = BaselineOutput {
        schema_version: "2.2".to_string(),
        status: "ok".to_string(),
        baseline_path: normalize_repo_relative(&loaded.project_root, &baseline_path),
        entry_count: baseline.entries.len(),
        source_violation_count: artifacts.policy_violations.len(),
    };

    CliRunResult::json(EXIT_CODE_PASS, &output)
}

fn handle_doctor(args: DoctorArgs) -> CliRunResult {
    match args.command {
        Some(DoctorCommand::Compare(compare_args)) => handle_doctor_compare(compare_args),
        None => handle_doctor_overview(args.common),
    }
}

fn handle_doctor_overview(args: CommonProjectArgs) -> CliRunResult {
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

    let artifacts = match analyze_project(&loaded) {
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
    };

    let exit_code = if output.status == "ok" {
        EXIT_CODE_PASS
    } else {
        EXIT_CODE_RUNTIME_ERROR
    };

    CliRunResult::json(exit_code, &output)
}

fn handle_doctor_compare(args: DoctorCompareArgs) -> CliRunResult {
    let loaded = match load_project(&args.common.project_root) {
        Ok(loaded) => loaded,
        Err(error) => return runtime_error_json("config", "failed to load project", vec![error]),
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

    let artifacts = match analyze_project(&loaded) {
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

    let trace_source = match load_trace_source(&loaded.project_root, &args) {
        Ok(trace_source) => trace_source,
        Err(error) => {
            return runtime_error_json(
                "doctor.compare",
                "failed to load tsc parity trace",
                vec![error],
            );
        }
    };

    let Some(trace_json) = trace_source.payload else {
        let output = DoctorCompareOutput {
            schema_version: "2.2".to_string(),
            status: "skipped".to_string(),
            configured: trace_source.configured,
            reason: trace_source.reason,
            specgate_edge_count: artifacts.edge_pairs.len(),
            trace_edge_count: 0,
            missing_in_specgate: Vec::new(),
            extra_in_specgate: Vec::new(),
        };

        return CliRunResult::json(EXIT_CODE_PASS, &output);
    };

    let trace_edges = match parse_trace_edges(&loaded.project_root, &trace_json) {
        Ok(edges) => edges,
        Err(error) => {
            return runtime_error_json(
                "doctor.compare",
                "failed to parse trace edges",
                vec![error],
            );
        }
    };

    let missing_in_specgate = trace_edges
        .difference(&artifacts.edge_pairs)
        .map(|(from, to)| format!("{from} -> {to}"))
        .collect::<Vec<_>>();

    let extra_in_specgate = artifacts
        .edge_pairs
        .difference(&trace_edges)
        .map(|(from, to)| format!("{from} -> {to}"))
        .collect::<Vec<_>>();

    let status = if missing_in_specgate.is_empty() && extra_in_specgate.is_empty() {
        "match"
    } else {
        "mismatch"
    };

    let output = DoctorCompareOutput {
        schema_version: "2.2".to_string(),
        status: status.to_string(),
        configured: true,
        reason: trace_source.reason,
        specgate_edge_count: artifacts.edge_pairs.len(),
        trace_edge_count: trace_edges.len(),
        missing_in_specgate,
        extra_in_specgate,
    };

    let exit_code = if status == "mismatch" {
        EXIT_CODE_POLICY_VIOLATIONS
    } else {
        EXIT_CODE_PASS
    };

    CliRunResult::json(exit_code, &output)
}

#[derive(Debug)]
struct TraceSource {
    configured: bool,
    payload: Option<String>,
    reason: Option<String>,
}

fn load_trace_source(
    project_root: &Path,
    args: &DoctorCompareArgs,
) -> std::result::Result<TraceSource, String> {
    if let Some(trace_path) = &args.tsc_trace {
        let resolved = resolve_against_root(project_root, trace_path);
        let source = std::fs::read_to_string(&resolved).map_err(|error| {
            format!("failed to read trace file {}: {error}", resolved.display())
        })?;

        return Ok(TraceSource {
            configured: true,
            payload: Some(source),
            reason: Some(format!(
                "loaded trace file '{}'",
                normalize_repo_relative(project_root, &resolved)
            )),
        });
    }

    if let Some(command) = &args.tsc_command {
        let executable = command.split_whitespace().next().unwrap_or_default();
        if executable.is_empty() {
            return Ok(TraceSource {
                configured: true,
                payload: None,
                reason: Some("tsc command was empty".to_string()),
            });
        }

        if executable == "tsc" && !is_command_available(executable) {
            return Ok(TraceSource {
                configured: true,
                payload: None,
                reason: Some("tsc is not available on PATH; parity check skipped".to_string()),
            });
        }

        let output = std::process::Command::new("sh")
            .arg("-lc")
            .arg(command)
            .output()
            .map_err(|error| format!("failed to run command '{command}': {error}"))?;

        if !output.status.success() {
            if executable == "tsc" && output.status.code() == Some(127) {
                return Ok(TraceSource {
                    configured: true,
                    payload: None,
                    reason: Some("tsc is not available on PATH; parity check skipped".to_string()),
                });
            }

            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!(
                "trace command '{command}' failed with status {}: {}",
                output.status,
                stderr.trim()
            ));
        }

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        return Ok(TraceSource {
            configured: true,
            payload: Some(stdout),
            reason: Some(format!("loaded trace from command '{command}'")),
        });
    }

    Ok(TraceSource {
        configured: false,
        payload: None,
        reason: Some("no tsc trace source configured".to_string()),
    })
}

fn is_command_available(command: &str) -> bool {
    std::process::Command::new("sh")
        .arg("-lc")
        .arg(format!("command -v {command} >/dev/null 2>&1"))
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn parse_trace_edges(
    project_root: &Path,
    trace_json: &str,
) -> std::result::Result<BTreeSet<(String, String)>, String> {
    let value: serde_json::Value = serde_json::from_str(trace_json)
        .map_err(|error| format!("trace JSON parse error: {error}"))?;

    let mut edges = BTreeSet::new();
    collect_trace_edges(project_root, &value, &mut edges);
    Ok(edges)
}

fn collect_trace_edges(
    project_root: &Path,
    value: &serde_json::Value,
    edges: &mut BTreeSet<(String, String)>,
) {
    match value {
        serde_json::Value::Array(items) => {
            for item in items {
                collect_trace_edges(project_root, item, edges);
            }
        }
        serde_json::Value::Object(map) => {
            if let (Some(from), Some(to)) = (
                map.get("from").and_then(serde_json::Value::as_str),
                map.get("to").and_then(serde_json::Value::as_str),
            ) {
                edges.insert((
                    normalize_trace_path(project_root, from),
                    normalize_trace_path(project_root, to),
                ));
            }

            for nested in map.values() {
                collect_trace_edges(project_root, nested, edges);
            }
        }
        _ => {}
    }
}

fn normalize_trace_path(project_root: &Path, raw: &str) -> String {
    let path = Path::new(raw);
    if path.is_absolute() {
        normalize_repo_relative(project_root, path)
    } else {
        normalize_path(path)
    }
}

fn load_project(project_root: &Path) -> std::result::Result<LoadedProject, String> {
    let project_root =
        std::fs::canonicalize(project_root).unwrap_or_else(|_| project_root.to_path_buf());

    let config = spec::load_config(&project_root)
        .map_err(|error| format!("failed to load config: {error}"))?;

    let (specs, validation) = spec::discover_and_validate(&project_root, &config)
        .map_err(|error| format!("failed to discover specs: {error}"))?;

    Ok(LoadedProject {
        project_root,
        config,
        specs,
        validation,
    })
}

fn severity_rank(severity: Severity) -> u8 {
    match severity {
        Severity::Error => 0,
        Severity::Warning => 1,
    }
}

fn dependency_violation_severity(rule_id: &str) -> Severity {
    debug_assert!(
        matches!(
            rule_id,
            DEPENDENCY_FORBIDDEN_RULE_ID | DEPENDENCY_NOT_ALLOWED_RULE_ID
        ),
        "unexpected dependency rule id '{rule_id}'"
    );

    Severity::Error
}

fn rule_ids_match(constraint_rule: &str, violation_rule: &str) -> bool {
    if is_canonical_import_rule_id(constraint_rule) || is_canonical_import_rule_id(violation_rule) {
        return is_canonical_import_rule_id(constraint_rule)
            && is_canonical_import_rule_id(violation_rule);
    }

    constraint_rule == violation_rule
}

fn severity_for_constraint_rule(spec: &SpecFile, rule_id: &str) -> Option<Severity> {
    spec.constraints
        .iter()
        .filter(|constraint| rule_ids_match(&constraint.rule, rule_id))
        .map(|constraint| constraint.severity)
        .min_by_key(|severity| severity_rank(*severity))
}

fn boundary_constraint_module<'a>(violation: &'a RuleViolation) -> Option<&'a str> {
    let rule = violation.rule.as_str();

    match rule {
        "boundary.never_imports" | "boundary.allow_imports_from" | "boundary.public_api" => {
            violation.from_module.as_deref()
        }
        "boundary.deny_imported_by"
        | "boundary.allow_imported_by"
        | "boundary.visibility.internal"
        | "boundary.visibility.private" => violation.to_module.as_deref(),
        _ if is_canonical_import_rule_id(rule) => violation.to_module.as_deref(),
        _ => None,
    }
}

fn boundary_violation_severity(
    violation: &RuleViolation,
    spec_by_module: &BTreeMap<&str, &SpecFile>,
) -> Severity {
    let Some(module_id) = boundary_constraint_module(violation) else {
        return Severity::Error;
    };

    spec_by_module
        .get(module_id)
        .and_then(|spec| severity_for_constraint_rule(spec, &violation.rule))
        .unwrap_or(Severity::Error)
}

fn analyze_project(loaded: &LoadedProject) -> std::result::Result<AnalysisArtifacts, String> {
    let mut resolver = ModuleResolver::new(&loaded.project_root, &loaded.specs)
        .map_err(|error| format!("failed to initialize module resolver: {error}"))?;
    let module_map_overlaps = resolver.module_map_overlaps().to_vec();

    let graph = DependencyGraph::build(&loaded.project_root, &mut resolver, &loaded.config)
        .map_err(|error| format!("failed to build dependency graph: {error}"))?;

    let parse_warning_count = graph
        .files()
        .into_iter()
        .map(|node| node.analysis.parse_warnings.len())
        .sum();

    let suppressed_violations = graph
        .dependency_edges()
        .iter()
        .filter(|edge| edge.ignored_by_comment)
        .count();

    let edge_pairs = graph
        .dependency_edges()
        .into_iter()
        .map(|edge| {
            (
                normalize_repo_relative(&loaded.project_root, &edge.from),
                normalize_repo_relative(&loaded.project_root, &edge.to),
            )
        })
        .collect::<BTreeSet<_>>();

    let ctx = RuleContext {
        project_root: &loaded.project_root,
        config: &loaded.config,
        specs: &loaded.specs,
        graph: &graph,
    };

    let mut policy_violations = Vec::new();
    let spec_by_module = loaded
        .specs
        .iter()
        .map(|spec| (spec.module.as_str(), spec))
        .collect::<BTreeMap<_, _>>();

    let boundary_violations = evaluate_boundary_rules(&ctx)
        .into_iter()
        .map(|violation| {
            let severity = boundary_violation_severity(&violation, &spec_by_module);

            PolicyViolation {
                rule: violation.rule,
                severity,
                message: violation.message,
                from_file: violation.from_file,
                to_file: violation.to_file,
                from_module: violation.from_module,
                to_module: violation.to_module,
                line: violation.line,
                column: violation.column,
            }
        })
        .collect::<Vec<_>>();
    policy_violations.extend(boundary_violations);

    let dependency_violations = DependencyRule
        .evaluate_with_resolver(&ctx, &mut resolver)
        .map_err(|error| format!("failed to evaluate dependency rules: {error}"))?
        .into_iter()
        .map(|violation| {
            let severity = dependency_violation_severity(violation.rule.as_str());

            PolicyViolation {
                rule: violation.rule,
                severity,
                message: violation.message,
                from_file: violation.from_file,
                to_file: violation.to_file,
                from_module: violation.from_module,
                to_module: violation.to_module,
                line: violation.line,
                column: violation.column,
            }
        })
        .collect::<Vec<_>>();
    policy_violations.extend(dependency_violations);

    let circular_violations = evaluate_no_circular_deps(&loaded.specs, &graph)
        .into_iter()
        .map(|violation| {
            let from_file = violation
                .component_files
                .first()
                .cloned()
                .unwrap_or_else(|| loaded.project_root.clone());

            PolicyViolation {
                rule: violation.rule,
                severity: violation.severity,
                message: violation.message,
                from_file,
                to_file: None,
                from_module: Some(violation.module),
                to_module: None,
                line: None,
                column: None,
            }
        })
        .collect::<Vec<_>>();
    policy_violations.extend(circular_violations);

    let layer_report = evaluate_enforce_layer(&loaded.specs, &graph);
    let layer_violations = layer_report
        .violations
        .into_iter()
        .map(|violation| PolicyViolation {
            rule: crate::rules::ENFORCE_LAYER_RULE_ID.to_string(),
            severity: Severity::Error,
            message: violation.message,
            from_file: violation.from_file,
            to_file: Some(violation.to_file),
            from_module: Some(violation.from_module),
            to_module: Some(violation.to_module),
            line: None,
            column: None,
        })
        .collect::<Vec<_>>();
    policy_violations.extend(layer_violations);

    let layer_config_issues = layer_report
        .config_issues
        .into_iter()
        .map(|issue| format!("{}: {}", issue.module, issue.message))
        .collect::<Vec<_>>();

    verdict::sort_policy_violations(&mut policy_violations);

    Ok(AnalysisArtifacts {
        policy_violations,
        layer_config_issues,
        module_map_overlaps,
        parse_warning_count,
        graph_nodes: graph.node_count(),
        graph_edges: graph.edge_count(),
        suppressed_violations,
        edge_pairs,
    })
}

fn runtime_error_json(code: &str, message: &str, mut details: Vec<String>) -> CliRunResult {
    details.sort();
    details.dedup();

    CliRunResult::json(
        EXIT_CODE_RUNTIME_ERROR,
        &ErrorOutput {
            schema_version: "2.2".to_string(),
            status: "error".to_string(),
            code: code.to_string(),
            message: message.to_string(),
            details,
        },
    )
}

fn resolve_against_root(project_root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        project_root.join(path)
    }
}

fn record_timing(timings: &mut BTreeMap<String, u128>, key: &str, start: Instant) {
    timings.insert(key.to_string(), start.elapsed().as_millis());
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::Value;
    use tempfile::TempDir;

    use super::*;

    fn write_file(root: &Path, relative_path: &str, content: &str) {
        let path = root.join(relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("mkdir parent");
        }
        fs::write(path, content).expect("write file");
    }

    fn write_basic_project(root: &Path) {
        write_file(
            root,
            "modules/app.spec.yml",
            "version: \"2.2\"\nmodule: app\nboundaries:\n  path: src/app/**/*\nconstraints: []\n",
        );
        write_file(
            root,
            "modules/core.spec.yml",
            "version: \"2.2\"\nmodule: core\nboundaries:\n  path: src/core/**/*\nconstraints: []\n",
        );
        write_file(root, "src/app/main.ts", "export const app = 1;\n");
        write_file(root, "src/core/index.ts", "export const core = 1;\n");
        write_file(
            root,
            "specgate.config.yml",
            "spec_dirs:\n  - modules\nexclude: []\ntest_patterns: []\n",
        );
    }

    fn parse_json(stdout: &str) -> Value {
        serde_json::from_str(stdout).expect("cli output json")
    }

    #[test]
    fn validate_returns_exit_two_on_schema_errors() {
        let temp = TempDir::new().expect("tempdir");
        write_file(
            temp.path(),
            "modules/bad.spec.yml",
            "version: \"2.1\"\nmodule: bad\nconstraints: []\n",
        );
        write_file(
            temp.path(),
            "specgate.config.yml",
            "spec_dirs:\n  - modules\nexclude: []\ntest_patterns: []\n",
        );

        let result = run([
            "specgate",
            "validate",
            "--project-root",
            temp.path().to_str().expect("utf8 path"),
        ]);

        assert_eq!(result.exit_code, EXIT_CODE_RUNTIME_ERROR);
        assert!(result.stdout.contains("\"status\": \"error\""));
    }

    #[test]
    fn check_exit_codes_follow_policy_vs_runtime_contract() {
        let temp = TempDir::new().expect("tempdir");
        write_basic_project(temp.path());

        // Clean policy: exit 0.
        let pass = run([
            "specgate",
            "check",
            "--project-root",
            temp.path().to_str().expect("utf8 path"),
            "--no-baseline",
        ]);
        assert_eq!(pass.exit_code, EXIT_CODE_PASS);

        // Introduce policy violation: app may never import core.
        write_file(
            temp.path(),
            "modules/app.spec.yml",
            "version: \"2.2\"\nmodule: app\nboundaries:\n  path: src/app/**/*\n  never_imports:\n    - core\nconstraints: []\n",
        );
        write_file(
            temp.path(),
            "src/app/main.ts",
            "import { core } from '../core/index';\nexport const app = core;\n",
        );

        let fail = run([
            "specgate",
            "check",
            "--project-root",
            temp.path().to_str().expect("utf8 path"),
            "--no-baseline",
        ]);
        assert_eq!(fail.exit_code, EXIT_CODE_POLICY_VIOLATIONS);

        // Introduce runtime/config error.
        write_file(
            temp.path(),
            "specgate.config.yml",
            "spec_dirs:\n  - modules\nexclude:\n  - \"[\"\n",
        );
        let runtime = run([
            "specgate",
            "check",
            "--project-root",
            temp.path().to_str().expect("utf8 path"),
            "--no-baseline",
        ]);
        assert_eq!(runtime.exit_code, EXIT_CODE_RUNTIME_ERROR);
    }

    #[test]
    fn check_output_is_deterministic_by_default() {
        let temp = TempDir::new().expect("tempdir");
        write_basic_project(temp.path());
        write_file(
            temp.path(),
            "modules/app.spec.yml",
            "version: \"2.2\"\nmodule: app\nboundaries:\n  path: src/app/**/*\n  never_imports:\n    - core\nconstraints: []\n",
        );
        write_file(
            temp.path(),
            "src/app/main.ts",
            "import { core } from '../core/index';\nexport const app = core;\n",
        );

        let one = run([
            "specgate",
            "check",
            "--project-root",
            temp.path().to_str().expect("utf8 path"),
            "--no-baseline",
        ]);
        let two = run([
            "specgate",
            "check",
            "--project-root",
            temp.path().to_str().expect("utf8 path"),
            "--no-baseline",
        ]);

        assert_eq!(one.exit_code, EXIT_CODE_POLICY_VIOLATIONS);
        assert_eq!(one.stdout, two.stdout);
        assert!(!one.stdout.contains("\"metrics\""));
    }

    #[test]
    fn boundary_constraint_severity_is_propagated_to_verdict() {
        let temp = TempDir::new().expect("tempdir");
        write_basic_project(temp.path());

        write_file(
            temp.path(),
            "modules/app.spec.yml",
            "version: \"2.2\"\nmodule: app\nboundaries:\n  path: src/app/**/*\n  never_imports:\n    - core\nconstraints:\n  - rule: boundary.never_imports\n    severity: warning\n",
        );
        write_file(
            temp.path(),
            "src/app/main.ts",
            "import { core } from '../core/index';\nexport const app = core;\n",
        );

        let result = run([
            "specgate",
            "check",
            "--project-root",
            temp.path().to_str().expect("utf8 path"),
            "--no-baseline",
        ]);

        assert_eq!(result.exit_code, EXIT_CODE_PASS);
        let output = parse_json(&result.stdout);
        assert_eq!(output["status"], "pass");
        assert_eq!(output["summary"]["new_error_violations"], 0);
        assert_eq!(output["summary"]["new_warning_violations"], 1);
        assert_eq!(output["violations"][0]["rule"], "boundary.never_imports");
        assert_eq!(output["violations"][0]["severity"], "warning");
    }

    #[test]
    fn canonical_import_alias_constraint_maps_to_canonical_rule_id() {
        let temp = TempDir::new().expect("tempdir");
        write_basic_project(temp.path());

        write_file(
            temp.path(),
            "modules/core.spec.yml",
            "version: \"2.2\"\nmodule: core\nimport_id: '@app/core'\nboundaries:\n  path: src/core/**/*\n  enforce_canonical_imports: true\nconstraints:\n  - rule: boundary.canonical_imports\n    severity: warning\n",
        );
        write_file(
            temp.path(),
            "src/app/main.ts",
            "import { core } from '../core/index';\nexport const app = core;\n",
        );

        let result = run([
            "specgate",
            "check",
            "--project-root",
            temp.path().to_str().expect("utf8 path"),
            "--no-baseline",
        ]);

        assert_eq!(result.exit_code, EXIT_CODE_PASS);
        let output = parse_json(&result.stdout);
        assert_eq!(output["status"], "pass");
        assert_eq!(output["summary"]["new_error_violations"], 0);
        assert_eq!(output["summary"]["new_warning_violations"], 1);
        assert_eq!(
            output["violations"][0]["rule"],
            crate::rules::BOUNDARY_CANONICAL_IMPORT_RULE_ID
        );
        assert_eq!(output["violations"][0]["severity"], "warning");
    }

    #[test]
    fn baseline_generation_and_check_classification_work_together() {
        let temp = TempDir::new().expect("tempdir");
        write_basic_project(temp.path());

        write_file(
            temp.path(),
            "modules/app.spec.yml",
            "version: \"2.2\"\nmodule: app\nboundaries:\n  path: src/app/**/*\n  never_imports:\n    - core\nconstraints: []\n",
        );
        write_file(
            temp.path(),
            "src/app/main.ts",
            "import { core } from '../core/index';\nexport const app = core;\n",
        );

        let baseline = run([
            "specgate",
            "baseline",
            "--project-root",
            temp.path().to_str().expect("utf8 path"),
        ]);
        assert_eq!(baseline.exit_code, EXIT_CODE_PASS);

        let with_baseline = run([
            "specgate",
            "check",
            "--project-root",
            temp.path().to_str().expect("utf8 path"),
        ]);
        assert_eq!(with_baseline.exit_code, EXIT_CODE_PASS);
        assert!(with_baseline.stdout.contains("\"baseline_violations\": 1"));

        write_file(
            temp.path(),
            "src/app/another.ts",
            "import { core } from '../core/index';\nexport const another = core;\n",
        );

        let new_violation = run([
            "specgate",
            "check",
            "--project-root",
            temp.path().to_str().expect("utf8 path"),
        ]);

        assert_eq!(new_violation.exit_code, EXIT_CODE_POLICY_VIOLATIONS);
        assert!(new_violation.stdout.contains("\"new_violations\": 1"));
    }

    #[test]
    fn doctor_compare_skips_gracefully_when_tsc_missing() {
        let temp = TempDir::new().expect("tempdir");
        write_basic_project(temp.path());

        let result = run([
            "specgate",
            "doctor",
            "compare",
            "--project-root",
            temp.path().to_str().expect("utf8 path"),
            "--tsc-command",
            "tsc --generateTrace .specgate-trace --noEmit",
        ]);

        assert_eq!(result.exit_code, EXIT_CODE_PASS);
        assert!(result.stdout.contains("\"status\": \"skipped\""));
    }
}
