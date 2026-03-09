use std::fs;
use std::path::Path;

use crate::deterministic::normalize_repo_relative;

use super::DoctorCompareArgs;
use super::trace_parser::structured_snapshot_from_parsed_trace;
use super::trace_types::ParsedTraceData;
use crate::cli::resolve_against_root;

#[derive(Debug)]
pub(super) struct TraceSource {
    pub(super) configured: bool,
    pub(super) payload: Option<String>,
    pub(super) reason: Option<String>,
}

pub(super) fn load_trace_source(
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

pub(super) fn is_command_available(command: &str) -> bool {
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

pub(super) fn write_structured_snapshot(
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
