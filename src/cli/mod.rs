//! CLI module for Specgate.
//!
//! This module provides the command-line interface with subcommands for
//! check, validate, init, baseline, and doctor operations.

pub mod check;
pub mod init;
pub mod validate;

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use clap::{Args, Parser, Subcommand};
use serde::Serialize;

use crate::baseline::{
    BaselineGeneratedFrom, DEFAULT_BASELINE_PATH, build_baseline_with_metadata,
    classify_violations, load_optional_baseline, write_baseline,
};
use crate::build_info;
use crate::deterministic::{normalize_path, normalize_repo_relative, stable_hash_hex};
use crate::graph::DependencyGraph;
use crate::resolver::{ModuleMapOverlap, ModuleResolver, ResolvedImport};
use crate::rules::boundary::evaluate_boundary_rules;
use crate::rules::{
    DEPENDENCY_FORBIDDEN_RULE_ID, DEPENDENCY_NOT_ALLOWED_RULE_ID, DependencyRule, RuleContext,
    RuleViolation, RuleWithResolver, evaluate_enforce_layer, evaluate_no_circular_deps,
    is_canonical_import_rule_id,
};
use crate::spec::{
    self, Severity, SpecConfig, SpecFile, ValidationLevel, ValidationReport,
    types::SUPPORTED_SPEC_VERSION,
};
use crate::verdict::{
    self, PolicyViolation, VerdictIdentity, VerdictMetrics, VerdictStatus, build_verdict,
};

// Re-export from submodules for convenience
pub use check::{CheckArgs, CheckOutputMode, DiffMode};
pub use init::InitArgs as InitArgsEnhanced;
pub use validate::ValidateArgs;

pub const EXIT_CODE_PASS: i32 = 0;
pub const EXIT_CODE_POLICY_VIOLATIONS: i32 = 1;
pub const EXIT_CODE_RUNTIME_ERROR: i32 = 2;
pub const EXIT_CODE_DOCTOR_MISMATCH: i32 = 3;

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
            exit_code: error.exit_code(),
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
    /// Initialize starter config/spec scaffolding.
    Init(InitArgs),
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
struct BaselineArgs {
    #[command(flatten)]
    common: CommonProjectArgs,
    /// Output baseline path.
    #[arg(long, default_value = DEFAULT_BASELINE_PATH)]
    output: PathBuf,
}

#[derive(Debug, Clone, Args)]
struct InitArgs {
    #[command(flatten)]
    common: CommonProjectArgs,
    /// Directory where starter `.spec.yml` files are written.
    #[arg(long, default_value = "modules")]
    spec_dir: PathBuf,
    /// Starter module id used in the initial example spec.
    #[arg(long, default_value = "app")]
    module: String,
    /// Starter module boundary glob.
    #[arg(long, default_value = "src/app/**/*")]
    module_path: String,
    /// Overwrite existing scaffold files.
    #[arg(long)]
    force: bool,
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
    ///
    /// SECURITY: this command is executed through `sh -lc` and can run arbitrary shell code.
    /// You must also pass `--allow-shell` to opt into execution.
    #[arg(long)]
    tsc_command: Option<String>,
    /// Explicit opt-in for running `--tsc-command` through the system shell.
    #[arg(long)]
    allow_shell: bool,
    /// Resolve and compare a single import from a specific file.
    #[arg(long)]
    from: Option<PathBuf>,
    /// Import specifier paired with `--from` for single-edge diagnostics.
    #[arg(long = "import")]
    import_specifier: Option<String>,
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

#[derive(Debug, Clone)]
struct GovernanceHashes {
    config_hash: String,
    spec_hash: String,
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
struct InitOutput {
    schema_version: String,
    status: String,
    project_root: String,
    created: Vec<String>,
    skipped_existing: Vec<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    focus: Option<DoctorCompareFocusOutput>,
}

#[derive(Debug, Clone, Serialize)]
struct DoctorCompareFocusOutput {
    from: String,
    import_specifier: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    resolved_to: Option<String>,
    resolution_kind: String,
    in_specgate_graph: bool,
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
        Command::Init(args) => handle_init(args),
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

