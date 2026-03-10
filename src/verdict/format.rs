use std::collections::BTreeMap;
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
    let mut lines = Vec::new();

    if verdict.violations.is_empty() {
        lines.push("✓ No violations found".to_string());
        lines.push(String::new());
    } else {
        for violation in &verdict.violations {
            lines.push(format_violation_human_line(violation));
            lines.push(String::new());
        }
    }

    lines.push(format_summary_human(verdict));

    if let Some(ec) = &verdict.edge_classification {
        lines.push(String::new());
        lines.push("Edge classification:".to_string());
        lines.push(format!(
            "  Resolved: {}  Type-only: {}  External: {}",
            ec.resolved, ec.type_only, ec.external
        ));
        lines.push(format!(
            "  Unresolved (literal): {}  Unresolved (dynamic): {}",
            ec.unresolved_literal, ec.unresolved_dynamic
        ));
    }

    if !verdict.unresolved_edges.is_empty() {
        lines.push(String::new());
        lines.push("Unresolved imports:".to_string());
        for edge in &verdict.unresolved_edges {
            let location = if let Some(line) = edge.line {
                format!("{}:{}", edge.from, line)
            } else {
                edge.from.clone()
            };
            lines.push(format!(
                "  {} -> '{}' ({})",
                location, edge.specifier, edge.kind
            ));
        }
    }

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
    lines.push(format!(
        "  Total violations: {}",
        summary.core.total_violations
    ));

    if summary.core.new_violations > 0 {
        lines.push(format!("  New violations: {}", summary.core.new_violations));
    }

    if summary.core.baseline_violations > 0 {
        lines.push(format!(
            "  Baseline violations: {}",
            summary.core.baseline_violations
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

    if summary.core.stale_baseline_entries > 0 {
        lines.push(format!(
            "  Stale baseline entries: {}",
            summary.core.stale_baseline_entries
        ));
    }

    lines.push(String::new());
    lines.push(format!("Status: {:?}", verdict.status));
    lines.push(format!("Verdict schema: {}", verdict.verdict_schema));

    lines.join("\n")
}

#[derive(Debug, Clone, Serialize)]
struct NdjsonViolationRecord<'a> {
    verdict_schema: &'a str,
    schema_version: &'a str,
    #[serde(flatten)]
    violation: &'a VerdictViolation,
}

/// Format a verdict as NDJSON (one JSON object per violation per line).
pub fn format_verdict_ndjson(verdict: &Verdict) -> String {
    if verdict.violations.is_empty() {
        return String::new();
    }

    let lines: Vec<String> = verdict
        .violations
        .iter()
        .map(|violation| {
            serde_json::to_string(&NdjsonViolationRecord {
                verdict_schema: &verdict.verdict_schema,
                schema_version: &verdict.schema_version,
                violation,
            })
            .unwrap_or_default()
        })
        .collect();

    lines.join("\n")
}

#[derive(Debug, Clone, Serialize)]
struct SarifLog {
    #[serde(rename = "$schema")]
    schema: &'static str,
    version: &'static str,
    runs: Vec<SarifRun>,
}

#[derive(Debug, Clone, Serialize)]
struct SarifRun {
    tool: SarifTool,
    results: Vec<SarifResult>,
}

#[derive(Debug, Clone, Serialize)]
struct SarifTool {
    driver: SarifDriver,
}

#[derive(Debug, Clone, Serialize)]
struct SarifDriver {
    name: &'static str,
    version: String,
    #[serde(rename = "informationUri")]
    information_uri: &'static str,
    rules: Vec<SarifReportingDescriptor>,
}

#[derive(Debug, Clone, Serialize)]
struct SarifReportingDescriptor {
    id: String,
    #[serde(rename = "shortDescription")]
    short_description: SarifMessage,
    #[serde(rename = "defaultConfiguration")]
    default_configuration: SarifConfiguration,
}

#[derive(Debug, Clone, Serialize)]
struct SarifConfiguration {
    level: &'static str,
}

#[derive(Debug, Clone, Serialize)]
struct SarifResult {
    #[serde(rename = "ruleId")]
    rule_id: String,
    level: &'static str,
    message: SarifMessage,
    locations: Vec<SarifLocation>,
    fingerprints: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    properties: Option<SarifProperties>,
}

#[derive(Debug, Clone, Serialize)]
struct SarifProperties {
    edge_type: &'static str,
}

#[derive(Debug, Clone, Serialize)]
struct SarifMessage {
    text: String,
}

#[derive(Debug, Clone, Serialize)]
struct SarifLocation {
    #[serde(rename = "physicalLocation")]
    physical_location: SarifPhysicalLocation,
}

#[derive(Debug, Clone, Serialize)]
struct SarifPhysicalLocation {
    #[serde(rename = "artifactLocation")]
    artifact_location: SarifArtifactLocation,
    #[serde(skip_serializing_if = "Option::is_none")]
    region: Option<SarifRegion>,
}

#[derive(Debug, Clone, Serialize)]
struct SarifArtifactLocation {
    uri: String,
    #[serde(rename = "uriBaseId")]
    uri_base_id: &'static str,
}

#[derive(Debug, Clone, Serialize)]
struct SarifRegion {
    #[serde(rename = "startLine", skip_serializing_if = "Option::is_none")]
    start_line: Option<u32>,
    #[serde(rename = "startColumn", skip_serializing_if = "Option::is_none")]
    start_column: Option<u32>,
}

fn sarif_level(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
    }
}

