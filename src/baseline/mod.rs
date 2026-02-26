use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use miette::Diagnostic;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::build_info;
use crate::deterministic::{normalize_repo_relative, stable_fingerprint};
use crate::verdict::{
    FingerprintedViolation, PolicyViolation, ViolationDisposition, sort_policy_violations,
};

pub const BASELINE_FILE_VERSION: &str = "1";
pub const DEFAULT_BASELINE_PATH: &str = ".specgate-baseline.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaselineGeneratedFrom {
    pub tool_version: String,
    pub git_sha: String,
    pub config_hash: String,
    pub spec_hash: String,
}

impl Default for BaselineGeneratedFrom {
    fn default() -> Self {
        Self {
            tool_version: build_info::tool_version().to_string(),
            git_sha: build_info::git_sha().to_string(),
            config_hash: "sha256:unknown".to_string(),
            spec_hash: "sha256:unknown".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaselineFile {
    pub version: String,
    #[serde(default)]
    pub generated_from: BaselineGeneratedFrom,
    pub entries: Vec<BaselineEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaselineEntry {
    pub fingerprint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub positional_fingerprint: Option<String>,
    pub rule: String,
    pub severity: crate::spec::Severity,
    pub message: String,
    pub from_file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_module: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_module: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
}

impl Default for BaselineFile {
    fn default() -> Self {
        Self {
            version: BASELINE_FILE_VERSION.to_string(),
            generated_from: BaselineGeneratedFrom::default(),
            entries: Vec::new(),
        }
    }
}

#[derive(Debug, Error, Diagnostic)]
pub enum BaselineError {
    #[error("failed to read baseline file: {path}")]
    Read {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse baseline file as JSON: {path}")]
    Parse {
        path: std::path::PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("failed to write baseline file: {path}")]
    Write {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to serialize baseline file")]
    Serialize {
        #[source]
        source: serde_json::Error,
    },
}

pub type Result<T> = std::result::Result<T, BaselineError>;

pub fn load_optional_baseline(path: &Path) -> Result<Option<BaselineFile>> {
    if !path.exists() {
        return Ok(None);
    }

    let source = fs::read_to_string(path).map_err(|source| BaselineError::Read {
        path: path.to_path_buf(),
        source,
    })?;

    let mut parsed: BaselineFile =
        serde_json::from_str(&source).map_err(|source| BaselineError::Parse {
            path: path.to_path_buf(),
            source,
        })?;

    sort_baseline_entries(&mut parsed.entries);
    dedup_entries_by_identity(&mut parsed.entries);

    Ok(Some(parsed))
}

pub fn write_baseline(path: &Path, baseline: &BaselineFile) -> Result<()> {
    let mut stable = baseline.clone();
    sort_baseline_entries(&mut stable.entries);
    dedup_entries_by_identity(&mut stable.entries);

    let rendered = serde_json::to_string_pretty(&stable)
        .map_err(|source| BaselineError::Serialize { source })?;

    fs::write(path, format!("{rendered}\n")).map_err(|source| BaselineError::Write {
        path: path.to_path_buf(),
        source,
    })
}

pub fn build_baseline(project_root: &Path, violations: &[PolicyViolation]) -> BaselineFile {
    build_baseline_with_metadata(project_root, violations, BaselineGeneratedFrom::default())
}

pub fn build_baseline_with_metadata(
    project_root: &Path,
    violations: &[PolicyViolation],
    generated_from: BaselineGeneratedFrom,
) -> BaselineFile {
    let mut entries = violations
        .iter()
        .map(|violation| BaselineEntry {
            fingerprint: fingerprint_violation(project_root, violation),
            positional_fingerprint: positional_fingerprint_for_violation(violation),
            rule: violation.rule.clone(),
            severity: violation.severity,
            message: violation.message.clone(),
            from_file: normalize_repo_relative(project_root, &violation.from_file),
            to_file: violation
                .to_file
                .as_ref()
                .map(|path| normalize_repo_relative(project_root, path)),
            from_module: violation.from_module.clone(),
            to_module: violation.to_module.clone(),
            line: violation.line,
            column: violation.column,
        })
        .collect::<Vec<_>>();

    sort_baseline_entries(&mut entries);
    dedup_entries_by_identity(&mut entries);

    BaselineFile {
        version: BASELINE_FILE_VERSION.to_string(),
        generated_from,
        entries,
    }
}

/// Count stale baseline entries without returning the classified violations.
///
/// This is useful when you only need the stale count for summary reporting.
pub fn count_stale_baseline_entries(
    project_root: &Path,
    violations: &[PolicyViolation],
    baseline: Option<&BaselineFile>,
) -> usize {
    let (_, stale_count) = classify_violations_with_stale(project_root, violations, baseline);
    stale_count
}

pub fn classify_violations(
    project_root: &Path,
    violations: &[PolicyViolation],
    baseline: Option<&BaselineFile>,
) -> Vec<FingerprintedViolation> {
    let (classified, _stale_count) =
        classify_violations_with_stale(project_root, violations, baseline);
    classified
}

/// Classify violations against baseline and count stale entries.
///
/// Returns a tuple of (classified violations, stale baseline entry count).
/// Stale entries are baseline entries that no longer match any current violation.
pub fn classify_violations_with_stale(
    project_root: &Path,
    violations: &[PolicyViolation],
    baseline: Option<&BaselineFile>,
) -> (Vec<FingerprintedViolation>, usize) {
    let mut ordered_violations = violations.to_vec();
    sort_policy_violations(&mut ordered_violations);

    let mut remaining_by_primary = baseline.map(build_baseline_match_index).unwrap_or_default();

    let mut remaining_legacy = baseline
        .map(build_legacy_fingerprint_counts)
        .unwrap_or_default();

    let baseline_entry_count = baseline.map(|b| b.entries.len()).unwrap_or(0);

    let mut matched_baseline_entries = 0usize;

    let mut classified = ordered_violations
        .iter()
        .map(|violation| {
            let fingerprint = fingerprint_violation(project_root, violation);
            let positional_fingerprint = positional_fingerprint_for_violation(violation);

            let disposition = if let Some(remaining) = remaining_by_primary.get_mut(&fingerprint) {
                if consume_baseline_match(remaining, positional_fingerprint.as_deref()) {
                    matched_baseline_entries += 1;
                    ViolationDisposition::Baseline
                } else {
                    ViolationDisposition::New
                }
            } else {
                let legacy_fingerprint = legacy_fingerprint_violation(project_root, violation);
                if consume_legacy_fingerprint(&mut remaining_legacy, &legacy_fingerprint) {
                    matched_baseline_entries += 1;
                    ViolationDisposition::Baseline
                } else {
                    ViolationDisposition::New
                }
            };

            FingerprintedViolation {
                violation: violation.clone(),
                fingerprint,
                disposition,
            }
        })
        .collect::<Vec<_>>();

    classified.sort_by(|a, b| {
        a.violation
            .from_file
            .cmp(&b.violation.from_file)
            .then_with(|| a.violation.line.cmp(&b.violation.line))
            .then_with(|| a.violation.column.cmp(&b.violation.column))
            .then_with(|| a.violation.to_file.cmp(&b.violation.to_file))
            .then_with(|| a.violation.rule.cmp(&b.violation.rule))
            .then_with(|| a.fingerprint.cmp(&b.fingerprint))
            .then_with(|| a.violation.message.cmp(&b.violation.message))
    });

    let stale_count = baseline_entry_count.saturating_sub(matched_baseline_entries);

    (classified, stale_count)
}

/// Stable content fingerprint for baseline matching and verdict identity.
///
/// Intentionally excludes line/column so harmless code movement does not invalidate
/// baseline matches.
pub fn fingerprint_violation(project_root: &Path, violation: &PolicyViolation) -> String {
    let from_file = normalize_repo_relative(project_root, &violation.from_file);
    let to_file = violation
        .to_file
        .as_ref()
        .map(|path| normalize_repo_relative(project_root, path));

    stable_content_fingerprint(
        &violation.rule,
        violation.severity,
        &violation.message,
        &from_file,
        to_file.as_deref(),
        violation.from_module.as_deref(),
        violation.to_module.as_deref(),
    )
}

fn build_baseline_match_index(baseline: &BaselineFile) -> BTreeMap<String, Vec<Option<String>>> {
    let mut by_primary = BTreeMap::new();

    for entry in &baseline.entries {
        let primary = stable_content_fingerprint_for_entry(entry);
        by_primary
            .entry(primary)
            .or_insert_with(Vec::new)
            .push(positional_fingerprint_for_entry(entry));
    }

    for remaining in by_primary.values_mut() {
        remaining.sort();
    }

    by_primary
}

fn build_legacy_fingerprint_counts(baseline: &BaselineFile) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for entry in &baseline.entries {
        *counts.entry(entry.fingerprint.clone()).or_insert(0) += 1;
    }
    counts
}

fn stable_content_fingerprint_for_entry(entry: &BaselineEntry) -> String {
    stable_content_fingerprint(
        &entry.rule,
        entry.severity,
        &entry.message,
        &entry.from_file,
        entry.to_file.as_deref(),
        entry.from_module.as_deref(),
        entry.to_module.as_deref(),
    )
}

fn stable_content_fingerprint(
    rule: &str,
    severity: crate::spec::Severity,
    message: &str,
    from_file: &str,
    to_file: Option<&str>,
    from_module: Option<&str>,
    to_module: Option<&str>,
) -> String {
    stable_fingerprint(&[
        rule,
        &format!("{severity:?}"),
        message,
        from_file,
        to_file.unwrap_or_default(),
        from_module.unwrap_or_default(),
        to_module.unwrap_or_default(),
    ])
}

fn positional_fingerprint_for_violation(violation: &PolicyViolation) -> Option<String> {
    positional_fingerprint(violation.line, violation.column)
}

fn positional_fingerprint_for_entry(entry: &BaselineEntry) -> Option<String> {
    entry
        .positional_fingerprint
        .clone()
        .or_else(|| positional_fingerprint(entry.line, entry.column))
}

fn positional_fingerprint(line: Option<u32>, column: Option<u32>) -> Option<String> {
    if line.is_none() && column.is_none() {
        return None;
    }

    let line = line.map(|v| v.to_string()).unwrap_or_default();
    let column = column.map(|v| v.to_string()).unwrap_or_default();
    Some(stable_fingerprint(&[line.as_str(), column.as_str()]))
}

fn legacy_fingerprint_violation(project_root: &Path, violation: &PolicyViolation) -> String {
    let from_file = normalize_repo_relative(project_root, &violation.from_file);
    let to_file = violation
        .to_file
        .as_ref()
        .map(|path| normalize_repo_relative(project_root, path))
        .unwrap_or_default();

    let from_module = violation.from_module.as_deref().unwrap_or_default();
    let to_module = violation.to_module.as_deref().unwrap_or_default();

    let line = violation
        .line
        .map(|line| line.to_string())
        .unwrap_or_default();
    let column = violation
        .column
        .map(|column| column.to_string())
        .unwrap_or_default();

    stable_fingerprint(&[
        violation.rule.as_str(),
        &format!("{:?}", violation.severity),
        violation.message.as_str(),
        from_file.as_str(),
        to_file.as_str(),
        from_module,
        to_module,
        line.as_str(),
        column.as_str(),
    ])
}

fn consume_baseline_match(
    remaining: &mut Vec<Option<String>>,
    positional_fingerprint: Option<&str>,
) -> bool {
    if remaining.is_empty() {
        return false;
    }

    if let Some(target) = positional_fingerprint {
        if let Some(idx) = remaining
            .iter()
            .position(|candidate| candidate.as_deref() == Some(target))
        {
            remaining.remove(idx);
            return true;
        }
    }

    if let Some(idx) = remaining.iter().position(|candidate| candidate.is_none()) {
        remaining.remove(idx);
        return true;
    }

    remaining.remove(0);
    true
}

fn consume_legacy_fingerprint(remaining: &mut BTreeMap<String, usize>, fingerprint: &str) -> bool {
    if let Some(count) = remaining.get_mut(fingerprint)
        && *count > 0
    {
        *count -= 1;
        return true;
    }

    false
}

fn sort_baseline_entries(entries: &mut [BaselineEntry]) {
    entries.sort_by(|a, b| {
        stable_content_fingerprint_for_entry(a)
            .cmp(&stable_content_fingerprint_for_entry(b))
            .then_with(|| {
                positional_fingerprint_for_entry(a).cmp(&positional_fingerprint_for_entry(b))
            })
            .then_with(|| a.rule.cmp(&b.rule))
            .then_with(|| a.from_file.cmp(&b.from_file))
            .then_with(|| a.to_file.cmp(&b.to_file))
            .then_with(|| a.line.cmp(&b.line))
            .then_with(|| a.column.cmp(&b.column))
            .then_with(|| a.message.cmp(&b.message))
    });
}

fn dedup_entries_by_identity(entries: &mut Vec<BaselineEntry>) {
    // `dedup_by` keeps the first adjacent entry and drops following duplicates.
    // After stable sorting this is deterministic, and the field names clarify
    // which entry is retained vs discarded.
    entries.dedup_by(|retained_entry, duplicate_entry| {
        stable_content_fingerprint_for_entry(retained_entry)
            == stable_content_fingerprint_for_entry(duplicate_entry)
            && positional_fingerprint_for_entry(retained_entry)
                == positional_fingerprint_for_entry(duplicate_entry)
    });
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::spec::Severity;
    use crate::verdict::ViolationDisposition;

    use super::*;

    fn violation(message: &str, from_file: &str) -> PolicyViolation {
        PolicyViolation {
            rule: "boundary.never_imports".to_string(),
            severity: Severity::Error,
            message: message.to_string(),
            from_file: PathBuf::from(from_file),
            to_file: Some(PathBuf::from("src/provider/index.ts")),
            from_module: Some("app".to_string()),
            to_module: Some("provider".to_string()),
            line: Some(1),
            column: Some(0),
        }
    }

    #[test]
    fn fingerprints_are_stable() {
        let project_root = Path::new(".");
        let a = fingerprint_violation(project_root, &violation("A", "src/a.ts"));
        let b = fingerprint_violation(project_root, &violation("A", "src/a.ts"));
        assert_eq!(a, b);
    }

    #[test]
    fn baseline_classification_marks_existing_entries_as_report_only() {
        let project_root = Path::new(".");
        let v = violation("A", "src/a.ts");
        let fingerprint = fingerprint_violation(project_root, &v);

        let baseline = BaselineFile {
            version: BASELINE_FILE_VERSION.to_string(),
            generated_from: BaselineGeneratedFrom::default(),
            entries: vec![BaselineEntry {
                fingerprint,
                positional_fingerprint: positional_fingerprint_for_violation(&v),
                rule: v.rule.clone(),
                severity: v.severity,
                message: v.message.clone(),
                from_file: "src/a.ts".to_string(),
                to_file: Some("src/provider/index.ts".to_string()),
                from_module: v.from_module.clone(),
                to_module: v.to_module.clone(),
                line: v.line,
                column: v.column,
            }],
        };

        let classified = classify_violations(project_root, &[v], Some(&baseline));
        assert_eq!(classified.len(), 1);
        assert_eq!(classified[0].disposition, ViolationDisposition::Baseline);
    }

    #[test]
    fn baseline_match_survives_line_movement() {
        let project_root = Path::new(".");

        let mut baseline_violation = violation("A", "src/a.ts");
        baseline_violation.line = Some(12);

        let baseline = BaselineFile {
            version: BASELINE_FILE_VERSION.to_string(),
            generated_from: BaselineGeneratedFrom::default(),
            entries: vec![BaselineEntry {
                fingerprint: legacy_fingerprint_violation(project_root, &baseline_violation),
                positional_fingerprint: None,
                rule: baseline_violation.rule.clone(),
                severity: baseline_violation.severity,
                message: baseline_violation.message.clone(),
                from_file: "src/a.ts".to_string(),
                to_file: Some("src/provider/index.ts".to_string()),
                from_module: baseline_violation.from_module.clone(),
                to_module: baseline_violation.to_module.clone(),
                line: baseline_violation.line,
                column: baseline_violation.column,
            }],
        };

        let mut moved = violation("A", "src/a.ts");
        moved.line = Some(40);

        let classified = classify_violations(project_root, &[moved], Some(&baseline));
        assert_eq!(classified.len(), 1);
        assert_eq!(classified[0].disposition, ViolationDisposition::Baseline);
    }

    #[test]
    fn baseline_matching_uses_positional_discriminator_for_duplicate_counts() {
        let project_root = Path::new(".");

        let mut existing = violation("A", "src/a.ts");
        existing.line = Some(10);

        let baseline = build_baseline(project_root, std::slice::from_ref(&existing));

        let mut new_duplicate = violation("A", "src/a.ts");
        new_duplicate.line = Some(20);

        let classified =
            classify_violations(project_root, &[existing, new_duplicate], Some(&baseline));
        assert_eq!(classified.len(), 2);
        assert_eq!(classified[0].disposition, ViolationDisposition::Baseline);
        assert_eq!(classified[1].disposition, ViolationDisposition::New);
    }

    #[test]
    fn build_baseline_is_sorted_and_deduped() {
        let project_root = Path::new(".");
        let violations = vec![
            violation("B", "src/b.ts"),
            violation("A", "src/a.ts"),
            violation("A", "src/a.ts"),
        ];

        let baseline = build_baseline(project_root, &violations);
        assert_eq!(baseline.entries.len(), 2);

        let mut sorted = baseline.entries.clone();
        sort_baseline_entries(&mut sorted);
        assert_eq!(baseline.entries, sorted);
    }

    #[test]
    fn dedupe_retains_distinct_entries_when_only_position_differs() {
        let project_root = Path::new(".");
        let mut at_line_one = violation("A", "src/a.ts");
        at_line_one.line = Some(1);

        let mut at_line_two = at_line_one.clone();
        at_line_two.line = Some(2);

        let baseline = build_baseline(project_root, &[at_line_one, at_line_two]);
        assert_eq!(baseline.entries.len(), 2);

        assert_ne!(
            baseline.entries[0].positional_fingerprint,
            baseline.entries[1].positional_fingerprint
        );
    }

    #[test]
    fn classify_with_stale_counts_unmatched_baseline_entries() {
        let project_root = Path::new(".");

        // Create baseline with two violations
        let v1 = violation("A", "src/a.ts");
        let v2 = violation("B", "src/b.ts");
        let baseline = build_baseline(project_root, &[v1.clone(), v2.clone()]);
        assert_eq!(baseline.entries.len(), 2);

        // Now classify with only one current violation (v2)
        let (classified, stale_count) =
            classify_violations_with_stale(project_root, &[v2], Some(&baseline));

        assert_eq!(classified.len(), 1);
        assert_eq!(classified[0].disposition, ViolationDisposition::Baseline);
        assert_eq!(stale_count, 1); // v1 is now stale
    }

    #[test]
    fn stale_count_is_zero_when_all_baseline_entries_match() {
        let project_root = Path::new(".");

        let v1 = violation("A", "src/a.ts");
        let v2 = violation("B", "src/b.ts");
        let baseline = build_baseline(project_root, &[v1.clone(), v2.clone()]);

        let (_, stale_count) =
            classify_violations_with_stale(project_root, &[v1, v2], Some(&baseline));

        assert_eq!(stale_count, 0);
    }

    #[test]
    fn stale_count_equals_baseline_size_when_no_current_violations() {
        let project_root = Path::new(".");

        let v1 = violation("A", "src/a.ts");
        let v2 = violation("B", "src/b.ts");
        let baseline = build_baseline(project_root, &[v1, v2]);

        let (_, stale_count) = classify_violations_with_stale(project_root, &[], Some(&baseline));

        assert_eq!(stale_count, 2);
    }

    #[test]
    fn count_stale_baseline_entries_helper_function() {
        let project_root = Path::new(".");

        let v1 = violation("A", "src/a.ts");
        let baseline = build_baseline(project_root, &[v1]);

        let stale_count = count_stale_baseline_entries(project_root, &[], Some(&baseline));
        assert_eq!(stale_count, 1);
    }
}