    // Handle --since blast-radius mode
    let blast_radius = if let Some(since_ref) = &args.since {
        let blast_data = build_blast_radius_data(&loaded);
        let radius = crate::git_blast::compute_blast_radius(
            &loaded.project_root,
            since_ref,
            &blast_data.module_to_files,
            &blast_data.file_to_module,
            &blast_data.importer_graph,
        );

        if let Some(error) = &radius.error {
            return runtime_error_json(
                "git",
                "failed to compute blast radius",
                vec![error.clone()],
            );
        }

        Some(radius)
    } else {
        None
    };

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
    let classified =
        classify_violations(&loaded.project_root, &policy_violations, baseline.as_ref());
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

    let verdict = build_verdict(
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
    );

    let exit_code = match verdict.status {
        VerdictStatus::Pass => EXIT_CODE_PASS,
        VerdictStatus::Fail => EXIT_CODE_POLICY_VIOLATIONS,
    };

    let mut result = CliRunResult::json(exit_code, &verdict);
    if let Some(warning) = deprecation_warning {
        result.stderr = format!("{warning}\n");
    }
    result
}

/// Data needed for blast-radius computation.
struct BlastRadiusData {
    module_to_files: BTreeMap<String, BTreeSet<String>>,
    file_to_module: BTreeMap<String, String>,
    importer_graph: BTreeMap<String, BTreeSet<String>>,
}

fn build_blast_radius_data(loaded: &LoadedProject) -> BlastRadiusData {
    let mut module_to_files: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut file_to_module: BTreeMap<String, String> = BTreeMap::new();

    // Map files to modules based on spec boundaries
    for spec in &loaded.specs {
        let module_id = spec.module.clone();
        let files_set = module_to_files.entry(module_id.clone()).or_default();

        // Include spec file itself
        if let Some(spec_path) = &spec.spec_path {
            let relative = normalize_repo_relative(&loaded.project_root, spec_path);
            files_set.insert(relative.clone());
            file_to_module.insert(relative, module_id.clone());
        }

        // Include files matched by boundaries.path
        if let Some(boundaries) = &spec.boundaries {
            if let Some(path_glob) = &boundaries.path {
                if let Ok(glob) = globset::Glob::new(path_glob) {
                    if let Ok(set) = globset::GlobSetBuilder::new().add(glob).build() {
                        // Walk project files
                        for file_entry in walkdir::WalkDir::new(&loaded.project_root)
                            .follow_links(true)
                            .into_iter()
                            .filter_map(std::result::Result::ok)
                            .filter(|e| e.file_type().is_file())
                        {
                            let relative =
                                normalize_repo_relative(&loaded.project_root, file_entry.path());
                            if set.is_match(&relative) {
                                files_set.insert(relative.clone());
                                file_to_module.insert(relative, module_id.clone());
                            }
                        }
                    }
                }
            }
        }
    }

    // Build importer graph from specs
    let mut importer_graph: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for spec in &loaded.specs {
        if let Some(boundaries) = &spec.boundaries {
            // allow_imports_from lists modules this module imports from
            for import_from in &boundaries.allow_imports_from {
                importer_graph
                    .entry(import_from.clone())
                    .or_default()
                    .insert(spec.module.clone());
            }
        }
    }

    BlastRadiusData {
        module_to_files,
        file_to_module,
        importer_graph,
    }
}

/// Enhanced check handler with diff mode support.
///
/// This function provides diff mode output for comparing violations against baseline.
/// In diff mode, violations are formatted in a git-style diff format where:
/// - New violations are prefixed with `+`
/// - Baseline violations are prefixed with ` ` (space)
pub fn handle_check_with_diff(args: CheckArgs, diff_mode: DiffMode) -> CliRunResult {
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

    let classified = classify_violations(
        &loaded.project_root,
        &artifacts.policy_violations,
        baseline.as_ref(),
    );

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

    // Determine exit code based on new errors
    let has_new_errors = filtered.iter().any(|v| {
        matches!(v.disposition, verdict::ViolationDisposition::New)
            && v.violation.severity == Severity::Error
    });

    let exit_code = if has_new_errors {
        EXIT_CODE_POLICY_VIOLATIONS
    } else {
        EXIT_CODE_PASS
    };

    CliRunResult {
        exit_code,
        stdout: lines.join("\n") + "\n",
        stderr: String::new(),
    }
}

fn handle_init(args: InitArgs) -> CliRunResult {
    let project_root = fs::canonicalize(&args.common.project_root)
        .unwrap_or_else(|_| args.common.project_root.clone());

    if args.module.trim().is_empty() {
        return runtime_error_json("init", "module must be non-empty", Vec::new());
    }

    if let Err(error) = fs::create_dir_all(&project_root) {
        return runtime_error_json(
            "init",
            "failed to prepare project root",
            vec![format!("{}: {error}", project_root.display())],
        );
    }

    let normalized_spec_dir = normalize_path(&args.spec_dir);
    let config_path = project_root.join("specgate.config.yml");
    let spec_file_stem = args.module.trim().replace('/', "__");
    let spec_path = resolve_against_root(
        &project_root,
        &args.spec_dir.join(format!("{spec_file_stem}.spec.yml")),
    );

    let config_content = format!(
        "spec_dirs:\n  - \"{}\"\nexclude: []\ntest_patterns: []\n",
        escape_yaml_double_quoted(&normalized_spec_dir)
    );

    let spec_content = format!(
        "version: \"{}\"\nmodule: \"{}\"\nboundaries:\n  path: \"{}\"\nconstraints: []\n",
        SUPPORTED_SPEC_VERSION,
        escape_yaml_double_quoted(args.module.trim()),
        escape_yaml_double_quoted(args.module_path.trim())
    );

    let mut created = Vec::new();
    let mut skipped_existing = Vec::new();

    if let Err(error) = write_scaffold_file(
        &project_root,
        &config_path,
        &config_content,
        args.force,
        &mut created,
        &mut skipped_existing,
    ) {
        return runtime_error_json("init", "failed to write scaffold", vec![error]);
    }

    if let Err(error) = write_scaffold_file(
        &project_root,
        &spec_path,
        &spec_content,
        args.force,
        &mut created,
        &mut skipped_existing,
    ) {
        return runtime_error_json("init", "failed to write scaffold", vec![error]);
    }

    CliRunResult::json(
        EXIT_CODE_PASS,
        &InitOutput {
            schema_version: "2.2".to_string(),
            status: "ok".to_string(),
            project_root: normalize_path(&project_root),
            created,
            skipped_existing,
        },
    )
}

fn write_scaffold_file(
    project_root: &Path,
    path: &Path,
    content: &str,
    force: bool,
    created: &mut Vec<String>,
    skipped_existing: &mut Vec<String>,
) -> std::result::Result<(), String> {
    let relative = normalize_repo_relative(project_root, path);

    if path.exists() && !force {
        skipped_existing.push(relative);
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create directory {}: {error}", parent.display()))?;
    }

    fs::write(path, content)
        .map_err(|error| format!("failed to write {}: {error}", path.display()))?;

    created.push(relative);
    Ok(())
}

fn escape_yaml_double_quoted(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());

