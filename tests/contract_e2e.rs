//! Happy-path E2E Contract Tests (W9-T1)
//!
//! These tests verify end-to-end contract behavior:
//! 1. Valid contract passes
//! 2. Contract missing, empty, match_unresolved
//! 3. Contracts in 2.2 specs fail validation
//!
//! All tests force --format json for consistent output parsing.

use std::fs;
use std::path::PathBuf;

use serde_json::Value;
use tempfile::TempDir;

use specgate::cli::{run, EXIT_CODE_PASS, EXIT_CODE_POLICY_VIOLATIONS, EXIT_CODE_RUNTIME_ERROR};

fn fixture_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/contract-project")
        .join(relative)
}

fn parse_json(source: &str) -> Value {
    serde_json::from_str(source).expect("valid json")
}

fn write_file(root: &std::path::Path, relative_path: &str, content: &str) {
    let path = root.join(relative_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("mkdir parent");
    }
    fs::write(path, content).expect("write file");
}

// =============================================================================
// Test 1: Valid contract passes
// =============================================================================

/// Test that a valid contract with matching file passes.
#[test]
fn valid_contract_passes() {
    let temp = TempDir::new().expect("tempdir");

    // Copy fixture structure to temp dir
    write_file(
        temp.path(),
        "specgate.config.yml",
        "spec_dirs:\n  - modules\nexclude: []\ntest_patterns: []\n",
    );

    write_file(
        temp.path(),
        "modules/api.spec.yml",
        r#"
version: "2.3"
module: api
boundaries:
  path: src/api/**/*
  contracts:
    - id: create_user
      contract: contracts/create-user.json
      match:
        files:
          - src/api/handlers/users.ts
constraints: []
"#,
    );

    // Create valid contract file
    write_file(
        temp.path(),
        "contracts/create-user.json",
        r#"{"type": "object", "properties": {"name": {"type": "string"}}, "required": ["name"]}"#,
    );

    // Create matching source file
    write_file(
        temp.path(),
        "src/api/handlers/users.ts",
        "export interface CreateUserRequest { name: string; }\n",
    );

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--format",
        "json",
        "--no-baseline",
    ]);

    assert_eq!(
        result.exit_code, EXIT_CODE_PASS,
        "valid contract should pass"
    );
    let output = parse_json(&result.stdout);
    assert_eq!(output["status"], "pass");
}

// =============================================================================
// Test 2: Contract missing
// =============================================================================

/// Test that a missing contract file is detected and reported.
#[test]
fn contract_missing_detected() {
    let temp = TempDir::new().expect("tempdir");

    write_file(
        temp.path(),
        "specgate.config.yml",
        "spec_dirs:\n  - modules\nexclude: []\ntest_patterns: []\n",
    );

    write_file(
        temp.path(),
        "modules/api.spec.yml",
        r#"
version: "2.3"
module: api
boundaries:
  path: src/api/**/*
  contracts:
    - id: missing_contract
      contract: contracts/non-existent.json
      match:
        files:
          - src/api/handlers/users.ts
constraints: []
"#,
    );

    // Create source file but NOT the contract file
    write_file(
        temp.path(),
        "src/api/handlers/users.ts",
        "export function handler() {}\n",
    );

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--format",
        "json",
        "--no-baseline",
    ]);

    assert_eq!(
        result.exit_code, EXIT_CODE_POLICY_VIOLATIONS,
        "missing contract should fail"
    );

    let output = parse_json(&result.stdout);
    let violations = output["violations"].as_array().expect("violations array");

    let missing_violations: Vec<_> = violations
        .iter()
        .filter(|v| v["rule"].as_str() == Some("boundary.contract_missing"))
        .collect();

    assert!(
        !missing_violations.is_empty(),
        "should have contract_missing violation"
    );

    for v in &missing_violations {
        assert_eq!(
            v["contract_id"].as_str(),
            Some("missing_contract"),
            "violation should have correct contract_id"
        );
        assert!(
            v.get("remediation_hint").is_some() && !v["remediation_hint"].is_null(),
            "contract_missing should have remediation_hint"
        );
    }
}

// =============================================================================
// Test 3: Contract empty
// =============================================================================

