use specgate::policy::config_diff::classify_config_changes;
use specgate::policy::types::{ChangeClassification, ConfigFieldChange};
use specgate::spec::config::{
    JestMockMode, ReleaseChannel, SpecConfig, StaleBaselinePolicy, StrictOwnershipLevel,
    UnresolvedEdgePolicy,
};

fn changes_for<'a>(
    changes: &'a [ConfigFieldChange],
    field_path: &str,
) -> Vec<&'a ConfigFieldChange> {
    changes
        .iter()
        .filter(|change| change.field_path == field_path)
        .collect()
}

fn assert_single_change(
    changes: &[ConfigFieldChange],
    field_path: &str,
    classification: ChangeClassification,
) {
    let field_changes = changes_for(changes, field_path);
    assert_eq!(
        field_changes.len(),
        1,
        "unexpected change count for {field_path}"
    );
    assert_eq!(field_changes[0].classification, classification);
}

#[test]
fn jest_mock_mode_enforce_to_warn_is_widening() {
    let base = SpecConfig {
        jest_mock_mode: JestMockMode::Enforce,
        ..SpecConfig::default()
    };

    let head = SpecConfig {
        jest_mock_mode: JestMockMode::Warn,
        ..SpecConfig::default()
    };

    let changes = classify_config_changes(&base, &head);
    assert_single_change(&changes, "jest_mock_mode", ChangeClassification::Widening);
}

#[test]
fn jest_mock_mode_warn_to_enforce_is_narrowing() {
    let base = SpecConfig {
        jest_mock_mode: JestMockMode::Warn,
        ..SpecConfig::default()
    };

    let head = SpecConfig {
        jest_mock_mode: JestMockMode::Enforce,
        ..SpecConfig::default()
    };

    let changes = classify_config_changes(&base, &head);
    assert_single_change(&changes, "jest_mock_mode", ChangeClassification::Narrowing);
}

#[test]
fn added_exclude_pattern_is_widening() {
    let base = SpecConfig::default();
    let mut head = SpecConfig::default();
    head.exclude.push("**/new-exclude/**".into());

    let changes = classify_config_changes(&base, &head);
    assert_single_change(&changes, "exclude", ChangeClassification::Widening);
}

#[test]
fn removed_exclude_pattern_is_narrowing() {
    let base = SpecConfig::default();
    let mut head = SpecConfig::default();
    head.exclude.retain(|entry| entry != "**/node_modules/**");

    let changes = classify_config_changes(&base, &head);
    assert_single_change(&changes, "exclude", ChangeClassification::Narrowing);
}

#[test]
fn removed_spec_dir_is_widening() {
    let base = SpecConfig::default();
    let mut head = SpecConfig::default();
    head.spec_dirs.clear();

    let changes = classify_config_changes(&base, &head);
    assert_single_change(&changes, "spec_dirs", ChangeClassification::Widening);
}

#[test]
fn added_spec_dir_is_narrowing() {
    let base = SpecConfig::default();
    let mut head = SpecConfig::default();
    head.spec_dirs.push("packages".into());

    let changes = classify_config_changes(&base, &head);
    assert_single_change(&changes, "spec_dirs", ChangeClassification::Narrowing);
}

#[test]
fn escape_hatches_max_new_increased_is_widening() {
    let mut base = SpecConfig::default();
    base.escape_hatches.max_new_per_diff = Some(5);

    let mut head = SpecConfig::default();
    head.escape_hatches.max_new_per_diff = Some(10);

    let changes = classify_config_changes(&base, &head);
    assert_single_change(
        &changes,
        "escape_hatches.max_new_per_diff",
        ChangeClassification::Widening,
    );
}

#[test]
fn escape_hatches_max_new_decreased_is_narrowing() {
    let mut base = SpecConfig::default();
    base.escape_hatches.max_new_per_diff = Some(10);

    let mut head = SpecConfig::default();
    head.escape_hatches.max_new_per_diff = Some(5);

    let changes = classify_config_changes(&base, &head);
    assert_single_change(
        &changes,
        "escape_hatches.max_new_per_diff",
        ChangeClassification::Narrowing,
    );
}

#[test]
fn escape_hatches_max_new_removed_is_widening() {
    let mut base = SpecConfig::default();
    base.escape_hatches.max_new_per_diff = Some(5);

    let head = SpecConfig::default();
    let changes = classify_config_changes(&base, &head);

    assert_single_change(
        &changes,
        "escape_hatches.max_new_per_diff",
        ChangeClassification::Widening,
    );
}

