use std::collections::BTreeSet;

use super::types::{
    ChangeClassification, ConfigFieldChange, sort_config_field_changes_deterministic,
};
use crate::spec::Severity;
use crate::spec::config::{
    DenyDeepImportEntry, JestMockMode, ReleaseChannel, SpecConfig, StaleBaselinePolicy,
    StrictOwnershipLevel, UnresolvedEdgePolicy,
};

const ABSENT_VALUE: &str = "<absent>";
const NONE_VALUE: &str = "none";

pub fn classify_config_changes(base: &SpecConfig, head: &SpecConfig) -> Vec<ConfigFieldChange> {
    let mut changes = Vec::new();

    diff_string_set(
        &mut changes,
        "exclude",
        &base.exclude,
        &head.exclude,
        ChangeClassification::Widening,
        ChangeClassification::Narrowing,
    );
    diff_string_set(
        &mut changes,
        "spec_dirs",
        &base.spec_dirs,
        &head.spec_dirs,
        ChangeClassification::Narrowing,
        ChangeClassification::Widening,
    );
    diff_optional_limit(
        &mut changes,
        "escape_hatches.max_new_per_diff",
        base.escape_hatches.max_new_per_diff,
        head.escape_hatches.max_new_per_diff,
    );
    diff_ranked_change(
        &mut changes,
        "escape_hatches.require_expiry",
        u8::from(base.escape_hatches.require_expiry),
        u8::from(head.escape_hatches.require_expiry),
        base.escape_hatches.require_expiry.to_string(),
        head.escape_hatches.require_expiry.to_string(),
    );
    diff_ranked_change(
        &mut changes,
        "jest_mock_mode",
        jest_mock_mode_rank(base.jest_mock_mode),
        jest_mock_mode_rank(head.jest_mock_mode),
        render_jest_mock_mode(base.jest_mock_mode),
        render_jest_mock_mode(head.jest_mock_mode),
    );
    diff_ranked_change(
        &mut changes,
        "stale_baseline",
        stale_baseline_rank(base.stale_baseline),
        stale_baseline_rank(head.stale_baseline),
        render_stale_baseline(base.stale_baseline),
        render_stale_baseline(head.stale_baseline),
    );
    diff_ranked_change(
        &mut changes,
        "enforce_type_only_imports",
        u8::from(base.enforce_type_only_imports),
        u8::from(head.enforce_type_only_imports),
        base.enforce_type_only_imports.to_string(),
        head.enforce_type_only_imports.to_string(),
    );
    diff_ranked_change(
        &mut changes,
        "unresolved_edge_policy",
        unresolved_edge_policy_rank(base.unresolved_edge_policy),
        unresolved_edge_policy_rank(head.unresolved_edge_policy),
        render_unresolved_edge_policy(base.unresolved_edge_policy),
        render_unresolved_edge_policy(head.unresolved_edge_policy),
    );
    diff_ranked_change(
        &mut changes,
        "strict_ownership",
        u8::from(base.strict_ownership),
        u8::from(head.strict_ownership),
        base.strict_ownership.to_string(),
        head.strict_ownership.to_string(),
    );
    diff_deny_deep_import_entries(
        &mut changes,
        "import_hygiene.deny_deep_imports",
        &base.import_hygiene.deny_deep_imports,
        &head.import_hygiene.deny_deep_imports,
    );
    diff_ranked_change(
        &mut changes,
        "envelope.enabled",
        u8::from(base.envelope.enabled),
        u8::from(head.envelope.enabled),
        base.envelope.enabled.to_string(),
        head.envelope.enabled.to_string(),
    );
    diff_ranked_change(
        &mut changes,
        "strict_ownership_level",
        strict_ownership_level_rank(base.strict_ownership_level),
        strict_ownership_level_rank(head.strict_ownership_level),
        render_strict_ownership_level(base.strict_ownership_level),
        render_strict_ownership_level(head.strict_ownership_level),
    );

    diff_structural_change(
        &mut changes,
        "telemetry",
        base.telemetry,
        head.telemetry,
        base.telemetry.to_string(),
        head.telemetry.to_string(),
    );
    diff_structural_change(
        &mut changes,
        "release_channel",
        base.release_channel,
        head.release_channel,
        render_release_channel(base.release_channel),
        render_release_channel(head.release_channel),
    );
    diff_structural_change(
        &mut changes,
        "tsconfig_filename",
        &base.tsconfig_filename,
        &head.tsconfig_filename,
        base.tsconfig_filename.clone(),
        head.tsconfig_filename.clone(),
    );
    diff_structural_change(
        &mut changes,
        "test_patterns",
        &base.test_patterns,
        &head.test_patterns,
        render_string_list(&base.test_patterns),
        render_string_list(&head.test_patterns),
    );
    diff_structural_change(
        &mut changes,
        "include_dirs",
        &base.include_dirs,
        &head.include_dirs,
        render_string_list(&base.include_dirs),
        render_string_list(&head.include_dirs),
    );

    sort_config_field_changes_deterministic(&mut changes);
    changes
}

