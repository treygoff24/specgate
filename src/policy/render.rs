use serde::Serialize;

use super::types::{
    ChangeClassification, ChangeScope, CompensationCandidate, CompensationResult, FieldChange,
    ModulePolicyDiff, PolicyDiffReport,
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
    net_classification: ChangeClassification,
}

#[derive(Debug, Clone, Serialize)]
struct NdjsonConfigChangeRecord<'a> {
    #[serde(rename = "type")]
    r#type: &'static str,
    field_path: &'a str,
    classification: ChangeClassification,
    before: &'a str,
    after: &'a str,
}

#[derive(Debug, Clone, Serialize)]
struct NdjsonCompensationRecord<'a> {
    #[serde(rename = "type")]
    r#type: &'static str,
    widening_module: &'a str,
    widening_field: &'a str,
    narrowing_module: &'a str,
    narrowing_field: &'a str,
    relationship_importer: &'a str,
    relationship_provider: &'a str,
    result: CompensationResult,
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

    if !report.config_changes.is_empty() {
        lines.push("Config changes (specgate.config.yml):".to_string());
        for change in &report.config_changes {
            lines.push(format!(
                "  {}: {}: {} -> {}",
                render_classification(change.classification),
                change.field_path,
                change.before,
                change.after
            ));
        }
        lines.push(String::new());
    }

    // Render compensation candidates if any
    if !report.compensations.is_empty() {
        lines.push("Compensations:".to_string());
        for candidate in &report.compensations {
            lines.push(render_compensation_human(candidate));
        }
        lines.push(String::new());
    }

    lines.push(format!(
        "Summary: modules_changed={} widening={} narrowing={} structural={} net={}",
        report.summary.modules_changed,
        report.summary.widening_changes,
        report.summary.narrowing_changes,
        report.summary.structural_changes,
        render_classification(report.net_classification)
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

fn render_compensation_human(candidate: &CompensationCandidate) -> String {
    let result_str = match candidate.result {
        CompensationResult::Offset => "offset",
        CompensationResult::Partial => "partial",
        CompensationResult::Ambiguous => "ambiguous",
    };
    format!(
        "  COMPENSATED ({}): widening in {}/{} offset by narrowing in {}/{} ({} imports from {})",
        result_str,
        candidate.widening.module,
        candidate.widening.field,
        candidate.narrowing.module,
        candidate.narrowing.field,
        candidate.relationship.importer,
        candidate.relationship.provider
    )
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

    for change in &report.config_changes {
        lines.push(
            serde_json::to_string(&NdjsonConfigChangeRecord {
                r#type: "config_change",
                field_path: &change.field_path,
                classification: change.classification,
                before: &change.before,
                after: &change.after,
            })
            .expect("policy diff config change event must serialize"),
        );
    }

    // Add compensation records to NDJSON output
    for candidate in &report.compensations {
        lines.push(
            serde_json::to_string(&NdjsonCompensationRecord {
                r#type: "compensation",
                widening_module: &candidate.widening.module,
                widening_field: &candidate.widening.field,
                narrowing_module: &candidate.narrowing.module,
                narrowing_field: &candidate.narrowing.field,
                relationship_importer: &candidate.relationship.importer,
                relationship_provider: &candidate.relationship.provider,
                result: candidate.result,
            })
            .expect("policy diff compensation event must serialize"),
        );
    }

    lines.push(
        serde_json::to_string(&NdjsonSummaryRecord {
            r#type: "summary",
            modules_changed: report.summary.modules_changed,
            widening_changes: report.summary.widening_changes,
            narrowing_changes: report.summary.narrowing_changes,
            structural_changes: report.summary.structural_changes,
            has_widening: report.summary.has_widening,
            net_classification: report.net_classification,
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

fn render_classification(classification: ChangeClassification) -> &'static str {
    match classification {
        ChangeClassification::Widening => "WIDENING",
        ChangeClassification::Narrowing => "NARROWING",
        ChangeClassification::Structural => "STRUCTURAL",
    }
}
