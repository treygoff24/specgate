//! CLI module for Specgate.
//!
//! This module provides the command-line interface with subcommands for
//! check, validate, init, baseline, and doctor operations.

mod analysis;
mod baseline_cmd;
mod blast;
pub mod check;
mod doctor;
pub mod init;
mod project;
mod severity;
pub mod types;
mod util;
pub mod validate;

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use clap::{Args, Parser, Subcommand};
use serde::{Deserialize, Serialize};

use crate::baseline::{
    BaselineGeneratedFrom, DEFAULT_BASELINE_PATH, build_baseline_with_metadata,
    classify_violations_with_stale, load_optional_baseline, refresh_baseline_with_metadata,
    write_baseline,
};
use crate::build_info;
use crate::deterministic::{normalize_path, normalize_repo_relative, stable_hash_hex};
use crate::graph::DependencyGraph;
use crate::resolver::classify::extract_package_name;
use crate::resolver::nearest_tsconfig_for_dir_uncached;
use crate::resolver::{ModuleResolver, ModuleResolverOptions, ResolvedImport};
use crate::rules::boundary::evaluate_boundary_rules;
use crate::rules::{
    DEPENDENCY_FORBIDDEN_RULE_ID, DEPENDENCY_NOT_ALLOWED_RULE_ID, DependencyRule, RuleContext,
    RuleWithResolver, evaluate_enforce_layer, evaluate_no_circular_deps,
    is_canonical_import_rule_id,
};
use crate::spec::config::{ReleaseChannel, StaleBaselinePolicy};
use crate::spec::{
    self, Severity, SpecConfig, SpecFile, ValidationLevel,
    workspace_discovery::discover_workspace_packages_with_config,
};
use crate::verdict::{
    self, AnonymizedTelemetryEvent, AnonymizedTelemetrySummary, GovernanceContext, PolicyViolation,
    TelemetryEventName, VerdictBuildOptions, VerdictIdentity, VerdictMetrics, VerdictStatus,
    WorkspacePackageInfo, build_verdict_with_options,
};

// Re-export from submodules for convenience
pub(crate) use analysis::*;
pub(crate) use baseline_cmd::*;
pub(crate) use blast::*;
pub use check::{CheckArgs, CheckOutputMode, DiffMode, OutputFormat};
pub use init::InitArgs as InitArgsEnhanced;
pub(crate) use project::*;
pub(crate) use severity::*;
pub use types::*;
pub(crate) use util::*;
pub use validate::ValidateArgs;

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
    Init(init::InitArgs),
    /// Diagnostics and parity checks.
    Doctor(DoctorArgs),
    /// Generate a baseline file for current violations.
    Baseline(BaselineArgs),
}

#[derive(Debug, Clone, Args)]
pub(crate) struct CommonProjectArgs {
    /// Project root containing code + specs + optional specgate.config.yml.
    #[arg(long, default_value = ".")]
    project_root: PathBuf,
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
    /// Parser mode for tsc parity payloads.
    #[arg(long, value_enum, default_value_t = DoctorCompareParserMode::Auto)]
    parser_mode: DoctorCompareParserMode,
    /// Read structured trace snapshot JSON from this path.
    #[arg(long, conflicts_with_all = ["tsc_trace", "tsc_command"])]
    structured_snapshot_in: Option<PathBuf>,
    /// Write normalized structured trace snapshot JSON to this path.
    #[arg(long)]
    structured_snapshot_out: Option<PathBuf>,
    /// Trace payload file: JSON edge data or raw `tsc --traceResolution` output text.
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
#[value(rename_all = "lower")]
enum DoctorCompareParserMode {
    Auto,
    Structured,
    Legacy,
}

impl DoctorCompareParserMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Structured => "structured",
            Self::Legacy => "legacy",
        }
    }
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
    #[serde(skip_serializing_if = "Option::is_none")]
    workspace_packages: Option<Vec<WorkspacePackageInfo>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tsconfig_filename_override: Option<String>,
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
    parity_verdict: String,
    parser_mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    trace_parser: Option<String>,
    configured: bool,
    reason: Option<String>,
    specgate_edge_count: usize,
    trace_edge_count: usize,
    missing_in_specgate: Vec<String>,
    extra_in_specgate: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mismatch_category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    actionable_mismatch_hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    structured_snapshot_in: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    structured_snapshot_out: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    specgate_resolution: Option<DoctorCompareResolutionOutput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tsc_trace_resolution: Option<DoctorCompareResolutionOutput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    focus: Option<DoctorCompareFocusOutput>,
}

#[derive(Debug, Clone, Serialize)]
struct DoctorCompareResolutionOutput {
    source: String,
    #[serde(serialize_with = "serialize_trace_result_kind")]
    result_kind: TraceResultKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    resolved_to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    package_name: Option<String>,
    trace: Vec<String>,
}

fn serialize_trace_result_kind<S>(kind: &TraceResultKind, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(kind.as_str())
}

#[derive(Debug, Clone, Serialize)]
struct DoctorCompareFocusOutput {
    from: String,
    import_specifier: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    resolved_to: Option<String>,
    resolution_kind: String,
    in_specgate_graph: bool,
    specgate_trace: Vec<String>,
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
        Command::Validate(args) => validate::handle_validate(args),
        Command::Check(args) => check::handle_check(args),
        Command::Init(args) => init::handle_init(args),
        Command::Baseline(args) => baseline_cmd::handle_baseline(args),
        Command::Doctor(args) => doctor::handle_doctor(args),
    }
}