#[test]
fn escape_hatches_max_new_added_is_narrowing() {
    let base = SpecConfig::default();
    let mut head = SpecConfig::default();
    head.escape_hatches.max_new_per_diff = Some(5);

    let changes = classify_config_changes(&base, &head);
    assert_single_change(
        &changes,
        "escape_hatches.max_new_per_diff",
        ChangeClassification::Narrowing,
    );
}

#[test]
fn escape_hatches_require_expiry_true_to_false_is_widening() {
    let mut base = SpecConfig::default();
    base.escape_hatches.require_expiry = true;

    let head = SpecConfig::default();
    let changes = classify_config_changes(&base, &head);

    assert_single_change(
        &changes,
        "escape_hatches.require_expiry",
        ChangeClassification::Widening,
    );
}

#[test]
fn escape_hatches_require_expiry_false_to_true_is_narrowing() {
    let base = SpecConfig::default();
    let mut head = SpecConfig::default();
    head.escape_hatches.require_expiry = true;

    let changes = classify_config_changes(&base, &head);
    assert_single_change(
        &changes,
        "escape_hatches.require_expiry",
        ChangeClassification::Narrowing,
    );
}

#[test]
fn stale_baseline_fail_to_warn_is_widening() {
    let base = SpecConfig {
        stale_baseline: StaleBaselinePolicy::Fail,
        ..SpecConfig::default()
    };

    let head = SpecConfig {
        stale_baseline: StaleBaselinePolicy::Warn,
        ..SpecConfig::default()
    };

    let changes = classify_config_changes(&base, &head);
    assert_single_change(&changes, "stale_baseline", ChangeClassification::Widening);
}

#[test]
fn stale_baseline_warn_to_fail_is_narrowing() {
    let base = SpecConfig {
        stale_baseline: StaleBaselinePolicy::Warn,
        ..SpecConfig::default()
    };

    let head = SpecConfig {
        stale_baseline: StaleBaselinePolicy::Fail,
        ..SpecConfig::default()
    };

    let changes = classify_config_changes(&base, &head);
    assert_single_change(&changes, "stale_baseline", ChangeClassification::Narrowing);
}

#[test]
fn enforce_type_only_imports_true_to_false_is_widening() {
    let base = SpecConfig {
        enforce_type_only_imports: true,
        ..SpecConfig::default()
    };

    let head = SpecConfig::default();
    let changes = classify_config_changes(&base, &head);

    assert_single_change(
        &changes,
        "enforce_type_only_imports",
        ChangeClassification::Widening,
    );
}

#[test]
fn enforce_type_only_imports_false_to_true_is_narrowing() {
    let base = SpecConfig::default();
    let head = SpecConfig {
        enforce_type_only_imports: true,
        ..SpecConfig::default()
    };

    let changes = classify_config_changes(&base, &head);
    assert_single_change(
        &changes,
        "enforce_type_only_imports",
        ChangeClassification::Narrowing,
    );
}

#[test]
fn unresolved_edge_policy_error_to_ignore_is_widening() {
    let base = SpecConfig {
        unresolved_edge_policy: UnresolvedEdgePolicy::Error,
        ..SpecConfig::default()
    };

    let head = SpecConfig {
        unresolved_edge_policy: UnresolvedEdgePolicy::Ignore,
        ..SpecConfig::default()
    };

    let changes = classify_config_changes(&base, &head);
    assert_single_change(
        &changes,
        "unresolved_edge_policy",
        ChangeClassification::Widening,
    );
}

#[test]
fn unresolved_edge_policy_ignore_to_error_is_narrowing() {
    let base = SpecConfig {
        unresolved_edge_policy: UnresolvedEdgePolicy::Ignore,
        ..SpecConfig::default()
    };

    let head = SpecConfig {
        unresolved_edge_policy: UnresolvedEdgePolicy::Error,
        ..SpecConfig::default()
    };

    let changes = classify_config_changes(&base, &head);
    assert_single_change(
        &changes,
        "unresolved_edge_policy",
        ChangeClassification::Narrowing,
    );
}

#[test]
fn strict_ownership_true_to_false_is_widening() {
    let base = SpecConfig {
        strict_ownership: true,
        ..SpecConfig::default()
    };

    let head = SpecConfig::default();
    let changes = classify_config_changes(&base, &head);

    assert_single_change(&changes, "strict_ownership", ChangeClassification::Widening);
}

#[test]
fn strict_ownership_false_to_true_is_narrowing() {
    let base = SpecConfig::default();
    let head = SpecConfig {
        strict_ownership: true,
        ..SpecConfig::default()
    };

    let changes = classify_config_changes(&base, &head);
    assert_single_change(
        &changes,
        "strict_ownership",
        ChangeClassification::Narrowing,
    );
}

