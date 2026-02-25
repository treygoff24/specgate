use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use miette::Diagnostic;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::deterministic::{normalize_repo_relative, stable_fingerprint};
use crate::verdict::{FingerprintedViolation, PolicyViolation, ViolationDisposition};

pub const BASELINE_FILE_VERSION: &str = "1";
pub const DEFAULT_BASELINE_PATH: &str = ".specgate-baseline.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaselineFile {
    pub version: String,
    pub entries: Vec<BaselineEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaselineEntry {
    pub fingerprint: String,
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

    parsed.entries.sort_by(|a, b| {
        a.fingerprint
            .cmp(&b.fingerprint)
            .then_with(|| a.rule.cmp(&b.rule))
            .then_with(|| a.from_file.cmp(&b.from_file))
            .then_with(|| a.to_file.cmp(&b.to_file))
            .then_with(|| a.line.cmp(&b.line))
            .then_with(|| a.column.cmp(&b.column))
            .then_with(|| a.message.cmp(&b.message))
    });
    parsed
        .entries
        .dedup_by(|left, right| left.fingerprint == right.fingerprint);

    Ok(Some(parsed))
}

pub fn write_baseline(path: &Path, baseline: &BaselineFile) -> Result<()> {
    let mut stable = baseline.clone();
    stable.entries.sort_by(|a, b| {
        a.fingerprint
            .cmp(&b.fingerprint)
            .then_with(|| a.rule.cmp(&b.rule))
            .then_with(|| a.from_file.cmp(&b.from_file))
            .then_with(|| a.to_file.cmp(&b.to_file))
            .then_with(|| a.line.cmp(&b.line))
            .then_with(|| a.column.cmp(&b.column))
            .then_with(|| a.message.cmp(&b.message))
    });
    stable
        .entries
        .dedup_by(|left, right| left.fingerprint == right.fingerprint);

    let rendered = serde_json::to_string_pretty(&stable)
        .map_err(|source| BaselineError::Serialize { source })?;

    fs::write(path, format!("{rendered}\n")).map_err(|source| BaselineError::Write {
        path: path.to_path_buf(),
        source,
    })
}

pub fn build_baseline(project_root: &Path, violations: &[PolicyViolation]) -> BaselineFile {
    let mut entries = violations
        .iter()
        .map(|violation| {
            let fingerprint = fingerprint_violation(project_root, violation);
            BaselineEntry {
                fingerprint,
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
            }
        })
        .collect::<Vec<_>>();

    entries.sort_by(|a, b| {
        a.fingerprint
            .cmp(&b.fingerprint)
            .then_with(|| a.rule.cmp(&b.rule))
            .then_with(|| a.from_file.cmp(&b.from_file))
            .then_with(|| a.to_file.cmp(&b.to_file))
            .then_with(|| a.line.cmp(&b.line))
            .then_with(|| a.column.cmp(&b.column))
            .then_with(|| a.message.cmp(&b.message))
    });
    entries.dedup_by(|left, right| left.fingerprint == right.fingerprint);

    BaselineFile {
        version: BASELINE_FILE_VERSION.to_string(),
        entries,
    }
}

pub fn classify_violations(
    project_root: &Path,
    violations: &[PolicyViolation],
    baseline: Option<&BaselineFile>,
) -> Vec<FingerprintedViolation> {
    let fingerprints = baseline
        .map(|baseline| {
            baseline
                .entries
                .iter()
                .map(|entry| entry.fingerprint.clone())
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();

    let mut classified = violations
        .iter()
        .map(|violation| {
            let fingerprint = fingerprint_violation(project_root, violation);
            let disposition = if fingerprints.contains(&fingerprint) {
                ViolationDisposition::Baseline
            } else {
                ViolationDisposition::New
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

    classified
}

pub fn fingerprint_violation(project_root: &Path, violation: &PolicyViolation) -> String {
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
            entries: vec![BaselineEntry {
                fingerprint,
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
    fn build_baseline_is_sorted_and_deduped() {
        let project_root = Path::new(".");
        let violations = vec![
            violation("B", "src/b.ts"),
            violation("A", "src/a.ts"),
            violation("A", "src/a.ts"),
        ];

        let baseline = build_baseline(project_root, &violations);
        assert_eq!(baseline.entries.len(), 2);
        assert!(baseline.entries[0].from_file <= baseline.entries[1].from_file);
    }
}