#[derive(Debug, Clone)]
struct DoctorCompareFocus {
    edge: Option<(String, String)>,
    output: DoctorCompareFocusOutput,
    specgate_resolution: DoctorCompareResolutionOutput,
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

            let mut resolver = ModuleResolver::new_with_options(
                &loaded.project_root,
                &loaded.specs,
                ModuleResolverOptions {
                    include_dirs: loaded.config.include_dirs.clone(),
                    tsconfig_filename: loaded.config.tsconfig_filename.clone(),
                },
            )
            .map_err(|error| format!("failed to initialize module resolver: {error}"))?;
            let explanation = resolver.explain_resolution(&from_file, import_specifier);
            let specgate_trace = explanation.steps.clone();

            let (edge, resolved_to, resolution_kind) = match &explanation.result {
                ResolvedImport::FirstParty { resolved_path, .. } => {
                    let to = normalize_repo_relative(&loaded.project_root, resolved_path);
                    (
                        Some((from_normalized.clone(), to.clone())),
                        Some(to),
                        "first_party".to_string(),
                    )
                }
                ResolvedImport::ThirdParty { package_name } => {
                    (None, Some(package_name.clone()), "third_party".to_string())
                }
                ResolvedImport::Unresolvable { .. } => (None, None, "unresolvable".to_string()),
            };

            let in_specgate_graph = edge
                .as_ref()
                .is_some_and(|edge| artifacts.edge_pairs.contains(edge));

            let specgate_resolution = doctor_resolution_from_specgate(
                &loaded.project_root,
                &explanation.result,
                specgate_trace.clone(),
            );

