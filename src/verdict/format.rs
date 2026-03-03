use std::path::Path;

use serde::Serialize;

use crate::deterministic::normalize_repo_relative;
use crate::spec::Severity;

use super::{FingerprintedViolation, ViolationDisposition};

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

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::verdict::PolicyViolation;

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
}
