use super::*;

#[derive(Debug, serde::Serialize)]
pub(crate) struct HashedSpec {
    pub(crate) module: String,
    pub(crate) path: String,
    pub(crate) spec: crate::spec::SpecFile,
}

pub(crate) fn runtime_error_json(
    code: &str,
    message: &str,
    mut details: Vec<String>,
) -> CliRunResult {
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

pub(crate) fn resolve_against_root(project_root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        project_root.join(path)
    }
}

pub(crate) fn record_timing(timings: &mut BTreeMap<String, u128>, key: &str, start: Instant) {
    timings.insert(key.to_string(), start.elapsed().as_millis());
}

pub(crate) fn compute_governance_hashes(
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

pub(crate) fn compute_telemetry_summary(
    classified: &[verdict::FingerprintedViolation],
    _suppressed_violations: usize,
    stale_baseline_entries: usize,
) -> AnonymizedTelemetrySummary {
    let total_violations = classified.len();
    let new_violations = classified
        .iter()
        .filter(|v| matches!(v.disposition, verdict::ViolationDisposition::New))
        .count();
    let baseline_violations = total_violations.saturating_sub(new_violations);
    let new_error_violations = classified
        .iter()
        .filter(|v| {
            matches!(v.disposition, verdict::ViolationDisposition::New)
                && v.violation.severity == Severity::Error
        })
        .count();
    let new_warning_violations = new_violations.saturating_sub(new_error_violations);

    AnonymizedTelemetrySummary {
        total_violations,
        new_violations,
        baseline_violations,
        new_error_violations,
        new_warning_violations,
        stale_baseline_entries,
        expired_baseline_entries: 0,
    }
}

pub(crate) fn project_fingerprint(project_root: &Path) -> String {
    let canonical =
        std::fs::canonicalize(project_root).unwrap_or_else(|_| project_root.to_path_buf());
    let path_bytes = canonical.as_os_str().as_encoded_bytes();
    format!("sha256:{}", stable_hash_hex(path_bytes))
}

pub(crate) fn hash_canonical_json(
    value: &serde_json::Value,
) -> std::result::Result<String, serde_json::Error> {
    let canonical = canonicalize_json(value);
    let rendered = serde_json::to_vec(&canonical)?;
    Ok(format!("sha256:{}", stable_hash_hex(rendered)))
}

pub(crate) fn canonicalize_json(value: &serde_json::Value) -> serde_json::Value {
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
