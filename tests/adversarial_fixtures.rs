use std::fs;
use std::path::PathBuf;

use serde::Deserialize;
use serde_json::Value;

use specgate::cli::{EXIT_CODE_PASS, EXIT_CODE_POLICY_VIOLATIONS, EXIT_CODE_RUNTIME_ERROR, run};

#[derive(Deserialize)]
struct ExpectedBehavior {
    assertion: String,
    #[serde(default)]
    violations: Vec<ExpectedViolation>,
    gap_reason: Option<String>,
    future_priority: Option<String>,
    notes: Option<String>,
}

#[derive(Deserialize)]
struct ExpectedViolation {
    rule: String,
    from_module: Option<String>,
    to_module: Option<String>,
}

fn fixture_root(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("adversarial")
        .join(name)
}

fn parse_json(source: &str) -> Value {
    serde_json::from_str(source).expect("valid json output")
}

fn load_expected_behavior(name: &str) -> ExpectedBehavior {
    let path = fixture_root(name).join("expected.yml");
    let source = fs::read_to_string(&path).expect("read expected.yml");
    yaml_serde::from_str(&source).expect("parse expected.yml")
}

fn run_check_fixture(fixture_name: &str) -> (i32, Value, String, String) {
    let root = fixture_root(fixture_name);
    let result = run([
        "specgate",
        "check",
        "--project-root",
        root.to_str().expect("utf8"),
        "--no-baseline",
        "--format",
        "json",
    ]);

    let verdict = parse_json(&result.stdout);
    (result.exit_code, verdict, result.stdout, result.stderr)
}

fn violation_matches(actual: &Value, expected: &ExpectedViolation) -> bool {
    if actual["rule"].as_str() != Some(expected.rule.as_str()) {
        return false;
    }

    if let Some(from_module) = expected.from_module.as_deref()
        && actual["from_module"].as_str() != Some(from_module)
    {
        return false;
    }

    if let Some(to_module) = expected.to_module.as_deref()
        && actual["to_module"].as_str() != Some(to_module)
    {
        return false;
    }

    true
}

fn assert_fixture_behavior(fixture_name: &str) {
    let expected = load_expected_behavior(fixture_name);

    let _ = (
        expected.gap_reason.as_deref(),
        expected.future_priority.as_deref(),
        expected.notes.as_deref(),
    );

    let (exit_code, verdict, stdout, stderr) = run_check_fixture(fixture_name);
    let violations = verdict["violations"]
        .as_array()
        .expect("violations array in verdict json");

    match expected.assertion.as_str() {
        "catch" => {
            assert_ne!(
                exit_code, EXIT_CODE_RUNTIME_ERROR,
                "fixture {fixture_name} should not crash: stderr={stderr} stdout={stdout}"
            );
            assert!(
                !violations.is_empty(),
                "fixture {fixture_name} should produce at least one violation"
            );

            for expected_violation in &expected.violations {
                assert!(
                    violations
                        .iter()
                        .any(|actual| violation_matches(actual, expected_violation)),
                    "fixture {fixture_name} missing expected violation {:?}; actual={violations:?}",
                    expected_violation.rule
                );
            }
        }
        "clean_pass" => {
            assert_eq!(
                exit_code, EXIT_CODE_PASS,
                "fixture {fixture_name} should pass cleanly: stderr={stderr} stdout={stdout}"
            );

            if expected.violations.is_empty() {
                assert!(
                    violations.is_empty(),
                    "fixture {fixture_name} expected no violations, found {violations:?}"
                );
            } else {
                for expected_violation in &expected.violations {
                    assert!(
                        !violations
                            .iter()
                            .any(|actual| violation_matches(actual, expected_violation)),
                        "fixture {fixture_name} should not include violation {:?}; actual={violations:?}",
                        expected_violation.rule
                    );
                }
            }
        }
        "diagnostic" => {
            assert!(
                exit_code == EXIT_CODE_PASS || exit_code == EXIT_CODE_POLICY_VIOLATIONS,
                "fixture {fixture_name} diagnostic check should not crash (exit 0/1 expected): stderr={stderr} stdout={stdout}"
            );

            if fixture_name == "ownership-overlap" || fixture_name == "orphan-module" {
                let root = fixture_root(fixture_name);
                let doctor_result = run([
                    "specgate",
                    "doctor",
                    "ownership",
                    "--project-root",
                    root.to_str().expect("utf8"),
                    "--format",
                    "json",
                ]);

                assert!(
                    doctor_result.exit_code == EXIT_CODE_PASS
                        || doctor_result.exit_code == EXIT_CODE_POLICY_VIOLATIONS,
                    "{fixture_name} doctor ownership should return pass/policy-violation: stderr={} stdout={}",
                    doctor_result.stderr,
                    doctor_result.stdout
                );

                let doctor_output = parse_json(&doctor_result.stdout);
                assert!(
                    doctor_output.get("status").is_some(),
                    "doctor output should contain status field: {doctor_output:?}"
                );

                let report = &doctor_output["report"];
                assert!(
                    report.is_object(),
                    "doctor ownership json should include report object: {doctor_output:?}"
                );

                if fixture_name == "ownership-overlap" {
                    assert!(
                        report["overlapping_files"]
                            .as_array()
                            .map(|v| !v.is_empty())
                            .unwrap_or(false),
                        "ownership-overlap should report at least one overlap: {doctor_output:?}"
                    );
                }

                if fixture_name == "orphan-module" {
                    assert!(
                        report["orphaned_specs"]
                            .as_array()
                            .map(|v| !v.is_empty())
                            .unwrap_or(false),
                        "orphan-module should report at least one orphaned spec: {doctor_output:?}"
                    );
                }
            }
        }
        other => panic!("unsupported assertion type `{other}` in fixture {fixture_name}"),
    }
}

#[test]
fn test_adversarial_aliased_deep_import() {
    assert_fixture_behavior("aliased-deep-import");
}

#[test]
fn test_adversarial_barrel_re_export_chain() {
    assert_fixture_behavior("barrel-re-export-chain");
}

#[test]
fn test_adversarial_circular_via_re_export() {
    assert_fixture_behavior("circular-via-re-export");
}

#[test]
fn test_adversarial_conditional_require() {
    assert_fixture_behavior("conditional-require");
}

#[test]
fn test_adversarial_cross_layer_shortcut() {
    assert_fixture_behavior("cross-layer-shortcut");
}

#[test]
fn test_adversarial_deep_third_party_import() {
    assert_fixture_behavior("deep-third-party-import");
}

#[test]
fn test_adversarial_dynamic_import_evasion() {
    assert_fixture_behavior("dynamic-import-evasion");
}

#[test]
fn test_adversarial_hallucinated_import() {
    assert_fixture_behavior("hallucinated-import");
}

#[test]
fn test_adversarial_orphan_module() {
    assert_fixture_behavior("orphan-module");
}

#[test]
fn test_adversarial_ownership_overlap() {
    assert_fixture_behavior("ownership-overlap");
}

#[test]
fn test_adversarial_path_traversal() {
    assert_fixture_behavior("path-traversal");
}

#[test]
fn test_adversarial_test_helper_leak() {
    assert_fixture_behavior("test-helper-leak");
}

#[test]
fn test_adversarial_type_import_downgrade() {
    assert_fixture_behavior("type-import-downgrade");
}

#[test]
fn test_adversarial_wildcard_re_export_leak() {
    assert_fixture_behavior("wildcard-re-export-leak");
}