fn diff_string_set(
    changes: &mut Vec<ConfigFieldChange>,
    field_path: &str,
    base: &[String],
    head: &[String],
    added_classification: ChangeClassification,
    removed_classification: ChangeClassification,
) {
    let base_values: BTreeSet<&str> = base.iter().map(String::as_str).collect();
    let head_values: BTreeSet<&str> = head.iter().map(String::as_str).collect();

    for value in head_values.difference(&base_values) {
        push_change(
            changes,
            field_path,
            added_classification,
            ABSENT_VALUE.to_string(),
            (*value).to_string(),
        );
    }

    for value in base_values.difference(&head_values) {
        push_change(
            changes,
            field_path,
            removed_classification,
            (*value).to_string(),
            ABSENT_VALUE.to_string(),
        );
    }
}

fn diff_deny_deep_import_entries(
    changes: &mut Vec<ConfigFieldChange>,
    field_path: &str,
    base: &[DenyDeepImportEntry],
    head: &[DenyDeepImportEntry],
) {
    let base_map = normalized_deep_import_entry_map(base);
    let head_map = normalized_deep_import_entry_map(head);
    let patterns = base_map
        .keys()
        .chain(head_map.keys())
        .cloned()
        .collect::<BTreeSet<_>>();

    for pattern in patterns {
        match (base_map.get(&pattern), head_map.get(&pattern)) {
            (Some(before), None) => push_change(
                changes,
                field_path,
                ChangeClassification::Widening,
                render_deep_import_entry(before),
                ABSENT_VALUE.to_string(),
            ),
            (None, Some(after)) => push_change(
                changes,
                field_path,
                ChangeClassification::Narrowing,
                ABSENT_VALUE.to_string(),
                render_deep_import_entry(after),
            ),
            (Some(before), Some(after)) if before != after => {
                push_change(
                    changes,
                    field_path,
                    classify_deep_import_entry_change(before, after),
                    render_deep_import_entry(before),
                    render_deep_import_entry(after),
                );
            }
            _ => {}
        }
    }
}

fn diff_optional_limit(
    changes: &mut Vec<ConfigFieldChange>,
    field_path: &str,
    base: Option<usize>,
    head: Option<usize>,
) {
    if base == head {
        return;
    }

    let classification = match (base, head) {
        (Some(base), Some(head)) if head > base => ChangeClassification::Widening,
        (Some(base), Some(head)) if head < base => ChangeClassification::Narrowing,
        (Some(_), None) => ChangeClassification::Widening,
        (None, Some(_)) => ChangeClassification::Narrowing,
        _ => return,
    };

    push_change(
        changes,
        field_path,
        classification,
        render_optional_limit(base),
        render_optional_limit(head),
    );
}

fn diff_ranked_change(
    changes: &mut Vec<ConfigFieldChange>,
    field_path: &str,
    base_rank: u8,
    head_rank: u8,
    before: String,
    after: String,
) {
    if base_rank == head_rank {
        return;
    }

    let classification = if head_rank < base_rank {
        ChangeClassification::Widening
    } else {
        ChangeClassification::Narrowing
    };

    push_change(changes, field_path, classification, before, after);
}

fn diff_structural_change<T: PartialEq>(
    changes: &mut Vec<ConfigFieldChange>,
    field_path: &str,
    base: T,
    head: T,
    before: String,
    after: String,
) {
    if base == head {
        return;
    }

    push_change(
        changes,
        field_path,
        ChangeClassification::Structural,
        before,
        after,
    );
}

