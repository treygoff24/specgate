use std::path::Path;

use serde::Serialize;

use crate::deterministic::normalize_repo_relative;
use crate::spec::Severity;

use super::{
    FingerprintedViolation, Verdict, VerdictDisposition, VerdictViolation, ViolationDisposition,
};

/// Render a violation for human-readable output.
pub fn format_violation_human(project_root: &Path, entry: &FingerprintedViolation) -> String {
    let violation = &entry.violation;
    let disposition = match entry.disposition {
        ViolationDisposition::New => "NEW",
        ViolationDisposition::Baseline => "BASELINE",
    };

    let severity_str = match violation.severity {
        Severity::Error => "ERROR",
        Severity::Warning => "WARN",
    };

    let location = if let Some(line) = violation.line {
        let col = violation.column.unwrap_or(0);
        format!(
            "{}:{}:{}",
            normalize_repo_relative(project_root, &violation.from_file),
            line,
            col
        )
    } else {
        normalize_repo_relative(project_root, &violation.from_file)
    };

    let module_context = match (&violation.from_module, &violation.to_module) {
        (Some(from), Some(to)) => format!(" [{from} -> {to}]"),
        (Some(from), None) => format!(" [{from}]"),
        (None, Some(to)) => format!(" [-> {to}]"),
        (None, None) => String::new(),
    };

    let to_context = if let Some(to_file) = &violation.to_file {
        format!(" -> {}", normalize_repo_relative(project_root, to_file))
    } else {
        String::new()
    };

    format!(
        "[{}] {}{}{}: {} - {} [{}] (fingerprint: {})",
        disposition,
        location,
        module_context,
        to_context,
        severity_str,
        violation.message,
        violation.rule,
        &entry.fingerprint[..12.min(entry.fingerprint.len())]
    )
}

/// Render a violation for diff mode (machine-readable, git-style).
pub fn format_violation_diff(project_root: &Path, entry: &FingerprintedViolation) -> String {
    let violation = &entry.violation;
    let prefix = match entry.disposition {
        ViolationDisposition::New => "+",
        ViolationDisposition::Baseline => " ",
    };

    let from_path = normalize_repo_relative(project_root, &violation.from_file);
    let to_path = violation
        .to_file
        .as_ref()
        .map(|p| normalize_repo_relative(project_root, p))
        .unwrap_or_else(|| "-".to_string());

    let line_info = violation.line.map(|l| format!(":{l}")).unwrap_or_default();
    let col_info = violation
        .column
        .map(|c| format!(":{c}"))
        .unwrap_or_default();

    let severity = match violation.severity {
        Severity::Error => "E",
        Severity::Warning => "W",
    };

    let module_from = violation.from_module.as_deref().unwrap_or("-");
    let module_to = violation.to_module.as_deref().unwrap_or("-");

    format!(
        "{}{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
        prefix,
        from_path,
        line_info,
        col_info,
        severity,
        violation.rule,
        module_from,
        module_to,
        to_path,
        violation.message
    )
}

/// Format multiple violations as a summary table.
pub fn format_summary_table(project_root: &Path, violations: &[FingerprintedViolation]) -> String {
    let mut lines = vec![
        "FINGERPRINT\tSEVERITY\tRULE\tLOCATION\tSTATUS".to_string(),
        "───────────\t────────\t────\t────────\t──────".to_string(),
    ];

    for entry in violations {
        let violation = &entry.violation;
        let fp_short = &entry.fingerprint[..12.min(entry.fingerprint.len())];
        let severity = match violation.severity {
            Severity::Error => "ERROR",
            Severity::Warning => "WARN",
        };
        let location = normalize_repo_relative(project_root, &violation.from_file);
        let status = match entry.disposition {
            ViolationDisposition::New => "NEW",
            ViolationDisposition::Baseline => "BASELINE",
        };

        lines.push(format!(
            "{}\t{}\t{}\t{}\t{}",
            fp_short, severity, violation.rule, location, status
        ));
    }

    lines.join("\n")
}

/// Summary statistics for human output.
#[derive(Debug, Clone, Serialize)]
pub struct ViolationStats {
    pub total: usize,
    pub errors: usize,
    pub warnings: usize,
    pub new: usize,
    pub baseline: usize,
}