#[test]
fn import_hygiene_deny_deep_imports_removed_is_widening() {
    let mut base = SpecConfig::default();
    base.import_hygiene.deny_deep_imports = vec!["lodash".into()];

    let head = SpecConfig::default();
    let changes = classify_config_changes(&base, &head);

    assert_single_change(
        &changes,
        "import_hygiene.deny_deep_imports",
        ChangeClassification::Widening,
    );
}

#[test]
fn import_hygiene_deny_deep_imports_added_is_narrowing() {
    let base = SpecConfig::default();
    let mut head = SpecConfig::default();
    head.import_hygiene.deny_deep_imports = vec!["lodash".into()];

    let changes = classify_config_changes(&base, &head);
    assert_single_change(
        &changes,
        "import_hygiene.deny_deep_imports",
        ChangeClassification::Narrowing,
    );
}

#[test]
fn envelope_enabled_true_to_false_is_widening() {
    let base = SpecConfig::default();
    let mut head = SpecConfig::default();
    head.envelope.enabled = false;

    let changes = classify_config_changes(&base, &head);
    assert_single_change(&changes, "envelope.enabled", ChangeClassification::Widening);
}

#[test]
fn envelope_enabled_false_to_true_is_narrowing() {
    let mut base = SpecConfig::default();
    base.envelope.enabled = false;

    let head = SpecConfig::default();
    let changes = classify_config_changes(&base, &head);

    assert_single_change(
        &changes,
        "envelope.enabled",
        ChangeClassification::Narrowing,
    );
}

#[test]
fn strict_ownership_level_warnings_to_errors_is_widening() {
    let base = SpecConfig {
        strict_ownership_level: StrictOwnershipLevel::Warnings,
        ..SpecConfig::default()
    };

    let head = SpecConfig::default();
    let changes = classify_config_changes(&base, &head);

    assert_single_change(
        &changes,
        "strict_ownership_level",
        ChangeClassification::Widening,
    );
}

#[test]
fn strict_ownership_level_errors_to_warnings_is_narrowing() {
    let base = SpecConfig::default();
    let head = SpecConfig {
        strict_ownership_level: StrictOwnershipLevel::Warnings,
        ..SpecConfig::default()
    };

    let changes = classify_config_changes(&base, &head);
    assert_single_change(
        &changes,
        "strict_ownership_level",
        ChangeClassification::Narrowing,
    );
}

#[test]
fn ownership_governance_fields_share_directional_parity() {
    let strict = SpecConfig {
        strict_ownership: true,
        strict_ownership_level: StrictOwnershipLevel::Warnings,
        ..SpecConfig::default()
    };

    let relaxed = SpecConfig::default();
    let changes = classify_config_changes(&strict, &relaxed);

    assert_single_change(&changes, "strict_ownership", ChangeClassification::Widening);
    assert_single_change(
        &changes,
        "strict_ownership_level",
        ChangeClassification::Widening,
    );
}

#[test]
fn telemetry_change_is_structural() {
    let base = SpecConfig::default();
    let head = SpecConfig {
        telemetry: true,
        ..SpecConfig::default()
    };

    let changes = classify_config_changes(&base, &head);
    assert_single_change(&changes, "telemetry", ChangeClassification::Structural);
}

#[test]
fn release_channel_change_is_structural() {
    let base = SpecConfig::default();
    let head = SpecConfig {
        release_channel: ReleaseChannel::Beta,
        ..SpecConfig::default()
    };

    let changes = classify_config_changes(&base, &head);
    assert_single_change(
        &changes,
        "release_channel",
        ChangeClassification::Structural,
    );
}

#[test]
fn tsconfig_filename_change_is_structural() {
    let base = SpecConfig::default();
    let head = SpecConfig {
        tsconfig_filename: "tsconfig.base.json".into(),
        ..SpecConfig::default()
    };

    let changes = classify_config_changes(&base, &head);
    assert_single_change(
        &changes,
        "tsconfig_filename",
        ChangeClassification::Structural,
    );
}

#[test]
fn test_patterns_change_is_structural() {
    let base = SpecConfig::default();
    let mut head = SpecConfig::default();
    head.test_patterns.push("**/*.cy.ts".into());

    let changes = classify_config_changes(&base, &head);
    assert_single_change(&changes, "test_patterns", ChangeClassification::Structural);
}

#[test]
fn include_dirs_change_is_structural() {
    let base = SpecConfig::default();
    let mut head = SpecConfig::default();
    head.include_dirs.push("vendor".into());

    let changes = classify_config_changes(&base, &head);
    assert_single_change(&changes, "include_dirs", ChangeClassification::Structural);
}

#[test]
fn no_changes_produces_empty() {
    let config = SpecConfig::default();
    let changes = classify_config_changes(&config, &config);
    assert!(changes.is_empty());
}