    for ch in value.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            control if control.is_control() => {
                escaped.push_str(&format!("\\u{:04X}", control as u32));
            }
            _ => escaped.push(ch),
        }
    }

    escaped
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

    let baseline = build_baseline_with_metadata(
        &loaded.project_root,
        &artifacts.policy_violations,
        BaselineGeneratedFrom {
            tool_version: build_info::tool_version().to_string(),
            git_sha: build_info::git_sha().to_string(),
            config_hash: governance.config_hash,
            spec_hash: governance.spec_hash,
        },
    );
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

    let focus = match build_doctor_compare_focus(&loaded, &artifacts, &args) {
        Ok(focus) => focus,
        Err(error) => {
            return runtime_error_json(
                "doctor.compare",
                "invalid compare focus options",
                vec![error],
            );
        }
    };

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

    let compare_specgate_edges = filter_edges_for_focus(&artifacts.edge_pairs, focus.as_ref());

    let Some(trace_json) = trace_source.payload else {
        let output = DoctorCompareOutput {
            schema_version: "2.2".to_string(),
            status: "skipped".to_string(),
            configured: trace_source.configured,
            reason: trace_source.reason,
            specgate_edge_count: compare_specgate_edges.len(),
            trace_edge_count: 0,
            missing_in_specgate: Vec::new(),
            extra_in_specgate: Vec::new(),
            focus: focus.as_ref().map(|focus| focus.output.clone()),
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

    let compare_trace_edges = filter_edges_for_focus(&trace_edges, focus.as_ref());

    let missing_in_specgate = compare_trace_edges
        .difference(&compare_specgate_edges)
        .map(|(from, to)| format!("{from} -> {to}"))
        .collect::<Vec<_>>();

    let extra_in_specgate = compare_specgate_edges
        .difference(&compare_trace_edges)
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
        specgate_edge_count: compare_specgate_edges.len(),
        trace_edge_count: compare_trace_edges.len(),
        missing_in_specgate,
        extra_in_specgate,
        focus: focus.as_ref().map(|focus| focus.output.clone()),
    };

    let exit_code = if status == "mismatch" {
        EXIT_CODE_DOCTOR_MISMATCH
    } else {
        EXIT_CODE_PASS
    };

    CliRunResult::json(exit_code, &output)
}