impl ViolationStats {
    pub fn from_violations(violations: &[FingerprintedViolation]) -> Self {
        let total = violations.len();
        let new = violations
            .iter()
            .filter(|v| matches!(v.disposition, ViolationDisposition::New))
            .count();
        let baseline = total.saturating_sub(new);

        let errors = violations
            .iter()
            .filter(|v| v.violation.severity == Severity::Error)
            .count();
        let warnings = total.saturating_sub(errors);

        Self {
            total,
            errors,
            warnings,
            new,
            baseline,
        }
    }

    pub fn format_human(&self) -> String {
        let mut parts = vec![format!("{} total", self.total)];

        if self.new > 0 {
            parts.push(format!("{} new", self.new));
        }

        if self.baseline > 0 {
            parts.push(format!("{} baseline", self.baseline));
        }

        parts.push(format!("{} errors", self.errors));
        parts.push(format!("{} warnings", self.warnings));

        parts.join(", ")
    }
}

/// Format a verdict for human-readable output.
/// Shows rule, message, hint/suggestion for each violation, plus a summary.
pub fn format_verdict_human(verdict: &Verdict) -> String {
    if verdict.violations.is_empty() {
        return format!("✓ No violations found\n\n{}", format_summary_human(verdict));
    }

    let mut lines = Vec::new();

    for violation in &verdict.violations {
        lines.push(format_violation_human_line(violation));
        lines.push(String::new());
    }

    lines.push(format_summary_human(verdict));

    lines.join("\n")
}

/// Format a single verdict violation for human output.
fn format_violation_human_line(violation: &VerdictViolation) -> String {
    let severity_icon = match violation.severity {
        Severity::Error => "✗",
        Severity::Warning => "⚠",
    };

    let disposition_label = match violation.disposition {
        VerdictDisposition::New => "[NEW] ",
        VerdictDisposition::Baseline => "[BASELINE] ",
    };

    let level = match violation.severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
    };

    let mut lines = vec![format!(
        "{} {}[{}] {}{}",
        severity_icon, level, violation.rule, disposition_label, violation.message
    )];

    let location = if let Some(line) = violation.line {
        let col = violation.column.unwrap_or(0);
        format!("{}:{}:{}", violation.from_file, line, col)
    } else {
        violation.from_file.clone()
    };
    lines.push(format!("  Location: {location}"));

    if let Some(to_file) = &violation.to_file {
        lines.push(format!("  Target: {to_file}"));
    }

    if let Some(from_mod) = &violation.from_module {
        if let Some(to_mod) = &violation.to_module {
            lines.push(format!("  Modules: {from_mod} → {to_mod}"));
        } else {
            lines.push(format!("  Module: {from_mod}"));
        }
    } else if let Some(to_mod) = &violation.to_module {
        lines.push(format!("  Target Module: {to_mod}"));
    }

    if let Some(hint) = &violation.remediation_hint {
        lines.push(format!("  Help: {hint}"));
    } else {
        lines.push("  Help: No remediation hint was attached; review this rule's docs and update spec/config to satisfy the contract.".to_string());
    }

    if let Some(expected) = &violation.expected {
        lines.push(format!("  Expected: {expected}"));
    }

    if let Some(actual) = &violation.actual {
        lines.push(format!("  Actual: {actual}"));
    }

    lines.push(format!("  Fingerprint: {}", violation.fingerprint));

    lines.join("\n")
}

/// Format summary section for human output.
fn format_summary_human(verdict: &Verdict) -> String {
    let mut lines = vec!["Summary:".to_string()];

    let summary = &verdict.summary;
    lines.push(format!("  Total violations: {}", summary.total_violations));

    if summary.new_violations > 0 {
        lines.push(format!("  New violations: {}", summary.new_violations));
    }

    if summary.baseline_violations > 0 {
        lines.push(format!(
            "  Baseline violations: {}",
            summary.baseline_violations
        ));
    }

    if summary.suppressed_violations > 0 {
        lines.push(format!(
            "  Suppressed violations: {}",
            summary.suppressed_violations
        ));
    }

    lines.push(format!("  Errors: {}", summary.error_violations));
    lines.push(format!("  Warnings: {}", summary.warning_violations));

    if summary.stale_baseline_entries > 0 {
        lines.push(format!(
            "  Stale baseline entries: {}",
            summary.stale_baseline_entries
        ));
    }

    lines.push(String::new());
    lines.push(format!("Status: {:?}", verdict.status));

    lines.join("\n")
}

