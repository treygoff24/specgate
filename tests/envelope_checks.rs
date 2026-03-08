use std::path::PathBuf;

use serde_json::Value;

use specgate::cli::{EXIT_CODE_RUNTIME_ERROR, run};

fn fixture_root(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("envelope")
        .join(name)
}

fn parse_json(source: &str) -> Value {
    serde_json::from_str(source).expect("valid json")
}

fn envelope_missing_violations(verdict: &Value) -> Vec<&Value> {
    verdict["violations"]
        .as_array()
        .expect("violations array")
        .iter()
        .filter(|v| v["rule"].as_str() == Some("boundary.envelope_missing"))
        .collect()
}

fn run_check_fixture(name: &str) -> Value {
    let root = fixture_root(name);
    let result = run([
        "specgate",
        "check",
        "--project-root",
        root.to_str().expect("utf8"),
        "--format",
        "json",
        "--no-baseline",
    ]);

    assert_ne!(
        result.exit_code, EXIT_CODE_RUNTIME_ERROR,
        "fixture {name} should not have runtime errors: stderr={} stdout={}",
        result.stderr, result.stdout
    );

    parse_json(&result.stdout)
}

fn assert_envelope_missing_count(name: &str, expected_count: usize) {
    let verdict = run_check_fixture(name);
    let violations = envelope_missing_violations(&verdict);

    assert_eq!(
        violations.len(),
        expected_count,
        "unexpected boundary.envelope_missing count for fixture {name}: {violations:?}"
    );

    for violation in &violations {
        assert_eq!(
            violation["severity"].as_str(),
            Some("warning"),
            "boundary.envelope_missing severity should be warning for fixture {name}"
        );
    }
}

#[test]
fn valid_basic_has_no_envelope_missing_violation() {
    assert_envelope_missing_count("valid-basic", 0);
}

#[test]
fn missing_import_reports_envelope_missing() {
    assert_envelope_missing_count("missing-import", 1);
}

#[test]
fn missing_call_reports_envelope_missing() {
    assert_envelope_missing_count("missing-call", 1);
}

#[test]
fn wrong_id_reports_envelope_missing() {
    assert_envelope_missing_count("wrong-id", 1);
}

#[test]
fn optional_skip_has_no_envelope_missing_violation() {
    assert_envelope_missing_count("optional-skip", 0);
}

#[test]
fn disabled_config_has_no_envelope_missing_violation() {
    assert_envelope_missing_count("disabled-config", 0);
}

#[test]
fn match_pattern_scoped_checks_only_selected_function() {
    assert_envelope_missing_count("match-pattern-scoped", 0);
}

#[test]
fn match_pattern_wrong_fn_reports_envelope_missing() {
    assert_envelope_missing_count("match-pattern-wrong-fn", 1);
}

#[test]
fn type_only_import_reports_envelope_missing() {
    assert_envelope_missing_count("type-only-import", 1);
}

#[test]
fn envelope_missing_is_reported_as_warning_in_summary() {
    let verdict = run_check_fixture("missing-import");
    assert_eq!(verdict["summary"]["warning_violations"].as_u64(), Some(1));
    assert_eq!(verdict["summary"]["error_violations"].as_u64(), Some(0));
}
