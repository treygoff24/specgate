use std::collections::BTreeSet;
use std::path::Path;

use crate::resolver::classify::extract_package_name;

use super::DoctorCompareParserMode;
use super::trace_types::{
    ParsedTraceData, ParsedTraceResult, STRUCTURED_TRACE_SCHEMA_VERSION, StructuredTraceEdge,
    StructuredTraceResolution, StructuredTraceSnapshot, TRACE_JSON_MAX_DEPTH,
    TRACE_JSON_MAX_VISITED_NODES, TRACE_STEPS_MAX_LINES, TraceParserKind, TraceResolutionRecord,
    TraceResultKind, has_structured_snapshot_shape, infer_trace_result_kind, json_string_field,
    normalize_trace_path, path_contains_node_modules, structured_trace_schema_version,
};

pub(super) fn parse_structured_trace_data(
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

pub(super) fn parse_legacy_trace_data(
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

pub(super) fn parse_trace_data(
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

pub(super) fn parsed_trace_data_from_structured_snapshot(
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

pub(super) fn structured_snapshot_from_parsed_trace(
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

pub(super) fn collect_trace_data_iterative(
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
pub(super) struct PendingTraceResolution {
    from: String,
    import_specifier: String,
    trace: Vec<String>,
    resolved_to: Option<String>,
}

pub(super) fn parse_tsc_trace_text_records(
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

pub(super) fn finalize_pending_resolution(
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

pub(super) fn parse_tsc_start_line(line: &str) -> Option<(String, String)> {
    let (_, remainder) = line.split_once("Resolving module '")?;
    let (import_specifier, remainder) = remainder.split_once("' from '")?;
    let (from, _) = remainder.split_once('\'')?;
    Some((import_specifier.to_string(), from.to_string()))
}

pub(super) fn parse_tsc_success_line(line: &str) -> Option<(String, String)> {
    let (_, remainder) = line.split_once("Module name '")?;
    let (import_specifier, remainder) = remainder.split_once("' was successfully resolved to '")?;
    let (resolved_to, _) = remainder.split_once('\'')?;
    Some((import_specifier.to_string(), resolved_to.to_string()))
}

pub(super) fn parse_tsc_not_resolved_line(line: &str) -> Option<String> {
    let (_, remainder) = line.split_once("Module name '")?;
    let (import_specifier, _) = remainder.split_once("' was not resolved")?;
    Some(import_specifier.to_string())
}