/// Test that an empty contract file is detected and reported.
#[test]
fn contract_empty_detected() {
    let temp = TempDir::new().expect("tempdir");

    write_file(
        temp.path(),
        "specgate.config.yml",
        "spec_dirs:\n  - modules\nexclude: []\ntest_patterns: []\n",
    );

    write_file(
        temp.path(),
        "modules/api.spec.yml",
        r#"
version: "2.3"
module: api
boundaries:
  path: src/api/**/*
  contracts:
    - id: empty_contract
      contract: contracts/empty.json
      match:
        files:
          - src/api/handlers/users.ts
constraints: []
"#,
    );

    // Create empty contract file
    fs::create_dir_all(temp.path().join("contracts")).expect("create contracts dir");
    fs::File::create(temp.path().join("contracts/empty.json")).expect("create empty file");

    // Create source file
    write_file(
        temp.path(),
        "src/api/handlers/users.ts",
        "export function handler() {}\n",
    );

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--format",
        "json",
        "--no-baseline",
    ]);

    assert_eq!(
        result.exit_code, EXIT_CODE_POLICY_VIOLATIONS,
        "empty contract should fail"
    );

    let output = parse_json(&result.stdout);
    let violations = output["violations"].as_array().expect("violations array");

    let empty_violations: Vec<_> = violations
        .iter()
        .filter(|v| v["rule"].as_str() == Some("boundary.contract_empty"))
        .collect();

    assert!(
        !empty_violations.is_empty(),
        "should have contract_empty violation"
    );

    for v in &empty_violations {
        assert_eq!(
            v["contract_id"].as_str(),
            Some("empty_contract"),
            "violation should have correct contract_id"
        );
        assert!(
            v.get("remediation_hint").is_some() && !v["remediation_hint"].is_null(),
            "contract_empty should have remediation_hint"
        );
    }
}

// =============================================================================
// Test 4: Match unresolved
// =============================================================================

/// Test that a match pattern that resolves to no files is detected.
#[test]
fn match_unresolved_detected() {
    let temp = TempDir::new().expect("tempdir");

    write_file(
        temp.path(),
        "specgate.config.yml",
        "spec_dirs:\n  - modules\nexclude: []\ntest_patterns: []\n",
    );

    write_file(
        temp.path(),
        "modules/api.spec.yml",
        r#"
version: "2.3"
module: api
boundaries:
  path: src/api/**/*
  contracts:
    - id: unresolved_pattern
      contract: contracts/api.json
      match:
        files:
          - src/api/non-existent/**/*.ts
constraints: []
"#,
    );

    // Create valid contract file
    write_file(temp.path(), "contracts/api.json", r#"{"type": "object"}"#);

    // Create some source files but NOT the matched pattern
    write_file(
        temp.path(),
        "src/api/other.ts",
        "export function other() {}\n",
    );

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--format",
        "json",
        "--no-baseline",
    ]);

    assert_eq!(
        result.exit_code, EXIT_CODE_POLICY_VIOLATIONS,
        "unresolved match should fail"
    );

    let output = parse_json(&result.stdout);
    let violations = output["violations"].as_array().expect("violations array");

    let unresolved_violations: Vec<_> = violations
        .iter()
        .filter(|v| v["rule"].as_str() == Some("boundary.match_unresolved"))
        .collect();

    assert!(
        !unresolved_violations.is_empty(),
        "should have match_unresolved violation"
    );

    for v in &unresolved_violations {
        assert_eq!(
            v["contract_id"].as_str(),
            Some("unresolved_pattern"),
            "violation should have correct contract_id"
        );
        assert!(
            v.get("remediation_hint").is_some() && !v["remediation_hint"].is_null(),
            "match_unresolved should have remediation_hint"
        );
    }
}

// =============================================================================
// Test 5: Contracts in 2.2 fail validation
// =============================================================================