            Ok(Some(DoctorCompareFocus {
                edge,
                output: DoctorCompareFocusOutput {
                    from: from_normalized,
                    import_specifier: import_specifier.clone(),
                    resolved_to,
                    resolution_kind,
                    in_specgate_graph,
                    specgate_trace,
                },
                specgate_resolution,
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
    if let Some(snapshot_path) = &args.structured_snapshot_in {
        let resolved = resolve_against_root(project_root, snapshot_path);
        let source = fs::read_to_string(&resolved).map_err(|error| {
            format!(
                "failed to read structured snapshot file {}: {error}",
                resolved.display()
            )
        })?;

        return Ok(TraceSource {
            configured: true,
            payload: Some(source),
            reason: Some(format!(
                "loaded structured snapshot file '{}'",
                normalize_repo_relative(project_root, &resolved)
            )),
        });
    }

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

        if !is_command_available(executable) {
            return Ok(TraceSource {
                configured: true,
                payload: None,
                reason: Some(format!(
                    "executable '{executable}' is not available on PATH; parity check skipped"
                )),
            });
        }

        let output = std::process::Command::new("sh")
            .arg("-lc")
            .arg(command)
            .output()
            .map_err(|error| format!("failed to run command '{command}': {error}"))?;

        if !output.status.success() {
            if output.status.code() == Some(127) {
                return Ok(TraceSource {
                    configured: true,
                    payload: None,
                    reason: Some(format!(
                        "executable '{executable}' was not found at runtime; parity check skipped"
                    )),
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
const TRACE_STEPS_MAX_LINES: usize = 48;
const STRUCTURED_TRACE_SCHEMA_VERSION: &str = "1";

#[derive(Debug, Clone)]
struct ParsedTraceData {
    edges: BTreeSet<(String, String)>,
    resolutions: Vec<TraceResolutionRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TraceResultKind {
    FirstParty,
    ThirdParty,
    Unresolvable,
    NotObserved,
}

impl TraceResultKind {
    fn as_str(&self) -> &'static str {
        match self {
            Self::FirstParty => "first_party",
            Self::ThirdParty => "third_party",
            Self::Unresolvable => "unresolvable",
            Self::NotObserved => "not_observed",
        }
    }

    fn from_str(s: &str) -> Self {
        match s {
            "first_party" => Self::FirstParty,
            "third_party" => Self::ThirdParty,
            "unresolvable" => Self::Unresolvable,
            "not_observed" => Self::NotObserved,
            _ => Self::NotObserved,
        }
    }
}

#[derive(Debug, Clone)]
struct TraceResolutionRecord {
    from: String,
    import_specifier: String,
    result_kind: TraceResultKind,
    resolved_to: Option<String>,
    package_name: Option<String>,
    trace: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TraceParserKind {
    StructuredSnapshot,
    LegacyTraceText,
}

impl TraceParserKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::StructuredSnapshot => "structured_snapshot",
            Self::LegacyTraceText => "legacy_trace_text",
        }
    }
}

#[derive(Debug, Clone)]
struct ParsedTraceResult {
    data: ParsedTraceData,
    parser_kind: TraceParserKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StructuredTraceSnapshot {
    #[serde(default = "structured_trace_schema_version")]
    schema_version: String,
    #[serde(default)]
    edges: Vec<StructuredTraceEdge>,
    #[serde(default)]
    resolutions: Vec<StructuredTraceResolution>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StructuredTraceEdge {
    from: String,
    to: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StructuredTraceResolution {
    from: String,
    #[serde(alias = "import", alias = "specifier")]
    import_specifier: String,
    #[serde(default, alias = "resolution_kind")]
    result_kind: Option<String>,
    #[serde(default, alias = "resolvedTo", alias = "to")]
    resolved_to: Option<String>,
    #[serde(default, alias = "packageName")]
    package_name: Option<String>,
    #[serde(default)]
    trace: Vec<String>,
}

impl StructuredTraceResolution {
    fn to_trace_result_kind(&self) -> TraceResultKind {
        self.result_kind
            .as_deref()
            .map(TraceResultKind::from_str)
            .unwrap_or_else(|| {
                infer_trace_result_kind(self.resolved_to.as_deref(), self.package_name.as_deref())
            })
    }
}

fn structured_trace_schema_version() -> String {
    STRUCTURED_TRACE_SCHEMA_VERSION.to_string()
}

fn has_structured_snapshot_shape(value: &serde_json::Value) -> bool {
    let Some(map) = value.as_object() else {
        return false;
    };

    matches!(map.get("edges"), Some(serde_json::Value::Array(_)))
        || matches!(map.get("resolutions"), Some(serde_json::Value::Array(_)))
}

fn parse_structured_trace_data(
    project_root: &Path,
    trace_source: &str,
) -> std::result::Result<ParsedTraceData, String> {
    let value = serde_json::from_str::<serde_json::Value>(trace_source)
        .map_err(|error| format!("structured snapshot JSON parse failed: {error}"))?;

    if has_structured_snapshot_shape(&value) {
        let snapshot = serde_json::from_value::<StructuredTraceSnapshot>(value)
            .map_err(|error| format!("structured snapshot JSON parse failed: {error}"))?;
        // "1.0.0" is accepted as a migration aid for snapshots generated before the
        // schema_version was normalised to the short form "1". Once all in-flight
        // snapshot files have been regenerated this fallback can be removed.
        // TODO: remove after 2026-04-01 or specgate v2.0.0
        if snapshot.schema_version == "1.0.0" {
            eprintln!(
                "WARNING: schema_version '1.0.0' is deprecated; please regenerate the snapshot file with version '{STRUCTURED_TRACE_SCHEMA_VERSION}'"
            );
        } else if snapshot.schema_version != STRUCTURED_TRACE_SCHEMA_VERSION {
            return Err(format!(
                "structured snapshot schema_version '{}' is not supported (expected '{}')",
                snapshot.schema_version, STRUCTURED_TRACE_SCHEMA_VERSION
            ));
        }
        return Ok(parsed_trace_data_from_structured_snapshot(
            project_root,
            snapshot,
        ));
    }

    let mut edges = BTreeSet::new();
    let mut resolutions = Vec::new();
    collect_trace_data_iterative(project_root, &value, &mut edges, &mut resolutions)?;
    if edges.is_empty() && resolutions.is_empty() {
        return Err(
            "structured snapshot JSON did not contain any trace edges or resolution records"
                .to_string(),
        );
    }

    Ok(ParsedTraceData { edges, resolutions })
}

fn parse_legacy_trace_data(
    project_root: &Path,
    trace_source: &str,
) -> std::result::Result<ParsedTraceData, String> {
    let parsed_text = parse_tsc_trace_text_records(project_root, trace_source);
    if parsed_text.edges.is_empty() && parsed_text.resolutions.is_empty() {
        return Err(
            "legacy parser expected raw `tsc --traceResolution` text with resolvable records"
                .to_string(),
        );
    }

    Ok(parsed_text)
}

fn parse_trace_data(
    project_root: &Path,
    trace_source: &str,
    parser_mode: DoctorCompareParserMode,
    legacy_trace_allowed: bool,
) -> std::result::Result<ParsedTraceResult, String> {
    match parser_mode {
        DoctorCompareParserMode::Structured => {
            let parsed = parse_structured_trace_data(project_root, trace_source)
                .map_err(|error| format!("parser mode `structured` failed: {error}"))?;
            Ok(ParsedTraceResult {
                data: parsed,
                parser_kind: TraceParserKind::StructuredSnapshot,
            })
        }
        DoctorCompareParserMode::Legacy => {
            if !legacy_trace_allowed {
                return Err(
                    "parser mode `legacy` is beta-only right now; enable the beta path before using raw traceResolution text"
                        .to_string(),
                );
            }

            let parsed = parse_legacy_trace_data(project_root, trace_source)
                .map_err(|error| format!("parser mode `legacy` failed: {error}"))?;
            Ok(ParsedTraceResult {
                data: parsed,
                parser_kind: TraceParserKind::LegacyTraceText,
            })
        }
        DoctorCompareParserMode::Auto => {
            match parse_structured_trace_data(project_root, trace_source) {
                Ok(parsed) => Ok(ParsedTraceResult {
                    data: parsed,
                    parser_kind: TraceParserKind::StructuredSnapshot,
                }),
                Err(structured_error) => {
                    if !legacy_trace_allowed {
                        return Err(format!(
                            "parser mode `auto` failed structured parsing: {structured_error}; legacy raw-trace fallback is beta-only and currently disabled"
                        ));
                    }

                    let parsed = parse_legacy_trace_data(project_root, trace_source).map_err(
                        |legacy_error| {
                            format!(
                                "parser mode `auto` failed structured parsing ({structured_error}) and legacy parsing ({legacy_error})"
                            )
                        },
                    )?;
                    Ok(ParsedTraceResult {
                        data: parsed,
                        parser_kind: TraceParserKind::LegacyTraceText,
                    })
                }
            }
        }
    }
}

fn parsed_trace_data_from_structured_snapshot(
    project_root: &Path,
    snapshot: StructuredTraceSnapshot,
) -> ParsedTraceData {
    let edges = snapshot
        .edges
        .into_iter()
        .map(|edge| {
            (
                normalize_trace_path(project_root, &edge.from),
                normalize_trace_path(project_root, &edge.to),
            )
        })
        .collect::<BTreeSet<_>>();

    let resolutions = snapshot
        .resolutions
        .into_iter()
        .map(|resolution| {
            let resolved_to = resolution
                .resolved_to
                .as_deref()
                .map(|raw| normalize_trace_path(project_root, raw));
            let result_kind = resolution.to_trace_result_kind();
            let package_name = resolution.package_name;

            TraceResolutionRecord {
                from: normalize_trace_path(project_root, &resolution.from),
                import_specifier: resolution.import_specifier,
                result_kind,
                resolved_to,
                package_name,
                trace: resolution.trace,
            }
        })
        .collect::<Vec<_>>();

    ParsedTraceData { edges, resolutions }
}

fn structured_snapshot_from_parsed_trace(
    parsed_trace: &ParsedTraceData,
) -> StructuredTraceSnapshot {
    let edges = parsed_trace
        .edges
        .iter()
        .map(|(from, to)| StructuredTraceEdge {
            from: from.clone(),
            to: to.clone(),
        })
        .collect::<Vec<_>>();

    let mut resolutions = parsed_trace
        .resolutions
        .iter()
        .map(|record| StructuredTraceResolution {
            from: record.from.clone(),
            import_specifier: record.import_specifier.clone(),
            result_kind: Some(record.result_kind.as_str().to_string()),
            resolved_to: record.resolved_to.clone(),
            package_name: record.package_name.clone(),
            trace: record.trace.clone(),
        })
        .collect::<Vec<_>>();
    resolutions.sort_by(|a, b| {
        a.from
            .cmp(&b.from)
            .then_with(|| a.import_specifier.cmp(&b.import_specifier))
            .then_with(|| a.result_kind.cmp(&b.result_kind))
            .then_with(|| a.resolved_to.cmp(&b.resolved_to))
            .then_with(|| a.package_name.cmp(&b.package_name))
            .then_with(|| a.trace.cmp(&b.trace))
    });

    StructuredTraceSnapshot {
        schema_version: structured_trace_schema_version(),
        edges,
        resolutions,
    }
}

fn write_structured_snapshot(
    project_root: &Path,
    output_path: &Path,
    parsed_trace: &ParsedTraceData,
) -> std::result::Result<String, String> {
    let resolved = resolve_against_root(project_root, output_path);
    if let Some(parent) = resolved.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "failed to create snapshot output directory {}: {error}",
                parent.display()
            )
        })?;
    }

    let snapshot = structured_snapshot_from_parsed_trace(parsed_trace);
    let rendered = serde_json::to_string_pretty(&snapshot)
        .map_err(|error| format!("failed to serialize structured snapshot JSON: {error}"))?;
    fs::write(&resolved, format!("{rendered}\n")).map_err(|error| {
        format!(
            "failed to write structured snapshot file {}: {error}",
            resolved.display()
        )
    })?;

    Ok(normalize_repo_relative(project_root, &resolved))
}

fn collect_trace_data_iterative(
    project_root: &Path,
    root: &serde_json::Value,
    edges: &mut BTreeSet<(String, String)>,
    resolutions: &mut Vec<TraceResolutionRecord>,
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

                if let (Some(from), Some(import_specifier)) = (
                    map.get("from").and_then(serde_json::Value::as_str),
                    json_string_field(map, &["import", "import_specifier", "specifier"]),
                ) {
                    let resolved_to = json_string_field(map, &["resolved_to", "resolvedTo", "to"])
                        .map(|raw| normalize_trace_path(project_root, raw));
                    let package_name = json_string_field(map, &["package_name", "packageName"])
                        .map(str::to_string);
                    let result_kind = json_string_field(map, &["result_kind", "resolution_kind"])
                        .map(TraceResultKind::from_str)
                        .unwrap_or_else(|| {
                            infer_trace_result_kind(resolved_to.as_deref(), package_name.as_deref())
                        });

                    let trace = map
                        .get("trace")
                        .and_then(serde_json::Value::as_array)
                        .map(|items| {
                            items
                                .iter()
                                .filter_map(serde_json::Value::as_str)
                                .take(TRACE_STEPS_MAX_LINES)
                                .map(str::to_string)
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();

                    resolutions.push(TraceResolutionRecord {
                        from: normalize_trace_path(project_root, from),
                        import_specifier: import_specifier.to_string(),
                        result_kind,
                        resolved_to,
                        package_name,
                        trace,
                    });
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

#[derive(Debug)]
struct PendingTraceResolution {
    from: String,
    import_specifier: String,
    trace: Vec<String>,
    resolved_to: Option<String>,
}

fn parse_tsc_trace_text_records(project_root: &Path, trace_text: &str) -> ParsedTraceData {
    let mut edges = BTreeSet::new();
    let mut resolutions = Vec::new();
    let mut pending: Option<PendingTraceResolution> = None;

    for raw_line in trace_text.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }

        if let Some((import_specifier, from)) = parse_tsc_start_line(line) {
            if let Some(previous) = pending.take() {
                finalize_pending_resolution(previous, &mut edges, &mut resolutions);
            }

            pending = Some(PendingTraceResolution {
                from: normalize_trace_path(project_root, &from),
                import_specifier,
                trace: vec![line.to_string()],
                resolved_to: None,
            });
            continue;
        }

        let Some(current) = pending.as_mut() else {
            continue;
        };

        if current.trace.len() < TRACE_STEPS_MAX_LINES {
            current.trace.push(line.to_string());
        }

        if let Some((resolved_specifier, resolved_to)) = parse_tsc_success_line(line) {
            if resolved_specifier == current.import_specifier {
                current.resolved_to = Some(normalize_trace_path(project_root, &resolved_to));
                if let Some(resolved) = pending.take() {
                    finalize_pending_resolution(resolved, &mut edges, &mut resolutions);
                }
            }
            continue;
        }

        if let Some(unresolved_specifier) = parse_tsc_not_resolved_line(line) {
            if unresolved_specifier == current.import_specifier {
                if let Some(unresolved) = pending.take() {
                    finalize_pending_resolution(unresolved, &mut edges, &mut resolutions);
                }
            }
        }
    }

    if let Some(remaining) = pending.take() {
        finalize_pending_resolution(remaining, &mut edges, &mut resolutions);
    }

    ParsedTraceData { edges, resolutions }
}

fn finalize_pending_resolution(
    pending: PendingTraceResolution,
    edges: &mut BTreeSet<(String, String)>,
    resolutions: &mut Vec<TraceResolutionRecord>,
) {
    let package_name = match pending.resolved_to.as_deref() {
        Some(target) if path_contains_node_modules(target) => {
            Some(extract_package_name(&pending.import_specifier).to_string())
        }
        _ => None,
    };

    let result_kind =
        infer_trace_result_kind(pending.resolved_to.as_deref(), package_name.as_deref());

    if result_kind == TraceResultKind::FirstParty {
        if let Some(to) = &pending.resolved_to {
            edges.insert((pending.from.clone(), to.clone()));
        }
    }

    let mut trace = pending.trace;
    if trace.is_empty() {
        trace.push("no resolution trace lines captured".to_string());
    }

    resolutions.push(TraceResolutionRecord {
        from: pending.from,
        import_specifier: pending.import_specifier,
        result_kind,
        resolved_to: pending.resolved_to,
        package_name,
        trace,
    });
}

fn json_string_field<'a>(
    map: &'a serde_json::Map<String, serde_json::Value>,
    keys: &[&str],
) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| map.get(*key).and_then(serde_json::Value::as_str))
}

fn infer_trace_result_kind(
    resolved_to: Option<&str>,
    package_name: Option<&str>,
) -> TraceResultKind {
    if package_name.is_some() {
        return TraceResultKind::ThirdParty;
    }

    match resolved_to {
        Some(target) if path_contains_node_modules(target) => TraceResultKind::ThirdParty,
        Some(_) => TraceResultKind::FirstParty,
        None => TraceResultKind::Unresolvable,
    }
}

fn parse_tsc_start_line(line: &str) -> Option<(String, String)> {
    let (_, remainder) = line.split_once("Resolving module '")?;
    let (import_specifier, remainder) = remainder.split_once("' from '")?;
    let (from, _) = remainder.split_once('\'')?;
    Some((import_specifier.to_string(), from.to_string()))
}

fn parse_tsc_success_line(line: &str) -> Option<(String, String)> {
    let (_, remainder) = line.split_once("Module name '")?;
    let (import_specifier, remainder) = remainder.split_once("' was successfully resolved to '")?;
    let (resolved_to, _) = remainder.split_once('\'')?;
    Some((import_specifier.to_string(), resolved_to.to_string()))
}

fn parse_tsc_not_resolved_line(line: &str) -> Option<String> {
    let (_, remainder) = line.split_once("Module name '")?;
    let (import_specifier, _) = remainder.split_once("' was not resolved")?;
    Some(import_specifier.to_string())
}

fn path_contains_node_modules(raw: &str) -> bool {
    Path::new(raw)
        .components()
        .any(|component| component.as_os_str() == "node_modules")
}

fn normalize_trace_path(project_root: &Path, raw: &str) -> String {
    let path = Path::new(raw);
    if path.is_absolute() {
        let canonical = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        normalize_repo_relative(project_root, &canonical)
    } else {
        normalize_path(path)
    }
}

fn doctor_resolution_from_specgate(
    project_root: &Path,
    result: &ResolvedImport,
    trace: Vec<String>,
) -> DoctorCompareResolutionOutput {
    match result {
        ResolvedImport::FirstParty {
            resolved_path,
            module_id: _,
        } => DoctorCompareResolutionOutput {
            source: "specgate".to_string(),
            result_kind: TraceResultKind::FirstParty,
            resolved_to: Some(normalize_repo_relative(project_root, resolved_path)),
            package_name: None,
            trace,
        },
        ResolvedImport::ThirdParty { package_name } => DoctorCompareResolutionOutput {
            source: "specgate".to_string(),
            result_kind: TraceResultKind::ThirdParty,
            resolved_to: None,
            package_name: Some(package_name.clone()),
            trace,
        },
        ResolvedImport::Unresolvable { reason, .. } => {
            let mut trace = trace;
            trace.push(format!("resolution error: {reason}"));
            DoctorCompareResolutionOutput {
                source: "specgate".to_string(),
                result_kind: TraceResultKind::Unresolvable,
                resolved_to: None,
                package_name: None,
                trace,
            }
        }
    }
}

fn derive_tsc_focus_resolution(
    parsed_trace: &ParsedTraceData,
    focus: &DoctorCompareFocus,
) -> DoctorCompareResolutionOutput {
    if let Some(record) = parsed_trace.resolutions.iter().rev().find(|record| {
        record.from == focus.output.from && record.import_specifier == focus.output.import_specifier
    }) {
        return DoctorCompareResolutionOutput {
            source: "tsc_trace".to_string(),
            result_kind: record.result_kind.clone(),
            resolved_to: record.resolved_to.clone(),
            package_name: record.package_name.clone(),
            trace: if record.trace.is_empty() {
                vec!["matched trace record without explicit step lines".to_string()]
            } else {
                record.trace.clone()
            },
        };
    }

    if let Some(expected_edge) = &focus.edge {
        if parsed_trace.edges.contains(expected_edge) {
            return DoctorCompareResolutionOutput {
                source: "tsc_trace".to_string(),
                result_kind: TraceResultKind::FirstParty,
                resolved_to: Some(expected_edge.1.clone()),
                package_name: None,
                trace: vec![
                    "no explicit trace stanza matched `--from/--import`; inferred from edge parity"
                        .to_string(),
                ],
            };
        }
    }

    if let Some((_, to)) = parsed_trace
        .edges
        .iter()
        .find(|(from, _)| *from == focus.output.from)
    {
        return DoctorCompareResolutionOutput {
            source: "tsc_trace".to_string(),
            result_kind: TraceResultKind::FirstParty,
            resolved_to: Some(to.clone()),
            package_name: None,
            trace: vec![
                "trace did not carry import-specifier context; using first edge from the same source file"
                    .to_string(),
            ],
        };
    }

    DoctorCompareResolutionOutput {
        source: "tsc_trace".to_string(),
        result_kind: TraceResultKind::NotObserved,
        resolved_to: None,
        package_name: None,
        trace: vec![
            "no matching edge or trace stanza found for `--from/--import` in the supplied trace"
                .to_string(),
        ],
    }
}

fn parity_verdict_for_status(status: &str) -> &'static str {
    match status {
        "match" => "MATCH",
        "mismatch" => "DIFF",
        _ => "SKIPPED",
    }
}

fn classify_doctor_compare_mismatch(
    status: &str,
    focus: Option<&DoctorCompareFocus>,
    specgate_resolution: Option<&DoctorCompareResolutionOutput>,
    tsc_trace_resolution: Option<&DoctorCompareResolutionOutput>,
    missing_in_specgate: &[String],
    extra_in_specgate: &[String],
) -> Option<String> {
    if status != "mismatch" {
        return None;
    }

    let Some(focus) = focus else {
        return Some("edge_set_diff".to_string());
    };

    let (Some(specgate_resolution), Some(tsc_trace_resolution)) =
        (specgate_resolution, tsc_trace_resolution)
    else {
        return Some("focused_unknown".to_string());
    };

    let heuristic_tag =
        classify_focus_mismatch_tag(focus, specgate_resolution, tsc_trace_resolution);

    let category = match (
        &specgate_resolution.result_kind,
        &tsc_trace_resolution.result_kind,
    ) {
        _ if heuristic_tag.is_some() => heuristic_tag.expect("checked is_some"),
        (TraceResultKind::FirstParty, TraceResultKind::FirstParty)
            if specgate_resolution.resolved_to != tsc_trace_resolution.resolved_to =>
        {
            "focused_target_mismatch"
        }
        (
            TraceResultKind::Unresolvable | TraceResultKind::NotObserved,
            TraceResultKind::FirstParty,
        ) => "focused_specgate_missing_resolution",
        (
            TraceResultKind::FirstParty,
            TraceResultKind::Unresolvable | TraceResultKind::NotObserved,
        ) => "focused_tsc_missing_resolution",
        (TraceResultKind::ThirdParty, TraceResultKind::FirstParty)
        | (TraceResultKind::FirstParty, TraceResultKind::ThirdParty) => {
            "focused_classification_mismatch"
        }
        _ if !missing_in_specgate.is_empty() || !extra_in_specgate.is_empty() => {
            "focused_edge_set_diff"
        }
        _ => "focused_resolution_mismatch",
    };

    Some(category.to_string())
}

fn classify_focus_mismatch_tag(
    focus: &DoctorCompareFocus,
    specgate_resolution: &DoctorCompareResolutionOutput,
    tsc_trace_resolution: &DoctorCompareResolutionOutput,
) -> Option<&'static str> {
    let specifier = focus.output.import_specifier.as_str();
    let is_relative = specifier.starts_with("./") || specifier.starts_with("../");

    if is_relative && matches_js_runtime_extension(specifier) {
        return Some("extension_alias");
    }

    if !is_relative {
        if resolution_path_looks_types(specgate_resolution.resolved_to.as_deref())
            || resolution_path_looks_types(tsc_trace_resolution.resolved_to.as_deref())
        {
            return Some("condition_names");
        }

        if specifier.starts_with('@') || specifier.contains('/') {
            return Some("paths");
        }

        return Some("exports");
    }

    None
}

fn matches_js_runtime_extension(specifier: &str) -> bool {
    [".js", ".mjs", ".cjs", ".jsx"]
        .iter()
        .any(|suffix| specifier.ends_with(suffix))
}

fn resolution_path_looks_types(path: Option<&str>) -> bool {
    let Some(path) = path else {
        return false;
    };
    let normalized = path.to_ascii_lowercase();
    normalized.ends_with(".d.ts")
        || normalized.contains("/types/")
        || normalized.contains("/@types/")
        || normalized.contains("index.d.ts")
}

fn build_actionable_mismatch_hint(
    status: &str,
    focus: Option<&DoctorCompareFocus>,
    specgate_resolution: Option<&DoctorCompareResolutionOutput>,
    tsc_trace_resolution: Option<&DoctorCompareResolutionOutput>,
    missing_in_specgate: &[String],
    extra_in_specgate: &[String],
) -> Option<String> {
    if status != "mismatch" {
        return None;
    }

    let shared_guidance = "check tsconfig selection/baseUrl/paths, monorepo project references, `moduleResolution` condition sets, package `exports`, and symlink handling (`preserveSymlinks`)";

    let Some(_focus) = focus else {
        return Some(format!(
            "Edge sets differ. Re-run with `--from <file> --import <specifier>` for targeted diagnosis, then {shared_guidance}."
        ));
    };

    let Some(specgate_resolution) = specgate_resolution else {
        return Some(format!(
            "Focused parity mismatch detected; {shared_guidance}."
        ));
    };

    let Some(tsc_trace_resolution) = tsc_trace_resolution else {
        return Some(format!(
            "Focused parity mismatch detected; {shared_guidance}."
        ));
    };

    match (
        &specgate_resolution.result_kind,
        &tsc_trace_resolution.result_kind,
    ) {
        (TraceResultKind::FirstParty, TraceResultKind::FirstParty)
            if specgate_resolution.resolved_to != tsc_trace_resolution.resolved_to =>
        {
            Some(format!(
                "Both resolvers found first-party targets, but they disagree on the resolved file. Compare path alias precedence and project reference roots; then {shared_guidance}."
            ))
        }
        (
            TraceResultKind::Unresolvable | TraceResultKind::NotObserved,
            TraceResultKind::FirstParty,
        ) => Some(format!(
            "TypeScript resolved this import, but Specgate did not. Verify this command uses the same root tsconfig and project-reference graph; then {shared_guidance}."
        )),
        (
            TraceResultKind::FirstParty,
            TraceResultKind::Unresolvable | TraceResultKind::NotObserved,
        ) => Some(format!(
            "Specgate resolved a first-party edge that TypeScript did not report. Ensure the trace comes from the same build target and includes the importing file; then {shared_guidance}."
        )),
        (TraceResultKind::ThirdParty, TraceResultKind::FirstParty)
        | (TraceResultKind::FirstParty, TraceResultKind::ThirdParty) => Some(format!(
            "Resolver classification differs (first-party vs third-party). Inspect package `exports` conditions, path aliases, and symlinked workspace package links; then {shared_guidance}."
        )),
        _ if !missing_in_specgate.is_empty() || !extra_in_specgate.is_empty() => Some(format!(
            "Focused parity mismatch detected; {shared_guidance}."
        )),
        _ => Some(format!(
            "Focused parity mismatch detected; {shared_guidance}."
        )),
    }
}

fn doctor_compare_beta_channel_enabled(config: &SpecConfig) -> bool {
    config.release_channel == ReleaseChannel::Beta
}

#[derive(Debug, Serialize)]
struct HashedSpec {
    module: String,
    path: String,
    spec: SpecFile,
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

    fn write_basic_project_with_edge(root: &Path) {
        write_basic_project(root);
        write_file(
            root,
            "src/app/main.ts",
            "import { core } from '../core/index';\nexport const app = core;\n",
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
    fn baseline_refresh_rewrites_unknown_governance_hashes() {
        let temp = TempDir::new().expect("tempdir");
        write_basic_project(temp.path());
        write_file(
            temp.path(),
            "legacy-baseline.json",
            r#"{
  "version": "1",
  "generated_from": {
    "tool_version": "legacy",
    "git_sha": "legacy",
    "config_hash": "sha256:unknown",
    "spec_hash": "sha256:unknown"
  },
  "entries": []
}
"#,
        );

        let refreshed = run([
            "specgate",
            "baseline",
            "--project-root",
            temp.path().to_str().expect("utf8 path"),
            "--output",
            "legacy-baseline.json",
            "--refresh",
        ]);
        assert_eq!(refreshed.exit_code, EXIT_CODE_PASS);

        let baseline = fs::read_to_string(temp.path().join("legacy-baseline.json"))
            .expect("refreshed baseline output");
        let parsed = parse_json(&baseline);
        let config_hash = parsed["generated_from"]["config_hash"]
            .as_str()
            .expect("config hash string");
        let spec_hash = parsed["generated_from"]["spec_hash"]
            .as_str()
            .expect("spec hash string");

        assert_ne!(config_hash, "sha256:unknown");
        assert_ne!(spec_hash, "sha256:unknown");
        assert!(config_hash.starts_with("sha256:"));
        assert!(spec_hash.starts_with("sha256:"));
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
            "__specgate_missing_tsc__ --generateTrace .specgate-trace --noEmit",
            "--allow-shell",
        ]);

        assert_eq!(result.exit_code, EXIT_CODE_PASS);
        assert!(result.stdout.contains("\"status\": \"skipped\""));
        assert!(result.stdout.contains("\"parity_verdict\": \"SKIPPED\""));
    }

    #[test]
    fn doctor_compare_legacy_parser_mode_requires_beta_channel() {
        let temp = TempDir::new().expect("tempdir");
        write_basic_project_with_edge(temp.path());
        let from = temp.path().join("src/app/main.ts");
        let to = temp.path().join("src/core/index.ts");
        write_file(
            temp.path(),
            "trace.log",
            &format!(
                "======== Resolving module '../core/index' from '{}'. ========\n======== Module name '../core/index' was successfully resolved to '{}'. ========\n",
                from.display(),
                to.display()
            ),
        );

        let result = run([
            "specgate",
            "doctor",
            "compare",
            "--project-root",
            temp.path().to_str().expect("utf8 path"),
            "--tsc-trace",
            temp.path().join("trace.log").to_str().expect("utf8 path"),
            "--parser-mode",
            "legacy",
        ]);

        assert_eq!(result.exit_code, EXIT_CODE_RUNTIME_ERROR);
        assert!(result.stdout.contains("beta-only"));
    }

    #[test]
    fn doctor_compare_legacy_parser_mode_succeeds_with_beta_channel() {
        let temp = TempDir::new().expect("tempdir");
        write_basic_project_with_edge(temp.path());
        write_file(
            temp.path(),
            "specgate.config.yml",
            "spec_dirs:\n  - modules\nexclude: []\ntest_patterns: []\nrelease_channel: beta\n",
        );
        let from = temp.path().join("src/app/main.ts");
        let to = temp.path().join("src/core/index.ts");
        write_file(
            temp.path(),
            "trace.log",
            &format!(
                "======== Resolving module '../core/index' from '{}'. ========\n======== Module name '../core/index' was successfully resolved to '{}'. ========\n",
                from.display(),
                to.display()
            ),
        );

        let result = run([
            "specgate",
            "doctor",
            "compare",
            "--project-root",
            temp.path().to_str().expect("utf8 path"),
            "--tsc-trace",
            temp.path().join("trace.log").to_str().expect("utf8 path"),
            "--parser-mode",
            "legacy",
        ]);

        assert_eq!(result.exit_code, EXIT_CODE_PASS);
        assert!(result.stdout.contains("\"status\": \"match\""));
        assert!(
            result
                .stdout
                .contains("\"trace_parser\": \"legacy_trace_text\"")
        );
    }

    #[test]
    fn doctor_compare_writes_structured_snapshot_output() {
        let temp = TempDir::new().expect("tempdir");
        write_basic_project_with_edge(temp.path());
        write_file(
            temp.path(),
            "snapshot.json",
            r#"{
  "schema_version": "1",
  "edges": [
    { "from": "src/app/main.ts", "to": "src/core/index.ts" }
  ],
  "resolutions": [
    {
      "from": "src/app/main.ts",
      "import_specifier": "../core/index",
      "resolved_to": "src/core/index.ts"
    }
  ]
}
"#,
        );

        let result = run([
            "specgate",
            "doctor",
            "compare",
            "--project-root",
            temp.path().to_str().expect("utf8 path"),
            "--structured-snapshot-in",
            temp.path()
                .join("snapshot.json")
                .to_str()
                .expect("utf8 path"),
            "--structured-snapshot-out",
            "snapshots/normalized.json",
            "--parser-mode",
            "structured",
        ]);

        assert_eq!(result.exit_code, EXIT_CODE_PASS);
        assert!(
            result
                .stdout
                .contains("\"trace_parser\": \"structured_snapshot\"")
        );
        assert!(
            result
                .stdout
                .contains("\"structured_snapshot_out\": \"snapshots/normalized.json\"")
        );

        let snapshot = fs::read_to_string(temp.path().join("snapshots/normalized.json"))
            .expect("structured snapshot output");
        let parsed = parse_json(&snapshot);
        assert_eq!(parsed["schema_version"], STRUCTURED_TRACE_SCHEMA_VERSION);
        assert_eq!(parsed["edges"][0]["from"], "src/app/main.ts");
        assert_eq!(parsed["edges"][0]["to"], "src/core/index.ts");
    }

