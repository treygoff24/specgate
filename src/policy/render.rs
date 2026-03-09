use serde::Serialize;

use super::types::{
    ChangeClassification, ChangeScope, FieldChange, ModulePolicyDiff, PolicyDiffReport,
};

#[derive(Debug, Clone, Serialize)]
struct NdjsonChangeRecord<'a> {
    #[serde(rename = "type")]
    r#type: &'static str,
    module: &'a str,
    spec_path: &'a str,
    scope: ChangeScope,
    field: &'a str,
    classification: ChangeClassification,
    before: Option<&'a serde_json::Value>,
    after: Option<&'a serde_json::Value>,
    detail: &'a str,
}

#[derive(Debug, Clone, Serialize)]
struct NdjsonErrorRecord<'a> {
    #[serde(rename = "type")]
    r#type: &'static str,
    code: &'a str,
    message: &'a str,
    spec_path: Option<&'a str>,
}

#[derive(Debug, Clone, Serialize)]
struct NdjsonSummaryRecord {
    #[serde(rename = "type")]
    r#type: &'static str,
    modules_changed: usize,
    widening_changes: usize,
    narrowing_changes: usize,
    structural_changes: usize,
    has_widening: bool,
}

/// Render a full policy diff report as human-readable grouped output.
pub fn render_policy_diff_human(report: &PolicyDiffReport) -> String {
    let report = sorted_copy(report);

    let mut lines = Vec::new();
    lines.push(format!(
        "Policy diff: base={} head={}",
        report.base_ref, report.head_ref
    ));
    lines.push(String::new());

    let widening = collect_changes_by_classification(&report.diffs, ChangeClassification::Widening);
    let narrowing =
        collect_changes_by_classification(&report.diffs, ChangeClassification::Narrowing);
    let structural =
        collect_changes_by_classification(&report.diffs, ChangeClassification::Structural);

    render_section("WIDENING", widening, &mut lines);
    render_section("NARROWING", narrowing, &mut lines);
    render_section("STRUCTURAL", structural, &mut lines);

    lines.push(format!(
        "Summary: modules_changed={} widening={} narrowing={} structural={}",
        report.summary.modules_changed,
        report.summary.widening_changes,
        report.summary.narrowing_changes,
        report.summary.structural_changes
    ));

    if !report.summary.limitations.is_empty() {
        lines.push(String::new());
        lines.push("Limitations:".to_string());
        for limitation in &report.summary.limitations {
            lines.push(format!("  - {limitation}"));
        }
    }

    if !report.errors.is_empty() {
        lines.push(String::new());
        lines.push(format!("Errors: {}", report.errors.len()));
        for error in &report.errors {
            lines.push(format!(
                "  - {} {} {}",
                error.code,
                error.spec_path.as_deref().unwrap_or("-"),
                error.message
            ));
        }
    }

    lines.join("\n")
}

/// Render a policy diff report as stable JSON.
pub fn render_policy_diff_json(report: &PolicyDiffReport) -> String {
    let mut report = report.clone();
    report.sort_deterministic();

    serde_json::to_string(&report).expect("policy diff report must serialize to JSON")
}

/// Render a policy diff report as NDJSON lines.
pub fn render_policy_diff_ndjson(report: &PolicyDiffReport) -> Vec<String> {
    let report = sorted_copy(report);
    let mut lines: Vec<String> = Vec::new();

    for error in &report.errors {
        lines.push(
            serde_json::to_string(&NdjsonErrorRecord {
                r#type: "error",
                code: &error.code,
                message: &error.message,
                spec_path: error.spec_path.as_deref(),
            })
            .expect("policy diff error event must serialize"),
        );
    }

    for diff in &report.diffs {
        for change in &diff.changes {
            lines.push(
                serde_json::to_string(&NdjsonChangeRecord {
                    r#type: "change",
                    module: &change.module,
                    spec_path: &change.spec_path,
                    scope: change.scope,
                    field: &change.field,
                    classification: change.classification,
                    before: change.before.as_ref(),
                    after: change.after.as_ref(),
                    detail: &change.detail,
                })
                .expect("policy diff change event must serialize"),
            );
        }
    }

    lines.push(
        serde_json::to_string(&NdjsonSummaryRecord {
            r#type: "summary",
            modules_changed: report.summary.modules_changed,
            widening_changes: report.summary.widening_changes,
            narrowing_changes: report.summary.narrowing_changes,
            structural_changes: report.summary.structural_changes,
            has_widening: report.summary.has_widening,
        })
        .expect("policy diff summary event must serialize"),
    );

    lines
}

fn sorted_copy(report: &PolicyDiffReport) -> PolicyDiffReport {
    let mut report = report.clone();
    report.sort_deterministic();
    report
}

fn render_section(title: &str, changes: Vec<&FieldChange>, lines: &mut Vec<String>) {
    lines.push(format!("{title} ({})", changes.len()));

    for change in changes {
        lines.push(format!(
            "  - module={} field={} detail={}",
            change.module, change.field, change.detail
        ));
    }

    lines.push(String::new());
}

fn collect_changes_by_classification(
    diffs: &[ModulePolicyDiff],
    classification: ChangeClassification,
) -> Vec<&FieldChange> {
    let mut changes = Vec::new();

    for diff in diffs {
        changes.extend(
            diff.changes
                .iter()
                .filter(|change| change.classification == classification),
        );
    }

    changes
}