/// Test that contracts in 2.2 specs fail validation.
#[test]
fn contracts_in_2_2_fail_validation() {
    let temp = TempDir::new().expect("tempdir");

    write_file(
        temp.path(),
        "specgate.config.yml",
        "spec_dirs:\n  - modules\nexclude: []\ntest_patterns: []\n",
    );

    // 2.2 spec with contracts (not allowed)
    write_file(
        temp.path(),
        "modules/api.spec.yml",
        r#"
version: "2.2"
module: api
boundaries:
  path: src/api/**/*
  contracts:
    - id: invalid_contract
      contract: contracts/api.json
      match:
        files:
          - src/api/handlers.ts
constraints: []
"#,
    );

    // Create source file
    write_file(
        temp.path(),
        "src/api/handlers.ts",
        "export function handler() {}\n",
    );

    // Validate should fail
    let validate_result = run([
        "specgate",
        "validate",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);

    assert_eq!(
        validate_result.exit_code, EXIT_CODE_RUNTIME_ERROR,
        "2.2 with contracts should fail validation"
    );

    let output = parse_json(&validate_result.stdout);
    assert_eq!(output["status"], "error");

    let issues = output["issues"].as_array().expect("issues array");
    let version_mismatch_issues: Vec<_> = issues
        .iter()
        .filter(|i| {
            i["message"]
                .as_str()
                .map_or(false, |m| m.contains("boundary.contract_version_mismatch"))
        })
        .collect();

    assert!(
        !version_mismatch_issues.is_empty(),
        "should report contract version mismatch error: {issues:?}"
    );
}

// =============================================================================
// Test 6: Multiple contract violations in single run
// =============================================================================

/// Test that multiple contract violations are all reported.
#[test]
fn multiple_contract_violations_reported() {
    let temp = TempDir::new().expect("tempdir");

    write_file(
        temp.path(),
        "specgate.config.yml",
        "spec_dirs:\n  - modules\nexclude: []\ntest_patterns: []\n",
    );

    write_file(
        temp.path(),
        "modules/api.spec.yml",
        r#"
version: "2.3"
module: api
boundaries:
  path: src/api/**/*
  contracts:
    - id: missing_contract
      contract: contracts/missing.json
      match:
        files:
          - src/api/existing.ts
    - id: unresolved_pattern
      contract: contracts/api.json
      match:
        files:
          - src/api/non-existent/**/*.ts
constraints: []
"#,
    );

    // Create empty contract file (for empty_contract test)
    fs::create_dir_all(temp.path().join("contracts")).expect("create contracts dir");
    fs::File::create(temp.path().join("contracts/api.json")).expect("create empty file");

    // Create one matching file
    write_file(
        temp.path(),
        "src/api/existing.ts",
        "export function existing() {}\n",
    );

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--format",
        "json",
        "--no-baseline",
    ]);

    assert_eq!(
        result.exit_code, EXIT_CODE_POLICY_VIOLATIONS,
        "should fail with violations"
    );

    let output = parse_json(&result.stdout);
    let violations = output["violations"].as_array().expect("violations array");

    // Check for all three violation types
    let has_missing = violations
        .iter()
        .any(|v| v["rule"].as_str() == Some("boundary.contract_missing"));
    let has_empty = violations
        .iter()
        .any(|v| v["rule"].as_str() == Some("boundary.contract_empty"));
    let has_unresolved = violations
        .iter()
        .any(|v| v["rule"].as_str() == Some("boundary.match_unresolved"));

    assert!(has_missing, "should have contract_missing violation");
    assert!(has_empty, "should have contract_empty violation");
    assert!(has_unresolved, "should have match_unresolved violation");
}

// =============================================================================
// Test 7: Fixture project passes
// =============================================================================

/// Test that the fixture project with valid contract passes.
#[test]
fn fixture_project_valid_contract_passes() {
    let fixture_root =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/contract-project");

    let result = run([
        "specgate",
        "check",
        "--project-root",
        fixture_root.to_str().expect("utf8"),
        "--format",
        "json",
        "--no-baseline",
    ]);

    assert_eq!(
        result.exit_code, EXIT_CODE_PASS,
        "fixture project should pass: stderr={}",
        result.stderr
    );

    let output = parse_json(&result.stdout);
    assert_eq!(output["status"], "pass");
}

// =============================================================================
// Test 8: Validate fixture project spec
// =============================================================================

/// Test that the fixture project spec passes validation.
#[test]
fn fixture_project_spec_passes_validation() {
    let fixture_root =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/contract-project");

    let result = run([
        "specgate",
        "validate",
        "--project-root",
        fixture_root.to_str().expect("utf8"),
    ]);

    assert_eq!(
        result.exit_code, EXIT_CODE_PASS,
        "fixture project should validate: stderr={}",
        result.stderr
    );

    let output = parse_json(&result.stdout);
    assert_eq!(output["status"], "ok");
}