#[derive(Debug, Clone)]
struct DoctorCompareFocus {
    edge: Option<(String, String)>,
    output: DoctorCompareFocusOutput,
}

fn build_doctor_compare_focus(
    loaded: &LoadedProject,
    artifacts: &AnalysisArtifacts,
    args: &DoctorCompareArgs,
) -> std::result::Result<Option<DoctorCompareFocus>, String> {
    match (&args.from, &args.import_specifier) {
        (None, None) => Ok(None),
        (Some(_), None) | (None, Some(_)) => {
            Err("`--from` and `--import` must be provided together".to_string())
        }
        (Some(from), Some(import_specifier)) => {
            let from_file = resolve_against_root(&loaded.project_root, from);
            let from_normalized = normalize_repo_relative(&loaded.project_root, &from_file);

            let mut resolver = ModuleResolver::new(&loaded.project_root, &loaded.specs)
                .map_err(|error| format!("failed to initialize module resolver: {error}"))?;
            let explanation = resolver.explain_resolution(&from_file, import_specifier);

            let (edge, resolved_to, resolution_kind) = match explanation.result {
                ResolvedImport::FirstParty { resolved_path, .. } => {
                    let to = normalize_repo_relative(&loaded.project_root, &resolved_path);
                    (
                        Some((from_normalized.clone(), to.clone())),
                        Some(to),
                        "first_party".to_string(),
                    )
                }
                ResolvedImport::ThirdParty { package_name } => {
                    (None, Some(package_name), "third_party".to_string())
                }
                ResolvedImport::Unresolvable { .. } => (None, None, "unresolvable".to_string()),
            };

            let in_specgate_graph = edge
                .as_ref()
                .is_some_and(|edge| artifacts.edge_pairs.contains(edge));

            Ok(Some(DoctorCompareFocus {
                edge,
                output: DoctorCompareFocusOutput {
                    from: from_normalized,
                    import_specifier: import_specifier.clone(),
                    resolved_to,
                    resolution_kind,
                    in_specgate_graph,
                },
            }))
        }
    }
}

