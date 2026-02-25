use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::deterministic::normalize_repo_relative;
use crate::spec::Severity;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyViolation {
    pub rule: String,
    pub severity: Severity,
    pub message: String,
    pub from_file: PathBuf,
    pub to_file: Option<PathBuf>,
    pub from_module: Option<String>,
    pub to_module: Option<String>,
    pub line: Option<u32>,
    pub column: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViolationDisposition {
    New,
    Baseline,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FingerprintedViolation {
    pub violation: PolicyViolation,
    pub fingerprint: String,
    pub disposition: ViolationDisposition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum VerdictStatus {
    Pass,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VerdictSummary {
    pub total_violations: usize,
    pub new_violations: usize,
    pub baseline_violations: usize,
    pub suppressed_violations: usize,
    pub error_violations: usize,
    pub warning_violations: usize,
    pub new_error_violations: usize,
    pub new_warning_violations: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VerdictViolation {
    pub rule: String,
    pub severity: Severity,
    pub message: String,
    pub fingerprint: String,
    pub disposition: VerdictDisposition,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum VerdictDisposition {
    New,
    Baseline,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VerdictMetrics {
    pub timings_ms: BTreeMap<String, u128>,
    pub total_ms: u128,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerdictIdentity {
    pub tool_version: String,
    pub git_sha: String,
    pub config_hash: String,
    pub spec_hash: String,
    pub output_mode: String,
    pub spec_files_changed: Vec<String>,
    pub rule_deltas: Vec<String>,
    pub policy_change_detected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Verdict {
    pub schema_version: String,
    pub tool_version: String,
    pub git_sha: String,
    pub config_hash: String,
    pub spec_hash: String,
    pub output_mode: String,
    pub spec_files_changed: Vec<String>,
    pub rule_deltas: Vec<String>,
    pub policy_change_detected: bool,
    pub status: VerdictStatus,
    pub summary: VerdictSummary,
    pub violations: Vec<VerdictViolation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metrics: Option<VerdictMetrics>,
}

pub fn build_verdict(
    project_root: &Path,
    violations: &[FingerprintedViolation],
    suppressed_violations: usize,
    metrics: Option<VerdictMetrics>,
    identity: VerdictIdentity,
) -> Verdict {
    let mut rendered = violations
        .iter()
        .map(|entry| render_violation(project_root, entry))
        .collect::<Vec<_>>();

    rendered.sort_by(|a, b| {
        a.from_file
            .cmp(&b.from_file)
            .then_with(|| a.line.cmp(&b.line))
            .then_with(|| a.column.cmp(&b.column))
            .then_with(|| a.to_file.cmp(&b.to_file))
            .then_with(|| a.rule.cmp(&b.rule))
            .then_with(|| severity_rank(a.severity).cmp(&severity_rank(b.severity)))
            .then_with(|| a.fingerprint.cmp(&b.fingerprint))
            .then_with(|| a.message.cmp(&b.message))
    });

    let summary = summarize(&rendered, suppressed_violations);
    let status = if summary.new_error_violations > 0 {
        VerdictStatus::Fail
    } else {
        VerdictStatus::Pass
    };

    Verdict {
        schema_version: "2.2".to_string(),
        tool_version: identity.tool_version,
        git_sha: identity.git_sha,
        config_hash: identity.config_hash,
        spec_hash: identity.spec_hash,
        output_mode: identity.output_mode,
        spec_files_changed: identity.spec_files_changed,
        rule_deltas: identity.rule_deltas,
        policy_change_detected: identity.policy_change_detected,
        status,
        summary,
        violations: rendered,
        metrics,
    }
}

pub fn sort_policy_violations(violations: &mut [PolicyViolation]) {
    violations.sort_by(|a, b| {
        a.from_file
            .cmp(&b.from_file)
            .then_with(|| a.line.cmp(&b.line))
            .then_with(|| a.column.cmp(&b.column))
            .then_with(|| a.to_file.cmp(&b.to_file))
            .then_with(|| a.from_module.cmp(&b.from_module))
            .then_with(|| a.to_module.cmp(&b.to_module))
            .then_with(|| a.rule.cmp(&b.rule))
            .then_with(|| severity_rank(a.severity).cmp(&severity_rank(b.severity)))
            .then_with(|| a.message.cmp(&b.message))
    });
}

fn render_violation(project_root: &Path, entry: &FingerprintedViolation) -> VerdictViolation {
    VerdictViolation {
        rule: entry.violation.rule.clone(),
        severity: entry.violation.severity,
        message: entry.violation.message.clone(),
        fingerprint: entry.fingerprint.clone(),
        disposition: match entry.disposition {
            ViolationDisposition::New => VerdictDisposition::New,
            ViolationDisposition::Baseline => VerdictDisposition::Baseline,
        },
        from_file: normalize_repo_relative(project_root, &entry.violation.from_file),
        to_file: entry
            .violation
            .to_file
            .as_ref()
            .map(|path| normalize_repo_relative(project_root, path)),
        from_module: entry.violation.from_module.clone(),
        to_module: entry.violation.to_module.clone(),
        line: entry.violation.line,
        column: entry.violation.column,
    }
}

fn severity_rank(severity: Severity) -> u8 {
    match severity {
        Severity::Error => 0,
        Severity::Warning => 1,
    }
}

fn summarize(violations: &[VerdictViolation], suppressed_violations: usize) -> VerdictSummary {
    let total_violations = violations.len();
    let new_violations = violations
        .iter()
        .filter(|violation| matches!(violation.disposition, VerdictDisposition::New))
        .count();
    let baseline_violations = total_violations.saturating_sub(new_violations);

    let error_violations = violations
        .iter()
        .filter(|violation| violation.severity == Severity::Error)
        .count();
    let warning_violations = total_violations.saturating_sub(error_violations);

    let new_error_violations = violations
        .iter()
        .filter(|violation| {
            matches!(violation.disposition, VerdictDisposition::New)
                && violation.severity == Severity::Error
        })
        .count();
    let new_warning_violations = new_violations.saturating_sub(new_error_violations);

    VerdictSummary {
        total_violations,
        new_violations,
        baseline_violations,
        suppressed_violations,
        error_violations,
        warning_violations,
        new_error_violations,
        new_warning_violations,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::Path;

    use super::*;

    fn violation(
        rule: &str,
        severity: Severity,
        from_file: &str,
        message: &str,
    ) -> PolicyViolation {
        PolicyViolation {
            rule: rule.to_string(),
            severity,
            message: message.to_string(),
            from_file: PathBuf::from(from_file),
            to_file: None,
            from_module: Some("app".to_string()),
            to_module: Some("core".to_string()),
            line: Some(1),
            column: Some(0),
        }
    }

    fn identity(output_mode: &str) -> VerdictIdentity {
        VerdictIdentity {
            tool_version: "0.1.0".to_string(),
            git_sha: "abc123".to_string(),
            config_hash: "sha256:config".to_string(),
            spec_hash: "sha256:spec".to_string(),
            output_mode: output_mode.to_string(),
            spec_files_changed: Vec::new(),
            rule_deltas: Vec::new(),
            policy_change_detected: false,
        }
    }

    #[test]
    fn verdict_status_fails_on_new_error_only() {
        let entries = vec![
            FingerprintedViolation {
                violation: violation("boundary.never_imports", Severity::Error, "src/a.ts", "bad"),
                fingerprint: "sha256:a".to_string(),
                disposition: ViolationDisposition::Baseline,
            },
            FingerprintedViolation {
                violation: violation("boundary.public_api", Severity::Warning, "src/b.ts", "warn"),
                fingerprint: "sha256:b".to_string(),
                disposition: ViolationDisposition::New,
            },
        ];

        let verdict = build_verdict(Path::new("."), &entries, 0, None, identity("deterministic"));
        assert_eq!(verdict.status, VerdictStatus::Pass);
        assert_eq!(verdict.summary.new_warning_violations, 1);

        let mut entries_with_error = entries;
        entries_with_error.push(FingerprintedViolation {
            violation: violation("dependency.not_allowed", Severity::Error, "src/c.ts", "bad"),
            fingerprint: "sha256:c".to_string(),
            disposition: ViolationDisposition::New,
        });

        let failing = build_verdict(
            Path::new("."),
            &entries_with_error,
            0,
            None,
            identity("deterministic"),
        );
        assert_eq!(failing.status, VerdictStatus::Fail);
        assert_eq!(failing.summary.new_error_violations, 1);
    }

    #[test]
    fn deterministic_json_omits_metrics_by_default() {
        let entries = vec![FingerprintedViolation {
            violation: violation("boundary.never_imports", Severity::Error, "src/a.ts", "bad"),
            fingerprint: "sha256:a".to_string(),
            disposition: ViolationDisposition::New,
        }];

        let verdict = build_verdict(Path::new("."), &entries, 2, None, identity("deterministic"));
        let rendered = serde_json::to_string(&verdict).expect("serialize");

        assert!(!rendered.contains("metrics"));
        assert!(rendered.contains("suppressed_violations"));
        assert!(rendered.contains("\"tool_version\""));
        assert!(rendered.contains("\"config_hash\""));
        assert!(rendered.contains("\"spec_hash\""));
        assert!(rendered.contains("\"output_mode\":\"deterministic\""));
        assert!(rendered.contains("\"spec_files_changed\":[]"));
        assert!(rendered.contains("\"rule_deltas\":[]"));
        assert!(rendered.contains("\"policy_change_detected\":false"));
    }

    #[test]
    fn metrics_mode_is_serialized_when_present() {
        let entries = vec![FingerprintedViolation {
            violation: violation("boundary.never_imports", Severity::Error, "src/a.ts", "bad"),
            fingerprint: "sha256:a".to_string(),
            disposition: ViolationDisposition::New,
        }];

        let mut timings = BTreeMap::new();
        timings.insert("build_graph".to_string(), 12);

        let verdict = build_verdict(
            Path::new("."),
            &entries,
            0,
            Some(VerdictMetrics {
                timings_ms: timings,
                total_ms: 20,
            }),
            identity("metrics"),
        );

        let rendered = serde_json::to_string(&verdict).expect("serialize");
        assert!(rendered.contains("metrics"));
        assert!(rendered.contains("build_graph"));
        assert!(rendered.contains("\"output_mode\":\"metrics\""));
    }
}
