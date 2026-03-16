use std::collections::BTreeSet;
use std::path::Path;

use super::DoctorCompareParserMode;
use super::trace_types::{
    ParsedTraceData, ParsedTraceResult, STRUCTURED_TRACE_SCHEMA_VERSION, StructuredTraceEdge,
    StructuredTraceResolution, StructuredTraceSnapshot, TRACE_JSON_MAX_DEPTH,
    TRACE_JSON_MAX_VISITED_NODES, TRACE_STEPS_MAX_LINES, TraceParserKind, TraceResolutionRecord,
    TraceResultKind, has_structured_snapshot_shape, infer_trace_result_kind, json_string_field,
    normalize_trace_path, structured_trace_schema_version,
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
        if snapshot.schema_version != STRUCTURED_TRACE_SCHEMA_VERSION {
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

pub(super) fn parse_trace_data(
    project_root: &Path,
    trace_source: &str,
    parser_mode: DoctorCompareParserMode,
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
        DoctorCompareParserMode::Auto => {
            let parsed =
                parse_structured_trace_data(project_root, trace_source).map_err(|error| {
                    format!("parser mode `auto` failed structured parsing: {error}")
                })?;
            Ok(ParsedTraceResult {
                data: parsed,
                parser_kind: TraceParserKind::StructuredSnapshot,
            })
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
