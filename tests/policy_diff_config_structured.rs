use specgate::policy::config_diff::classify_config_changes;
use specgate::policy::types::ChangeClassification;
use specgate::spec::config::SpecConfig;

fn parse_config(yaml: &str) -> SpecConfig {
    yaml_serde::from_str(yaml).expect("parse config")
}

#[test]
fn legacy_and_structured_deep_import_entries_normalize_equally() {
    let base = parse_config(
        r#"
import_hygiene:
  deny_deep_imports:
    - lodash
"#,
    );
    let head = parse_config(
        r#"
import_hygiene:
  deny_deep_imports:
    - pattern: lodash
      max_depth: 0
"#,
    );

    let changes = classify_config_changes(&base, &head);
    assert!(
        changes
            .iter()
            .all(|change| change.field_path != "import_hygiene.deny_deep_imports"),
        "unexpected structured diff: {changes:?}"
    );
}

#[test]
fn omitted_warning_severity_matches_explicit_warning() {
    let base = parse_config(
        r#"
import_hygiene:
  deny_deep_imports:
    - pattern: lodash/**
      max_depth: 1
"#,
    );
    let head = parse_config(
        r#"
import_hygiene:
  deny_deep_imports:
    - pattern: lodash/**
      max_depth: 1
      severity: warning
"#,
    );

    let changes = classify_config_changes(&base, &head);
    assert!(
        changes
            .iter()
            .all(|change| change.field_path != "import_hygiene.deny_deep_imports"),
        "unexpected warning-default diff: {changes:?}"
    );
}

#[test]
fn deep_import_severity_warning_to_error_is_narrowing() {
    let base = parse_config(
        r#"
import_hygiene:
  deny_deep_imports:
    - pattern: lodash/**
      max_depth: 1
      severity: warning
"#,
    );
    let head = parse_config(
        r#"
import_hygiene:
  deny_deep_imports:
    - pattern: lodash/**
      max_depth: 1
      severity: error
"#,
    );

    let changes = classify_config_changes(&base, &head);
    let change = changes
        .iter()
        .find(|change| change.field_path == "import_hygiene.deny_deep_imports")
        .expect("deep import change");
    assert_eq!(change.classification, ChangeClassification::Narrowing);
}

#[test]
fn deep_import_max_depth_relaxation_is_widening() {
    let base = parse_config(
        r#"
import_hygiene:
  deny_deep_imports:
    - pattern: lodash/**
      max_depth: 0
"#,
    );
    let head = parse_config(
        r#"
import_hygiene:
  deny_deep_imports:
    - pattern: lodash/**
      max_depth: 2
"#,
    );

    let changes = classify_config_changes(&base, &head);
    let change = changes
        .iter()
        .find(|change| change.field_path == "import_hygiene.deny_deep_imports")
        .expect("deep import change");
    assert_eq!(change.classification, ChangeClassification::Widening);
}