    #[test]
    fn doctor_compare_auto_mode_scans_nested_json_trace_payload() {
        let temp = TempDir::new().expect("tempdir");
        write_basic_project_with_edge(temp.path());
        write_file(
            temp.path(),
            "nested-trace.json",
            r#"{
  "metadata": {
    "source": "tsc"
  },
  "payload": {
    "trace": {
      "edges": [
        { "from": "src/app/main.ts", "to": "src/core/index.ts" }
      ],
      "resolutions": [
        {
          "from": "src/app/main.ts",
          "import_specifier": "../core/index",
          "resolved_to": "src/core/index.ts"
        }
      ]
    }
  }
}
"#,
        );

        let result = run([
            "specgate",
            "doctor",
            "compare",
            "--project-root",
            temp.path().to_str().expect("utf8 path"),
            "--tsc-trace",
            temp.path()
                .join("nested-trace.json")
                .to_str()
                .expect("utf8 path"),
            "--parser-mode",
            "auto",
        ]);

        assert_eq!(result.exit_code, EXIT_CODE_PASS);
        assert!(result.stdout.contains("\"status\": \"match\""));
        assert!(result.stdout.contains("\"trace_edge_count\": 1"));
    }

    #[test]
    fn parse_structured_snapshot_keeps_schema_version_validation() {
        let temp = TempDir::new().expect("tempdir");
        let error = parse_structured_trace_data(
            temp.path(),
            r#"{
  "schema_version": "999",
  "edges": [],
  "resolutions": []
}
"#,
        )
        .expect_err("unsupported schema version should fail");

        assert!(error.contains("schema_version '999' is not supported"));
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

    #[test]
    fn init_scaffold_includes_version_2_3_and_empty_contracts() {
        let temp = TempDir::new().expect("tempdir");

        let result = run([
            "specgate",
            "init",
            "--project-root",
            temp.path().to_str().expect("utf8 path"),
        ]);
        assert_eq!(result.exit_code, EXIT_CODE_PASS);

        let spec_path = temp.path().join("modules/app.spec.yml");
        assert!(spec_path.exists(), "scaffold spec file should exist");

        let spec_content = fs::read_to_string(&spec_path).expect("read scaffold spec");

        // Verify scaffold uses current spec version (2.3)
        assert!(
            spec_content.contains("version: \"2.3\""),
            "scaffold should use CURRENT_SPEC_VERSION (2.3), got: {spec_content}"
        );

        // Verify scaffold includes empty contracts array (new in 2.3)
        assert!(
            spec_content.contains("contracts: []"),
            "scaffold should include empty contracts array, got: {spec_content}"
        );

        // Verify scaffold structure
        assert!(spec_content.contains("module: \"app\""));
        assert!(spec_content.contains("boundaries:"));
        assert!(spec_content.contains("constraints: []"));
    }
}