fn push_change(
    changes: &mut Vec<ConfigFieldChange>,
    field_path: &str,
    classification: ChangeClassification,
    before: String,
    after: String,
) {
    changes.push(ConfigFieldChange {
        field_path: field_path.to_string(),
        classification,
        before,
        after,
    });
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NormalizedDeepImportEntry {
    pattern: String,
    max_depth: usize,
    severity: Severity,
}

fn normalized_deep_import_entry_map(
    entries: &[DenyDeepImportEntry],
) -> std::collections::BTreeMap<String, NormalizedDeepImportEntry> {
    entries
        .iter()
        .map(|entry| {
            let normalized = NormalizedDeepImportEntry {
                pattern: entry.pattern.trim().to_string(),
                max_depth: entry.max_depth,
                severity: entry.effective_severity(),
            };
            (normalized.pattern.clone(), normalized)
        })
        .collect()
}

fn classify_deep_import_entry_change(
    before: &NormalizedDeepImportEntry,
    after: &NormalizedDeepImportEntry,
) -> ChangeClassification {
    let depth_direction = match after.max_depth.cmp(&before.max_depth) {
        std::cmp::Ordering::Less => Some(ChangeClassification::Narrowing),
        std::cmp::Ordering::Greater => Some(ChangeClassification::Widening),
        std::cmp::Ordering::Equal => None,
    };
    let severity_direction =
        match severity_rank(after.severity).cmp(&severity_rank(before.severity)) {
            std::cmp::Ordering::Less => Some(ChangeClassification::Narrowing),
            std::cmp::Ordering::Greater => Some(ChangeClassification::Widening),
            std::cmp::Ordering::Equal => None,
        };

    match (depth_direction, severity_direction) {
        (Some(direction), None) | (None, Some(direction)) => direction,
        (Some(left), Some(right)) if left == right => left,
        _ => ChangeClassification::Structural,
    }
}

fn render_deep_import_entry(entry: &NormalizedDeepImportEntry) -> String {
    serde_json::json!({
        "pattern": entry.pattern,
        "max_depth": entry.max_depth,
        "severity": render_severity(entry.severity),
    })
    .to_string()
}

fn render_optional_limit(value: Option<usize>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| NONE_VALUE.to_string())
}

fn render_string_list(values: &[String]) -> String {
    serde_json::to_string(values).expect("config string list must serialize")
}

const fn jest_mock_mode_rank(mode: JestMockMode) -> u8 {
    match mode {
        JestMockMode::Warn => 0,
        JestMockMode::Enforce => 1,
    }
}

fn render_jest_mock_mode(mode: JestMockMode) -> String {
    match mode {
        JestMockMode::Warn => "warn".to_string(),
        JestMockMode::Enforce => "enforce".to_string(),
    }
}

const fn stale_baseline_rank(policy: StaleBaselinePolicy) -> u8 {
    match policy {
        StaleBaselinePolicy::Warn => 0,
        StaleBaselinePolicy::Fail => 1,
    }
}

fn render_stale_baseline(policy: StaleBaselinePolicy) -> String {
    match policy {
        StaleBaselinePolicy::Warn => "warn".to_string(),
        StaleBaselinePolicy::Fail => "fail".to_string(),
    }
}

const fn unresolved_edge_policy_rank(policy: UnresolvedEdgePolicy) -> u8 {
    match policy {
        UnresolvedEdgePolicy::Ignore => 0,
        UnresolvedEdgePolicy::Warn => 1,
        UnresolvedEdgePolicy::Error => 2,
    }
}

fn render_unresolved_edge_policy(policy: UnresolvedEdgePolicy) -> String {
    match policy {
        UnresolvedEdgePolicy::Warn => "warn".to_string(),
        UnresolvedEdgePolicy::Error => "error".to_string(),
        UnresolvedEdgePolicy::Ignore => "ignore".to_string(),
    }
}

const fn strict_ownership_level_rank(level: StrictOwnershipLevel) -> u8 {
    match level {
        StrictOwnershipLevel::Errors => 0,
        StrictOwnershipLevel::Warnings => 1,
    }
}

fn render_strict_ownership_level(level: StrictOwnershipLevel) -> String {
    match level {
        StrictOwnershipLevel::Errors => "errors".to_string(),
        StrictOwnershipLevel::Warnings => "warnings".to_string(),
    }
}

fn render_release_channel(channel: ReleaseChannel) -> String {
    match channel {
        ReleaseChannel::Stable => "stable".to_string(),
        ReleaseChannel::Beta => "beta".to_string(),
    }
}

const fn severity_rank(severity: Severity) -> u8 {
    match severity {
        Severity::Error => 0,
        Severity::Warning => 1,
    }
}

fn render_severity(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
    }
}