fn filter_edges_for_focus(
    edges: &BTreeSet<(String, String)>,
    focus: Option<&DoctorCompareFocus>,
) -> BTreeSet<(String, String)> {
    if let Some(focus) = focus {
        if let Some(edge) = &focus.edge {
            return edges
                .iter()
                .filter(|candidate| *candidate == edge)
                .cloned()
                .collect();
        }

        return BTreeSet::new();
    }

    edges.clone()
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
        let source = fs::read_to_string(&resolved).map_err(|error| {
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
        if !args.allow_shell {
            return Err(
                "`--tsc-command` executes via `sh -lc`; pass `--allow-shell` to opt in".to_string(),
            );
        }

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
    let Some(path_var) = std::env::var_os("PATH") else {
        return false;
    };

    for directory in std::env::split_paths(&path_var) {
        if directory.join(command).is_file() {
            return true;
        }
    }

    false
}

const TRACE_JSON_MAX_DEPTH: usize = 256;
const TRACE_JSON_MAX_VISITED_NODES: usize = 1_000_000;

fn parse_trace_edges(
    project_root: &Path,
    trace_json: &str,
) -> std::result::Result<BTreeSet<(String, String)>, String> {
    let value: serde_json::Value = serde_json::from_str(trace_json)
        .map_err(|error| format!("trace JSON parse error: {error}"))?;

    let mut edges = BTreeSet::new();
    collect_trace_edges_iterative(project_root, &value, &mut edges)?;
    Ok(edges)
}

fn collect_trace_edges_iterative(
    project_root: &Path,
    root: &serde_json::Value,
    edges: &mut BTreeSet<(String, String)>,
) -> std::result::Result<(), String> {
    let mut stack = vec![(root, 0usize)];
    let mut visited = 0usize;

    while let Some((value, depth)) = stack.pop() {
        visited = visited.saturating_add(1);
        if visited > TRACE_JSON_MAX_VISITED_NODES {
            return Err(format!(
                "trace JSON exceeds maximum traversed nodes ({TRACE_JSON_MAX_VISITED_NODES})"
            ));
        }

        if depth > TRACE_JSON_MAX_DEPTH {
            return Err(format!(
                "trace JSON exceeds maximum supported nesting depth ({TRACE_JSON_MAX_DEPTH})"
            ));
        }

        match value {
            serde_json::Value::Array(items) => {
                for item in items {
                    stack.push((item, depth + 1));
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
                    stack.push((nested, depth + 1));
                }
            }
            _ => {}
        }
    }

    Ok(())
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

fn boundary_constraint_module(violation: &RuleViolation) -> Option<&str> {
    let rule = violation.rule.as_str();

    match rule {
        "boundary.never_imports" | "boundary.allow_imports_from" => {
            violation.from_module.as_deref()
        }
        "boundary.public_api"
        | "boundary.deny_imported_by"
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

fn compute_governance_hashes(
    loaded: &LoadedProject,
) -> std::result::Result<GovernanceHashes, String> {
    let config_value = serde_json::to_value(&loaded.config)
        .map_err(|error| format!("failed to serialize config for hashing: {error}"))?;

    let spec_snapshot = loaded
        .specs
        .iter()
        .map(|spec| HashedSpec {
            module: spec.module.clone(),
            path: spec
                .spec_path
                .as_ref()
                .map(|path| normalize_repo_relative(&loaded.project_root, path))
                .unwrap_or_default(),
            spec: spec.clone(),
        })
        .collect::<Vec<_>>();

    let spec_value = serde_json::to_value(spec_snapshot)
        .map_err(|error| format!("failed to serialize specs for hashing: {error}"))?;

    Ok(GovernanceHashes {
        config_hash: hash_canonical_json(&config_value)
            .map_err(|error| format!("failed to hash config snapshot: {error}"))?,
        spec_hash: hash_canonical_json(&spec_value)
            .map_err(|error| format!("failed to hash spec snapshot: {error}"))?,
    })
}

#[derive(Debug, Serialize)]
struct HashedSpec {
    module: String,
    path: String,
    spec: SpecFile,
}

fn hash_canonical_json(
    value: &serde_json::Value,
) -> std::result::Result<String, serde_json::Error> {
    let canonical = canonicalize_json(value);
    let rendered = serde_json::to_vec(&canonical)?;
    Ok(format!("sha256:{}", stable_hash_hex(rendered)))
}

fn canonicalize_json(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.iter().map(canonicalize_json).collect())
        }
        serde_json::Value::Object(map) => {
            let mut keys = map.keys().cloned().collect::<Vec<_>>();
            keys.sort();

            let mut ordered = serde_json::Map::new();
            for key in keys {
                if let Some(nested) = map.get(&key) {
                    ordered.insert(key, canonicalize_json(nested));
                }
            }

            serde_json::Value::Object(ordered)
        }
        _ => value.clone(),
    }
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
            "--allow-shell",
        ]);

        assert_eq!(result.exit_code, EXIT_CODE_PASS);
        assert!(result.stdout.contains("\"status\": \"skipped\""));
    }

    #[test]
    fn check_output_mode_metrics_includes_timings() {
        let temp = TempDir::new().expect("tempdir");
        write_basic_project(temp.path());

        let result = run([
            "specgate",
            "check",
            "--project-root",
            temp.path().to_str().expect("utf8 path"),
            "--output-mode",
            "metrics",
            "--no-baseline",
        ]);

        assert_eq!(result.exit_code, EXIT_CODE_PASS);
        assert!(result.stdout.contains("\"metrics\""));
    }

    #[test]
    fn boundary_public_api_uses_provider_constraint_severity() {
        let temp = TempDir::new().expect("tempdir");
        write_basic_project(temp.path());

        write_file(
            temp.path(),
            "modules/app.spec.yml",
            "version: \"2.2\"\nmodule: app\nboundaries:\n  path: src/app/**/*\nconstraints:\n  - rule: boundary.public_api\n    severity: error\n",
        );
        write_file(
            temp.path(),
            "modules/core.spec.yml",
            "version: \"2.2\"\nmodule: core\nboundaries:\n  path: src/core/**/*\n  public_api:\n    - src/core/public/**/*\nconstraints:\n  - rule: boundary.public_api\n    severity: warning\n",
        );
        write_file(
            temp.path(),
            "src/core/internal.ts",
            "export const internal = 1;\n",
        );
        write_file(
            temp.path(),
            "src/app/main.ts",
            "import { internal } from '../core/internal';\nexport const app = internal;\n",
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
        assert_eq!(output["summary"]["new_error_violations"], 0);
        assert_eq!(output["summary"]["new_warning_violations"], 1);
        assert_eq!(output["violations"][0]["rule"], "boundary.public_api");
        assert_eq!(output["violations"][0]["severity"], "warning");
    }

    #[test]
    fn escape_yaml_double_quoted_escapes_control_chars() {
        let escaped = escape_yaml_double_quoted("line1\nline2\r\t\"\\");
        assert_eq!(escaped, "line1\\nline2\\r\\t\\\"\\\\");
    }

    #[test]
    fn init_quotes_spec_dir_with_special_chars() {
        let temp = TempDir::new().expect("tempdir");
        let spec_dir = "modules:#prod";

        let result = run([
            "specgate",
            "init",
            "--project-root",
            temp.path().to_str().expect("utf8 path"),
            "--spec-dir",
            spec_dir,
        ]);

        assert_eq!(result.exit_code, EXIT_CODE_PASS);
        let config = fs::read_to_string(temp.path().join("specgate.config.yml"))
            .expect("scaffold config exists");
        assert!(config.contains("spec_dirs:\n  - \"modules:#prod\"\n"));
        assert!(temp.path().join("modules:#prod/app.spec.yml").exists());
    }

    #[test]
    fn init_creates_scaffold_and_then_skips_existing_files() {
        let temp = TempDir::new().expect("tempdir");

        let first = run([
            "specgate",
            "init",
            "--project-root",
            temp.path().to_str().expect("utf8 path"),
        ]);
        assert_eq!(first.exit_code, EXIT_CODE_PASS);
        assert!(temp.path().join("specgate.config.yml").exists());
        assert!(temp.path().join("modules/app.spec.yml").exists());
        assert!(first.stdout.contains("\"created\""));

        let second = run([
            "specgate",
            "init",
            "--project-root",
            temp.path().to_str().expect("utf8 path"),
        ]);
        assert_eq!(second.exit_code, EXIT_CODE_PASS);
        assert!(second.stdout.contains("\"skipped_existing\""));
        assert!(second.stdout.contains("specgate.config.yml"));
    }
}