fn sarif_rule_short_description(rule_id: &str) -> String {
    match rule_id.split_once('.') {
        Some((namespace, name)) if !namespace.is_empty() && !name.is_empty() => {
            let mut chars = namespace.chars();
            let namespace_title = match chars.next() {
                Some(first) => {
                    let mut title = first.to_uppercase().to_string();
                    title.push_str(chars.as_str());
                    title
                }
                None => namespace.to_string(),
            };

            format!("{namespace_title} {name} violation")
        }
        _ => format!("{rule_id} violation"),
    }
}

/// Format a verdict as SARIF 2.1.0.
pub fn format_verdict_sarif(verdict: &Verdict) -> String {
    let mut rule_levels: BTreeMap<String, Severity> = BTreeMap::new();

    for violation in &verdict.violations {
        rule_levels
            .entry(violation.rule.clone())
            .and_modify(|level| {
                if violation.severity == Severity::Error {
                    *level = Severity::Error;
                }
            })
            .or_insert(violation.severity);
    }

    let rules = rule_levels
        .into_iter()
        .map(|(rule_id, severity)| SarifReportingDescriptor {
            id: rule_id.clone(),
            short_description: SarifMessage {
                text: sarif_rule_short_description(&rule_id),
            },
            default_configuration: SarifConfiguration {
                level: sarif_level(severity),
            },
        })
        .collect();

    let results = verdict
        .violations
        .iter()
        .map(|violation| {
            let mut fingerprints = BTreeMap::new();
            fingerprints.insert("specgate/v1".to_string(), violation.fingerprint.clone());

            let region = if violation.line.is_some() || violation.column.is_some() {
                Some(SarifRegion {
                    start_line: violation.line,
                    start_column: violation.column,
                })
            } else {
                None
            };

            SarifResult {
                rule_id: violation.rule.clone(),
                level: sarif_level(violation.severity),
                message: SarifMessage {
                    text: violation.message.clone(),
                },
                locations: vec![SarifLocation {
                    physical_location: SarifPhysicalLocation {
                        artifact_location: SarifArtifactLocation {
                            uri: violation.from_file.clone(),
                            uri_base_id: "%SRCROOT%",
                        },
                        region,
                    },
                }],
                fingerprints,
                properties: violation.edge_type.map(|edge_type| SarifProperties {
                    edge_type: edge_type.as_str(),
                }),
            }
        })
        .collect();

    let sarif = SarifLog {
        schema: "https://raw.githubusercontent.com/oasis-tcs/sarif-spec/main/sarif-2.1/schema/sarif-schema-2.1.0.json",
        version: "2.1.0",
        runs: vec![SarifRun {
            tool: SarifTool {
                driver: SarifDriver {
                    name: "specgate",
                    version: verdict.tool_version.clone(),
                    information_uri: "https://github.com/treygoff24/specgate",
                    rules,
                },
            },
            results,
        }],
    };

    serde_json::to_string_pretty(&sarif).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::verdict::{
        AnonymizedTelemetrySummary, PolicyViolation, VERDICT_SCHEMA_VERSION, VerdictStatus,
        VerdictSummary,
    };

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
            edge_type: None,
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
            edge_type: None,
        }
    }

    fn empty_verdict() -> Verdict {
        Verdict {
            verdict_schema: VERDICT_SCHEMA_VERSION.to_string(),
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
                core: AnonymizedTelemetrySummary {
                    total_violations: 0,
                    new_violations: 0,
                    baseline_violations: 0,
                    new_error_violations: 0,
                    new_warning_violations: 0,
                    stale_baseline_entries: 0,
                    expired_baseline_entries: 0,
                },
                suppressed_violations: 0,
                error_violations: 0,
                warning_violations: 0,
            },
            violations: vec![],
            metrics: None,
            governance: None,
            telemetry: None,
            workspace_packages: None,
            edge_classification: None,
            edges: Vec::new(),
            unresolved_edges: Vec::new(),
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
            verdict_schema: VERDICT_SCHEMA_VERSION.to_string(),
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
                core: AnonymizedTelemetrySummary {
                    total_violations: total,
                    new_violations: new_count,
                    baseline_violations: total - new_count,
                    new_error_violations: new_count,
                    new_warning_violations: 0,
                    stale_baseline_entries: 0,
                    expired_baseline_entries: 0,
                },
                suppressed_violations: 0,
                error_violations: error_count,
                warning_violations: total - error_count,
            },
            violations,
            metrics: None,
            governance: None,
            telemetry: None,
            workspace_packages: None,
            edge_classification: None,
            edges: Vec::new(),
            unresolved_edges: Vec::new(),
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
        assert!(output.contains("Verdict schema: 1.0"));
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
        assert_eq!(parsed["verdict_schema"], VERDICT_SCHEMA_VERSION);
        assert_eq!(parsed["schema_version"], "2.2");
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

        assert_eq!(parsed["verdict_schema"], VERDICT_SCHEMA_VERSION);
        assert_eq!(parsed["schema_version"], "2.2");
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

    // Tests for format_verdict_sarif

    #[test]
    fn format_verdict_sarif_empty_violations_has_zero_results() {
        let verdict = empty_verdict();
        let output = format_verdict_sarif(&verdict);
        let parsed: serde_json::Value = serde_json::from_str(&output).expect("Valid JSON");

        assert_eq!(parsed["version"], "2.1.0");
        assert_eq!(
            parsed["runs"][0]["results"]
                .as_array()
                .expect("results")
                .len(),
            0
        );
        assert_eq!(
            parsed["runs"][0]["tool"]["driver"]["rules"]
                .as_array()
                .expect("rules")
                .len(),
            0
        );
    }

    #[test]
    fn format_verdict_sarif_single_violation_includes_location_line_column() {
        let violations = vec![test_verdict_violation(
            "boundary.never_imports",
            Severity::Error,
            "src/app/main.ts",
            VerdictDisposition::New,
            "Import not allowed",
        )];
        let verdict = verdict_with_violations(violations);
        let output = format_verdict_sarif(&verdict);
        let parsed: serde_json::Value = serde_json::from_str(&output).expect("Valid JSON");

        assert_eq!(
            parsed["runs"][0]["results"][0]["locations"][0]["physicalLocation"]["artifactLocation"]
                ["uri"],
            "src/app/main.ts"
        );
        assert_eq!(
            parsed["runs"][0]["results"][0]["locations"][0]["physicalLocation"]["artifactLocation"]
                ["uriBaseId"],
            "%SRCROOT%"
        );
        assert_eq!(
            parsed["runs"][0]["results"][0]["locations"][0]["physicalLocation"]["region"]["startLine"],
            10
        );
        assert_eq!(
            parsed["runs"][0]["results"][0]["locations"][0]["physicalLocation"]["region"]["startColumn"],
            5
        );
    }

    #[test]
    fn format_verdict_sarif_multiple_rules_have_descriptors() {
        let violations = vec![
            test_verdict_violation(
                "boundary.never_imports",
                Severity::Error,
                "src/a.ts",
                VerdictDisposition::New,
                "Error",
            ),
            test_verdict_violation(
                "boundary.public_api",
                Severity::Error,
                "src/b.ts",
                VerdictDisposition::New,
                "Error",
            ),
        ];
        let verdict = verdict_with_violations(violations);
        let output = format_verdict_sarif(&verdict);
        let parsed: serde_json::Value = serde_json::from_str(&output).expect("Valid JSON");

        let rules = parsed["runs"][0]["tool"]["driver"]["rules"]
            .as_array()
            .expect("rules array");
        assert_eq!(rules.len(), 2);

        let ids = rules
            .iter()
            .map(|rule| rule["id"].as_str().expect("rule id"))
            .collect::<Vec<_>>();
        assert!(ids.contains(&"boundary.never_imports"));
        assert!(ids.contains(&"boundary.public_api"));
    }

    #[test]
    fn format_verdict_sarif_warning_uses_warning_level() {
        let violations = vec![test_verdict_violation(
            "boundary.never_imports",
            Severity::Warning,
            "src/app/main.ts",
            VerdictDisposition::New,
            "Import warning",
        )];
        let verdict = verdict_with_violations(violations);
        let output = format_verdict_sarif(&verdict);
        let parsed: serde_json::Value = serde_json::from_str(&output).expect("Valid JSON");

        assert_eq!(parsed["runs"][0]["results"][0]["level"], "warning");
    }

    #[test]
    fn format_verdict_sarif_includes_fingerprints() {
        let violations = vec![test_verdict_violation(
            "boundary.never_imports",
            Severity::Error,
            "src/app/main.ts",
            VerdictDisposition::New,
            "Import not allowed",
        )];
        let verdict = verdict_with_violations(violations);
        let output = format_verdict_sarif(&verdict);
        let parsed: serde_json::Value = serde_json::from_str(&output).expect("Valid JSON");

        assert_eq!(
            parsed["runs"][0]["results"][0]["fingerprints"]["specgate/v1"],
            "sha256:test123"
        );
    }

    #[test]
    fn format_verdict_sarif_output_is_valid_json_structure() {
        let violations = vec![test_verdict_violation(
            "boundary.never_imports",
            Severity::Error,
            "src/app/main.ts",
            VerdictDisposition::New,
            "Import not allowed",
        )];
        let verdict = verdict_with_violations(violations);
        let output = format_verdict_sarif(&verdict);

        let parsed: serde_json::Value = serde_json::from_str(&output).expect("Valid JSON");
        assert_eq!(
            parsed["$schema"],
            "https://raw.githubusercontent.com/oasis-tcs/sarif-spec/main/sarif-2.1/schema/sarif-schema-2.1.0.json"
        );
        assert_eq!(parsed["version"], "2.1.0");
        assert_eq!(parsed["runs"][0]["tool"]["driver"]["name"], "specgate");
        assert_eq!(parsed["runs"][0]["tool"]["driver"]["version"], "0.1.0");
    }

    #[test]
    fn human_output_includes_edge_classification() {
        // When a verdict has edge_classification set, format_verdict_human should
        // include a section showing the edge counts.
        use crate::verdict::EdgeClassification;

        let mut verdict = empty_verdict();
        verdict.edge_classification = Some(EdgeClassification {
            resolved: 342,
            type_only: 12,
            external: 28,
            unresolved_literal: 3,
            unresolved_dynamic: 7,
        });

        let output = format_verdict_human(&verdict);

        assert!(
            output.contains("Edge classification"),
            "should show edge classification section: {output}"
        );
        assert!(
            output.contains("342"),
            "should show resolved count: {output}"
        );
        assert!(
            output.contains("12"),
            "should show type_only count: {output}"
        );
        assert!(
            output.contains("28"),
            "should show external count: {output}"
        );
        assert!(
            output.contains("3"),
            "should show unresolved_literal count: {output}"
        );
        assert!(
            output.contains("7"),
            "should show unresolved_dynamic count: {output}"
        );
    }

    #[test]
    fn human_output_includes_unresolved_edges() {
        // When a verdict has unresolved_edges, format_verdict_human should show them.
        use crate::verdict::UnresolvedEdge;

        let mut verdict = empty_verdict();
        verdict.unresolved_edges = vec![
            UnresolvedEdge {
                from: "src/api/handler.ts".to_string(),
                specifier: "./missing-module".to_string(),
                kind: "unresolved_literal".to_string(),
                line: Some(42),
            },
            UnresolvedEdge {
                from: "src/utils/loader.ts".to_string(),
                specifier: "./dynamic-missing".to_string(),
                kind: "unresolved_dynamic".to_string(),
                line: None,
            },
        ];

        let output = format_verdict_human(&verdict);

        assert!(
            output.contains("Unresolved imports"),
            "should show unresolved imports section: {output}"
        );
        assert!(
            output.contains("src/api/handler.ts"),
            "should show from file: {output}"
        );
        assert!(
            output.contains("./missing-module"),
            "should show specifier: {output}"
        );
        assert!(
            output.contains("unresolved_literal"),
            "should show kind: {output}"
        );
        assert!(
            output.contains("src/utils/loader.ts"),
            "should show second from file: {output}"
        );
    }

    #[test]
    fn human_output_omits_edge_classification_when_none() {
        // When edge_classification is None, the section should not appear.
        let verdict = empty_verdict();
        let output = format_verdict_human(&verdict);
        assert!(
            !output.contains("Edge classification"),
            "should not show edge classification when None: {output}"
        );
    }

    #[test]
    fn human_output_omits_unresolved_edges_when_empty() {
        // When unresolved_edges is empty, the section should not appear.
        let verdict = empty_verdict();
        let output = format_verdict_human(&verdict);
        assert!(
            !output.contains("Unresolved imports"),
            "should not show unresolved imports when empty: {output}"
        );
    }
}
