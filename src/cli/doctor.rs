use crate::deterministic::normalize_repo_relative;

use super::*;

#[derive(Debug, Clone, Args)]
pub(crate) struct DoctorArgs {
    #[command(flatten)]
    common: CommonProjectArgs,
    #[command(subcommand)]
    command: Option<DoctorCommand>,
}

#[derive(Debug, Clone, Subcommand)]
pub(crate) enum DoctorCommand {
    /// Compare Specgate dependency edges with a configured trace source.
    Compare(DoctorCompareArgs),
}

#[derive(Debug, Clone, Args)]
pub(crate) struct DoctorCompareArgs {
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
pub(crate) enum DoctorCompareParserMode {
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
pub(crate) struct DoctorOutput {
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
pub(crate) struct DoctorOverlapOutput {
    file: String,
    selected_module: String,
    matched_modules: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct DoctorCompareOutput {
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
pub(crate) struct DoctorCompareResolutionOutput {
    source: String,
    #[serde(serialize_with = "serialize_trace_result_kind")]
    result_kind: TraceResultKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    resolved_to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    package_name: Option<String>,
    trace: Vec<String>,
}

pub(crate) fn serialize_trace_result_kind<S>(
    kind: &TraceResultKind,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(kind.as_str())
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct DoctorCompareFocusOutput {
    from: String,
    import_specifier: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    resolved_to: Option<String>,
    resolution_kind: String,
    in_specgate_graph: bool,
    specgate_trace: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct DoctorCompareFocus {
    edge: Option<(String, String)>,
    output: DoctorCompareFocusOutput,
    specgate_resolution: DoctorCompareResolutionOutput,
}

pub(crate) fn build_doctor_compare_focus(
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

pub(crate) fn filter_edges_for_focus(
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
pub(crate) struct TraceSource {
    configured: bool,
    payload: Option<String>,
    reason: Option<String>,
}

pub(crate) fn load_trace_source(
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

pub(crate) fn is_command_available(command: &str) -> bool {
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

pub(crate) const TRACE_JSON_MAX_DEPTH: usize = 256;
pub(crate) const TRACE_JSON_MAX_VISITED_NODES: usize = 1_000_000;
pub(crate) const TRACE_STEPS_MAX_LINES: usize = 48;
pub(crate) const STRUCTURED_TRACE_SCHEMA_VERSION: &str = "1";

#[derive(Debug, Clone)]
pub(crate) struct ParsedTraceData {
    edges: BTreeSet<(String, String)>,
    resolutions: Vec<TraceResolutionRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TraceResultKind {
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
pub(crate) struct TraceResolutionRecord {
    from: String,
    import_specifier: String,
    result_kind: TraceResultKind,
    resolved_to: Option<String>,
    package_name: Option<String>,
    trace: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TraceParserKind {
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
pub(crate) struct ParsedTraceResult {
    data: ParsedTraceData,
    parser_kind: TraceParserKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct StructuredTraceSnapshot {
    #[serde(default = "structured_trace_schema_version")]
    schema_version: String,
    #[serde(default)]
    edges: Vec<StructuredTraceEdge>,
    #[serde(default)]
    resolutions: Vec<StructuredTraceResolution>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct StructuredTraceEdge {
    from: String,
    to: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct StructuredTraceResolution {
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

pub(crate) fn structured_trace_schema_version() -> String {
    STRUCTURED_TRACE_SCHEMA_VERSION.to_string()
}

pub(crate) fn has_structured_snapshot_shape(value: &serde_json::Value) -> bool {
    let Some(map) = value.as_object() else {
        return false;
    };

    matches!(map.get("edges"), Some(serde_json::Value::Array(_)))
        || matches!(map.get("resolutions"), Some(serde_json::Value::Array(_)))
}

pub(crate) fn parse_structured_trace_data(
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

pub(crate) fn parse_legacy_trace_data(
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

pub(crate) fn parse_trace_data(
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

pub(crate) fn parsed_trace_data_from_structured_snapshot(
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

pub(crate) fn structured_snapshot_from_parsed_trace(
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

pub(crate) fn write_structured_snapshot(
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

pub(crate) fn collect_trace_data_iterative(
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
pub(crate) struct PendingTraceResolution {
    from: String,
    import_specifier: String,
    trace: Vec<String>,
    resolved_to: Option<String>,
}

pub(crate) fn parse_tsc_trace_text_records(
    project_root: &Path,
    trace_text: &str,
) -> ParsedTraceData {
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

pub(crate) fn finalize_pending_resolution(
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

pub(crate) fn json_string_field<'a>(
    map: &'a serde_json::Map<String, serde_json::Value>,
    keys: &[&str],
) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| map.get(*key).and_then(serde_json::Value::as_str))
}

pub(crate) fn infer_trace_result_kind(
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

pub(crate) fn parse_tsc_start_line(line: &str) -> Option<(String, String)> {
    let (_, remainder) = line.split_once("Resolving module '")?;
    let (import_specifier, remainder) = remainder.split_once("' from '")?;
    let (from, _) = remainder.split_once('\'')?;
    Some((import_specifier.to_string(), from.to_string()))
}

pub(crate) fn parse_tsc_success_line(line: &str) -> Option<(String, String)> {
    let (_, remainder) = line.split_once("Module name '")?;
    let (import_specifier, remainder) = remainder.split_once("' was successfully resolved to '")?;
    let (resolved_to, _) = remainder.split_once('\'')?;
    Some((import_specifier.to_string(), resolved_to.to_string()))
}

pub(crate) fn parse_tsc_not_resolved_line(line: &str) -> Option<String> {
    let (_, remainder) = line.split_once("Module name '")?;
    let (import_specifier, _) = remainder.split_once("' was not resolved")?;
    Some(import_specifier.to_string())
}

pub(crate) fn path_contains_node_modules(raw: &str) -> bool {
    Path::new(raw)
        .components()
        .any(|component| component.as_os_str() == "node_modules")
}

pub(crate) fn normalize_trace_path(project_root: &Path, raw: &str) -> String {
    let path = Path::new(raw);
    if path.is_absolute() {
        let canonical = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        normalize_repo_relative(project_root, &canonical)
    } else {
        normalize_path(path)
    }
}

pub(crate) fn doctor_resolution_from_specgate(
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

pub(crate) fn derive_tsc_focus_resolution(
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

pub(crate) fn parity_verdict_for_status(status: &str) -> &'static str {
    match status {
        "match" => "MATCH",
        "mismatch" => "DIFF",
        _ => "SKIPPED",
    }
}

pub(crate) fn classify_doctor_compare_mismatch(
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

pub(crate) fn classify_focus_mismatch_tag(
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

pub(crate) fn matches_js_runtime_extension(specifier: &str) -> bool {
    [".js", ".mjs", ".cjs", ".jsx"]
        .iter()
        .any(|suffix| specifier.ends_with(suffix))
}

pub(crate) fn resolution_path_looks_types(path: Option<&str>) -> bool {
    let Some(path) = path else {
        return false;
    };
    let normalized = path.to_ascii_lowercase();
    normalized.ends_with(".d.ts")
        || normalized.contains("/types/")
        || normalized.contains("/@types/")
        || normalized.contains("index.d.ts")
}

pub(crate) fn build_actionable_mismatch_hint(
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

pub(crate) fn doctor_compare_beta_channel_enabled(config: &SpecConfig) -> bool {
    config.release_channel == ReleaseChannel::Beta
}

pub(super) fn handle_doctor(args: DoctorArgs) -> CliRunResult {
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

    let artifacts = match analyze_project(&loaded, None) {
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

    let workspace_packages = build_workspace_packages_info(&loaded.project_root, &loaded.config);

    let tsconfig_filename_override = if loaded.config.tsconfig_filename != "tsconfig.json" {
        Some(loaded.config.tsconfig_filename.clone())
    } else {
        None
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
        workspace_packages,
        tsconfig_filename_override,
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

    let artifacts = match analyze_project(&loaded, None) {
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

    let legacy_trace_allowed = doctor_compare_beta_channel_enabled(&loaded.config);
    let compare_specgate_edges = filter_edges_for_focus(&artifacts.edge_pairs, focus.as_ref());
    let specgate_resolution = focus
        .as_ref()
        .map(|focus| focus.specgate_resolution.clone());
    let structured_snapshot_in = args.structured_snapshot_in.as_ref().map(|path| {
        normalize_repo_relative(
            &loaded.project_root,
            &resolve_against_root(&loaded.project_root, path),
        )
    });

    let Some(trace_source_payload) = trace_source.payload else {
        let output = DoctorCompareOutput {
            schema_version: "2.2".to_string(),
            status: "skipped".to_string(),
            parity_verdict: "SKIPPED".to_string(),
            parser_mode: args.parser_mode.as_str().to_string(),
            trace_parser: None,
            configured: trace_source.configured,
            reason: trace_source.reason,
            specgate_edge_count: compare_specgate_edges.len(),
            trace_edge_count: 0,
            missing_in_specgate: Vec::new(),
            extra_in_specgate: Vec::new(),
            mismatch_category: None,
            actionable_mismatch_hint: None,
            structured_snapshot_in,
            structured_snapshot_out: None,
            specgate_resolution,
            tsc_trace_resolution: None,
            focus: focus.as_ref().map(|focus| focus.output.clone()),
        };

        return CliRunResult::json(EXIT_CODE_PASS, &output);
    };

    let parsed_trace = match parse_trace_data(
        &loaded.project_root,
        &trace_source_payload,
        args.parser_mode,
        legacy_trace_allowed,
    ) {
        Ok(trace) => trace,
        Err(error) => {
            return runtime_error_json(
                "doctor.compare",
                "failed to parse trace edges",
                vec![error],
            );
        }
    };
    let parsed_trace_data = parsed_trace.data;

    let structured_snapshot_out = match &args.structured_snapshot_out {
        Some(output_path) => {
            match write_structured_snapshot(&loaded.project_root, output_path, &parsed_trace_data) {
                Ok(path) => Some(path),
                Err(error) => {
                    return runtime_error_json(
                        "doctor.compare",
                        "failed to write structured snapshot output",
                        vec![error],
                    );
                }
            }
        }
        None => None,
    };

    let compare_trace_edges = filter_edges_for_focus(&parsed_trace_data.edges, focus.as_ref());

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

    let tsc_trace_resolution = focus
        .as_ref()
        .map(|focus| derive_tsc_focus_resolution(&parsed_trace_data, focus));
    let mismatch_category = classify_doctor_compare_mismatch(
        status,
        focus.as_ref(),
        specgate_resolution.as_ref(),
        tsc_trace_resolution.as_ref(),
        &missing_in_specgate,
        &extra_in_specgate,
    );

    let actionable_mismatch_hint = build_actionable_mismatch_hint(
        status,
        focus.as_ref(),
        specgate_resolution.as_ref(),
        tsc_trace_resolution.as_ref(),
        &missing_in_specgate,
        &extra_in_specgate,
    );

    let output = DoctorCompareOutput {
        schema_version: "2.2".to_string(),
        status: status.to_string(),
        parity_verdict: parity_verdict_for_status(status).to_string(),
        parser_mode: args.parser_mode.as_str().to_string(),
        trace_parser: Some(parsed_trace.parser_kind.as_str().to_string()),
        configured: true,
        reason: trace_source.reason,
        specgate_edge_count: compare_specgate_edges.len(),
        trace_edge_count: compare_trace_edges.len(),
        missing_in_specgate,
        extra_in_specgate,
        mismatch_category,
        actionable_mismatch_hint,
        structured_snapshot_in,
        structured_snapshot_out,
        specgate_resolution,
        tsc_trace_resolution,
        focus: focus.as_ref().map(|focus| focus.output.clone()),
    };

    let exit_code = if status == "mismatch" {
        EXIT_CODE_DOCTOR_MISMATCH
    } else {
        EXIT_CODE_PASS
    };

    CliRunResult::json(exit_code, &output)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

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
}