/// Format a verdict as NDJSON (one JSON object per violation per line).
pub fn format_verdict_ndjson(verdict: &Verdict) -> String {
    if verdict.violations.is_empty() {
        return String::new();
    }

    let lines: Vec<String> = verdict
        .violations
        .iter()
        .map(|v| serde_json::to_string(v).unwrap_or_default())
        .collect();

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::verdict::{PolicyViolation, VerdictStatus, VerdictSummary};

    fn test_violation(rule: &str, severity: Severity, from_file: &str) -> PolicyViolation {
        PolicyViolation {
            rule: rule.to_string(),
            severity,
            message: "Test violation".to_string(),
            from_file: PathBuf::from(from_file),
            to_file: Some(PathBuf::from("src/to.ts")),
            from_module: Some("app".to_string()),
            to_module: Some("core".to_string()),
            line: Some(10),
            column: Some(5),
            expected: None,
            actual: None,
            remediation_hint: None,
            contract_id: None,
        }
    }

    fn test_entry(
        rule: &str,
        severity: Severity,
        from_file: &str,
        disposition: ViolationDisposition,
    ) -> FingerprintedViolation {
        FingerprintedViolation {
            violation: test_violation(rule, severity, from_file),
            fingerprint: "sha256:abc123def456789".to_string(),
            disposition,
        }
    }

    fn test_verdict_violation(
        rule: &str,
        severity: Severity,
        from_file: &str,
        disposition: VerdictDisposition,
        message: &str,
    ) -> VerdictViolation {
        VerdictViolation {
            rule: rule.to_string(),
            severity,
            message: message.to_string(),
            fingerprint: "sha256:test123".to_string(),
            disposition,
            from_file: from_file.to_string(),
            to_file: Some("src/to.ts".to_string()),
            from_module: Some("app".to_string()),
            to_module: Some("core".to_string()),
            line: Some(10),
            column: Some(5),
            expected: Some("expected_value".to_string()),
            actual: Some("actual_value".to_string()),
            remediation_hint: Some("Fix this issue".to_string()),
            contract_id: None,
        }
    }

    fn empty_verdict() -> Verdict {
        Verdict {
            verdict_schema: "1".to_string(),
            schema_version: "2.2".to_string(),
            tool_version: "0.1.0".to_string(),
            git_sha: "abc123".to_string(),
            config_hash: "sha256:config".to_string(),
            spec_hash: "sha256:spec".to_string(),
            output_mode: "human".to_string(),
            spec_files_changed: vec![],
            rule_deltas: vec![],
            policy_change_detected: false,
            status: VerdictStatus::Pass,
            summary: VerdictSummary {
                total_violations: 0,
                new_violations: 0,
                baseline_violations: 0,
                suppressed_violations: 0,
                error_violations: 0,
                warning_violations: 0,
                new_error_violations: 0,
                new_warning_violations: 0,
                stale_baseline_entries: 0,
            },
            violations: vec![],
            metrics: None,
            governance: None,
            telemetry: None,
        }
    }

    fn verdict_with_violations(violations: Vec<VerdictViolation>) -> Verdict {
        let total = violations.len();
        let new_count = violations
            .iter()
            .filter(|v| matches!(v.disposition, VerdictDisposition::New))
            .count();
        let error_count = violations
            .iter()
            .filter(|v| v.severity == Severity::Error)
            .count();

        Verdict {
            verdict_schema: "1".to_string(),
            schema_version: "2.2".to_string(),
            tool_version: "0.1.0".to_string(),
            git_sha: "abc123".to_string(),
            config_hash: "sha256:config".to_string(),
            spec_hash: "sha256:spec".to_string(),
            output_mode: "human".to_string(),
            spec_files_changed: vec![],
            rule_deltas: vec![],
            policy_change_detected: false,
            status: if error_count > 0 {
                VerdictStatus::Fail
            } else {
                VerdictStatus::Pass
            },
            summary: VerdictSummary {
                total_violations: total,
                new_violations: new_count,
                baseline_violations: total - new_count,
                suppressed_violations: 0,
                error_violations: error_count,
                warning_violations: total - error_count,
                new_error_violations: new_count,
                new_warning_violations: 0,
                stale_baseline_entries: 0,
            },
            violations,
            metrics: None,
            governance: None,
            telemetry: None,
        }
    }

    #[test]
    fn human_format_includes_all_fields() {
        let entry = test_entry(
            "boundary.never_imports",
            Severity::Error,
            "src/app/main.ts",
            ViolationDisposition::New,
        );
        let output = format_violation_human(Path::new("."), &entry);

        assert!(output.contains("[NEW]"));
        assert!(output.contains("ERROR"));
        assert!(output.contains("boundary.never_imports"));
        assert!(output.contains("app -> core"));
        assert!(output.contains("sha256:abc"));
    }

    #[test]
    fn diff_format_uses_git_style() {
        let entry = test_entry(
            "boundary.never_imports",
            Severity::Error,
            "src/app/main.ts",
            ViolationDisposition::New,
        );
        let output = format_violation_diff(Path::new("."), &entry);

        assert!(output.starts_with("+"));
        assert!(output.contains("\tE\t"));
        assert!(output.contains("boundary.never_imports"));
    }

    #[test]
    fn baseline_violations_have_space_prefix_in_diff() {
        let entry = test_entry(
            "boundary.never_imports",
            Severity::Warning,
            "src/app/main.ts",
            ViolationDisposition::Baseline,
        );
        let output = format_violation_diff(Path::new("."), &entry);

        assert!(output.starts_with(" "));
        assert!(output.contains("\tW\t"));
    }

    #[test]
    fn stats_format_includes_all_counts() {
        let violations = vec![
            test_entry("rule1", Severity::Error, "a.ts", ViolationDisposition::New),
            test_entry(
                "rule2",
                Severity::Warning,
                "b.ts",
                ViolationDisposition::New,
            ),
            test_entry(
                "rule3",
                Severity::Error,
                "c.ts",
                ViolationDisposition::Baseline,
            ),
        ];

        let stats = ViolationStats::from_violations(&violations);
        assert_eq!(stats.total, 3);
        assert_eq!(stats.errors, 2);
        assert_eq!(stats.warnings, 1);
        assert_eq!(stats.new, 2);
        assert_eq!(stats.baseline, 1);

        let human = stats.format_human();
        assert!(human.contains("3 total"));
        assert!(human.contains("2 new"));
        assert!(human.contains("1 baseline"));
    }

    // Tests for format_verdict_human

    #[test]
    fn format_verdict_human_empty_shows_success() {
        let verdict = empty_verdict();
        let output = format_verdict_human(&verdict);

        assert!(output.contains("No violations found"));
        assert!(output.contains("Total violations: 0"));
        assert!(output.contains("Status: Pass"));
    }

    #[test]
    fn format_verdict_human_shows_violations() {
        let violations = vec![test_verdict_violation(
            "boundary.never_imports",
            Severity::Error,
            "src/app/main.ts",
            VerdictDisposition::New,
            "Import not allowed",
        )];
        let verdict = verdict_with_violations(violations);
        let output = format_verdict_human(&verdict);

        assert!(output.contains("boundary.never_imports"));
        assert!(output.contains("Import not allowed"));
        assert!(output.contains("NEW"));
        assert!(output.contains("Error"));
        assert!(output.contains("Location:"));
        assert!(output.contains("Help:"));
        assert!(output.contains("Expected:"));
        assert!(output.contains("Actual:"));
        assert!(output.contains("Fingerprint:"));
    }

    #[test]
    fn format_verdict_human_shows_multiple_violations() {
        let violations = vec![
            test_verdict_violation(
                "rule1",
                Severity::Error,
                "src/a.ts",
                VerdictDisposition::New,
                "Error message",
            ),
            test_verdict_violation(
                "rule2",
                Severity::Warning,
                "src/b.ts",
                VerdictDisposition::Baseline,
                "Warning message",
            ),
        ];
        let verdict = verdict_with_violations(violations);
        let output = format_verdict_human(&verdict);

        assert!(output.contains("rule1"));
        assert!(output.contains("rule2"));
        assert!(output.contains("Error message"));
        assert!(output.contains("Warning message"));
        assert!(output.contains("NEW"));
        assert!(output.contains("BASELINE"));
        assert!(output.contains("Total violations: 2"));
    }

    #[test]
    fn format_verdict_human_includes_summary_stats() {
        let violations = vec![
            test_verdict_violation(
                "rule1",
                Severity::Error,
                "src/a.ts",
                VerdictDisposition::New,
                "Error",
            ),
            test_verdict_violation(
                "rule2",
                Severity::Warning,
                "src/b.ts",
                VerdictDisposition::Baseline,
                "Warning",
            ),
        ];
        let verdict = verdict_with_violations(violations);
        let output = format_verdict_human(&verdict);

        assert!(output.contains("Summary:"));
        assert!(output.contains("Total violations: 2"));
        assert!(output.contains("New violations: 1"));
        assert!(output.contains("Baseline violations: 1"));
        assert!(output.contains("Errors: 1"));
        assert!(output.contains("Warnings: 1"));
    }

    // Tests for format_verdict_ndjson

    #[test]
    fn format_verdict_ndjson_empty_returns_empty() {
        let verdict = empty_verdict();
        let output = format_verdict_ndjson(&verdict);

        assert!(output.is_empty());
    }

    #[test]
    fn format_verdict_ndjson_single_violation() {
        let violations = vec![test_verdict_violation(
            "boundary.never_imports",
            Severity::Error,
            "src/app/main.ts",
            VerdictDisposition::New,
            "Import not allowed",
        )];
        let verdict = verdict_with_violations(violations);
        let output = format_verdict_ndjson(&verdict);

        // Should be valid JSON and contain violation fields
        let parsed: serde_json::Value = serde_json::from_str(&output).expect("Valid JSON");
        assert_eq!(parsed["rule"], "boundary.never_imports");
        assert_eq!(parsed["message"], "Import not allowed");
        assert_eq!(parsed["fingerprint"], "sha256:test123");
    }

    #[test]
    fn format_verdict_ndjson_multiple_violations_one_per_line() {
        let violations = vec![
            test_verdict_violation(
                "rule1",
                Severity::Error,
                "src/a.ts",
                VerdictDisposition::New,
                "Error",
            ),
            test_verdict_violation(
                "rule2",
                Severity::Warning,
                "src/b.ts",
                VerdictDisposition::Baseline,
                "Warning",
            ),
        ];
        let verdict = verdict_with_violations(violations);
        let output = format_verdict_ndjson(&verdict);

        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines.len(), 2);

        // Each line should be valid JSON
        let first: serde_json::Value = serde_json::from_str(lines[0]).expect("Valid JSON");
        let second: serde_json::Value = serde_json::from_str(lines[1]).expect("Valid JSON");

        assert_eq!(first["rule"], "rule1");
        assert_eq!(second["rule"], "rule2");
    }

    #[test]
    fn format_verdict_ndjson_includes_all_violation_fields() {
        let violation = test_verdict_violation(
            "boundary.test",
            Severity::Error,
            "src/test.ts",
            VerdictDisposition::New,
            "Test message",
        );
        let verdict = verdict_with_violations(vec![violation]);
        let output = format_verdict_ndjson(&verdict);

        let parsed: serde_json::Value = serde_json::from_str(&output).expect("Valid JSON");

        assert_eq!(parsed["rule"], "boundary.test");
        assert_eq!(parsed["severity"], "error");
        assert_eq!(parsed["message"], "Test message");
        assert_eq!(parsed["fingerprint"], "sha256:test123");
        assert_eq!(parsed["disposition"], "new");
        assert_eq!(parsed["from_file"], "src/test.ts");
        assert_eq!(parsed["to_file"], "src/to.ts");
        assert_eq!(parsed["from_module"], "app");
        assert_eq!(parsed["to_module"], "core");
        assert_eq!(parsed["line"], 10);
        assert_eq!(parsed["column"], 5);
        assert_eq!(parsed["expected"], "expected_value");
        assert_eq!(parsed["actual"], "actual_value");
        assert_eq!(parsed["remediation_hint"], "Fix this issue");
    }
}
