//! Contract Edge Cases and Blast-Radius E2E Tests (W9-T2)
//!
//! Focused tests for:
//! 1. contract_ref_invalid violations (duplicate IDs, invalid imports_contract format)
//! 2. --since evaluation-time scoping for contract checks
//! 3. Contract violations are baselineable

use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::Value;
use tempfile::TempDir;

use specgate::cli::{run, EXIT_CODE_PASS, EXIT_CODE_POLICY_VIOLATIONS, EXIT_CODE_RUNTIME_ERROR};

fn parse_json(source: &str) -> Value {
    serde_json::from_str(source).expect("valid json")
}

fn init_git_repo(root: &Path) {
    let status = Command::new("git")
        .arg("init")
        .current_dir(root)
        .status()
        .expect("git init");
    assert!(status.success());

    let status = Command::new("git")
        .args(["config", "user.name", "Specgate Test"])
        .current_dir(root)
        .status()
        .expect("git config user.name");
    assert!(status.success());

    let status = Command::new("git")
        .args(["config", "user.email", "ci@example.com"])
        .current_dir(root)
        .status()
        .expect("git config user.email");
    assert!(status.success());
}

fn git_add_and_commit(root: &Path, message: &str) {
    let status = Command::new("git")
        .args(["add", "."])
        .current_dir(root)
        .status()
        .expect("git add");
    assert!(status.success());

    let status = Command::new("git")
        .args(["commit", "-m", message])
        .current_dir(root)
        .status()
        .expect("git commit");
    assert!(status.success());
}

fn write_file(root: &Path, relative_path: &str, content: &str) {
    let path = root.join(relative_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("mkdir parent");
    }
    fs::write(path, content).expect("write file");
}

fn fixture_root(relative_path: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(relative_path)
}

// =============================================================================
// Test Group 1: contract_ref_invalid Validation Errors
// Note: contract_ref_invalid are validation errors (exit code 2), not violations
// =============================================================================

#[test]
fn contract_ref_invalid_duplicate_ids_detected() {
    let temp = TempDir::new().expect("tempdir");

    fs::write(
        temp.path().join("specgate.config.yml"),
        "spec_version: \"2.3\"",
    )
    .expect("write config");

    let modules_dir = temp.path().join("modules").join("api");
    fs::create_dir_all(&modules_dir).expect("create api module");

    let spec = r#"version: "2.3"
module: api
boundaries:
  path: src/api/**
  contracts:
    - id: duplicate_id
      contract: contracts/first.json
      match:
        files:
          - src/api/first.ts
    - id: duplicate_id
      contract: contracts/second.json
      match:
        files:
          - src/api/second.ts"#;

    fs::write(modules_dir.join("api.spec.yml"), spec).expect("write spec");

    let src_dir = modules_dir.join("src").join("api");
    fs::create_dir_all(&src_dir).expect("create src/api");
    fs::write(src_dir.join("first.ts"), "export const first = 1;").expect("write first.ts");
    fs::write(src_dir.join("second.ts"), "export const second = 2;").expect("write second.ts");

    let contracts_dir = modules_dir.join("contracts");
    fs::create_dir_all(&contracts_dir).expect("create contracts");
    fs::write(contracts_dir.join("first.json"), r#"{"type": "object"}"#).expect("write first.json");
    fs::write(contracts_dir.join("second.json"), r#"{"type": "object"}"#).expect("write second.json");

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--format",
        "json",
    ]);

    // Validation errors return runtime error exit code
    assert_eq!(result.exit_code, EXIT_CODE_RUNTIME_ERROR);

    let verdict: Value = parse_json(&result.stdout);
    assert_eq!(verdict["status"], "error");
    assert_eq!(verdict["code"], "validation");

    let details = verdict["details"].as_array().expect("details array");
    let has_duplicate = details.iter().any(|d| {
        d.as_str().unwrap_or("").contains("boundary.contract_ref_invalid")
            && d.as_str().unwrap_or("").contains("duplicate contract id")
    });

    assert!(
        has_duplicate,
        "expected contract_ref_invalid for duplicate id in validation errors: {details:?}",
    );
}

#[test]
fn contract_ref_invalid_bad_imports_contract_format() {
    let temp = TempDir::new().expect("tempdir");

    fs::write(
        temp.path().join("specgate.config.yml"),
        "spec_version: \"2.3\"",
    )
    .expect("write config");

    let modules_dir = temp.path().join("modules").join("service");
    fs::create_dir_all(&modules_dir).expect("create service module");

    let spec = r#"version: "2.3"
module: service
boundaries:
  path: src/service/**
  contracts:
    - id: bad_ref_contract
      contract: contracts/api.json
      imports_contract:
        - "invalid_no_colon"
        - "module:extra:parts"
        - ":empty_module"
      match:
        files:
          - src/service/index.ts"#;

    fs::write(modules_dir.join("service.spec.yml"), spec).expect("write spec");

    let src_dir = modules_dir.join("src").join("service");
    fs::create_dir_all(&src_dir).expect("create src/service");
    fs::write(src_dir.join("index.ts"), "export const service = 1;").expect("write index.ts");

    let contracts_dir = modules_dir.join("contracts");
    fs::create_dir_all(&contracts_dir).expect("create contracts");
    fs::write(contracts_dir.join("api.json"), r#"{"type": "object"}"#).expect("write api.json");

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--format",
        "json",
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_RUNTIME_ERROR);

    let verdict: Value = parse_json(&result.stdout);
    let details = verdict["details"].as_array().expect("details array");
    let has_invalid_format = details.iter().any(|d| {
        let msg = d.as_str().unwrap_or("");
        msg.contains("boundary.contract_ref_invalid")
    });

    assert!(
        has_invalid_format,
        "expected contract_ref_invalid for bad imports_contract format: {details:?}",
    );
}

// =============================================================================
// Test Group 2: --since Evaluation-Time Scoping for Contract Checks
// =============================================================================

#[test]
fn since_blast_radius_includes_contract_violations_in_affected_modules() {
    let temp = TempDir::new().expect("tempdir");

    write_file(
        temp.path(),
        "modules/core.spec.yml",
        r#"version: "2.3"
module: core
boundaries:
  path: src/core/**
  contracts:
    - id: core_contract
      contract: contracts/core.json
      match:
        files:
          - src/core/index.ts
      envelope: required"#,
    );
    write_file(
        temp.path(),
        "modules/api.spec.yml",
        r#"version: "2.3"
module: api
boundaries:
  path: src/api/**
  contracts:
    - id: api_contract
      contract: contracts/api.json
      imports_contract:
        - "core:core_contract"
      match:
        files:
          - src/api/handler.ts
      envelope: required"#,
    );
    write_file(
        temp.path(),
        "modules/app.spec.yml",
        r#"version: "2.3"
module: app
boundaries:
  path: src/app/**
constraints: []"#,
    );
    write_file(
        temp.path(),
        "specgate.config.yml",
        r#"spec_version: "2.3"
spec_dirs:
  - modules"#,
    );

    write_file(temp.path(), "src/core/index.ts", "export const core = 1;");
    write_file(
        temp.path(),
        "src/api/handler.ts",
        "import { core } from '../core/index';\nexport const handler = () => core;",
    );
    write_file(
        temp.path(),
        "src/app/main.ts",
        "import { handler } from '../api/handler';\nexport const app = handler;",
    );

    write_file(temp.path(), "contracts/core.json", r#"{"type": "object"}"#);
    write_file(temp.path(), "contracts/api.json", r#"{"type": "object"}"#);

    init_git_repo(temp.path());
    git_add_and_commit(temp.path(), "chore: initial commit");

    write_file(temp.path(), "src/core/index.ts", "export const core = 2;");
    git_add_and_commit(temp.path(), "chore: touch core");

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--since",
        "HEAD~1",
        "--format",
        "json",
        "--no-baseline",
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_PASS);

    let verdict: Value = parse_json(&result.stdout);
    assert_eq!(verdict["status"], "pass");
}

#[test]
fn since_blast_radius_filters_contract_violations_in_unaffected_modules() {
    let temp = TempDir::new().expect("tempdir");

    write_file(
        temp.path(),
        "modules/unaffected.spec.yml",
        r#"version: "2.3"
module: unaffected
boundaries:
  path: src/unaffected/**
  contracts:
    - id: missing_contract
      contract: contracts/missing.json
      match:
        files:
          - src/unaffected/index.ts"#,
    );
    write_file(
        temp.path(),
        "modules/affected.spec.yml",
        r#"version: "2.3"
module: affected
boundaries:
  path: src/affected/**
  contracts:
    - id: valid_contract
      contract: contracts/valid.json
      match:
        files:
          - src/affected/index.ts"#,
    );
    write_file(
        temp.path(),
        "specgate.config.yml",
        r#"spec_version: "2.3"
spec_dirs:
  - modules"#,
    );

    write_file(temp.path(), "src/unaffected/index.ts", "export const unaffected = 1;");
    write_file(temp.path(), "src/affected/index.ts", "export const affected = 2;");

    write_file(temp.path(), "contracts/valid.json", r#"{"type": "object"}"#);

    init_git_repo(temp.path());
    git_add_and_commit(temp.path(), "chore: initial commit");

    write_file(temp.path(), "src/affected/index.ts", "export const affected = 3;");
    git_add_and_commit(temp.path(), "chore: touch affected");

    let full_result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--format",
        "json",
        "--no-baseline",
    ]);

    let full_verdict: Value = parse_json(&full_result.stdout);
    let full_violations = full_verdict["violations"].as_array().expect("violations");
    let contract_missing_count = full_violations
        .iter()
        .filter(|v| v["rule"].as_str() == Some("boundary.contract_missing"))
        .count();
    assert!(
        contract_missing_count > 0,
        "full check should find contract_missing violations"
    );

    let since_result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--since",
        "HEAD~1",
        "--format",
        "json",
        "--no-baseline",
    ]);

    let since_verdict: Value = parse_json(&since_result.stdout);
    let since_violations = since_verdict["violations"].as_array().expect("violations");
    let since_contract_missing = since_violations
        .iter()
        .filter(|v| v["rule"].as_str() == Some("boundary.contract_missing"))
        .count();

    assert_eq!(
        since_contract_missing, 0,
        "--since should filter contract violations in unaffected modules"
    );
}

#[test]
fn since_blast_radius_with_transitive_contract_dependencies() {
    let temp = TempDir::new().expect("tempdir");

    write_file(
        temp.path(),
        "modules/core.spec.yml",
        r#"version: "2.3"
module: core
boundaries:
  path: src/core/**
  contracts:
    - id: core_types
      contract: contracts/core.json
      match:
        files:
          - src/core/types.ts"#,
    );
    write_file(
        temp.path(),
        "modules/api.spec.yml",
        r#"version: "2.3"
module: api
boundaries:
  path: src/api/**
  contracts:
    - id: api_types
      contract: contracts/api.json
      imports_contract:
        - "core:core_types"
      match:
        files:
          - src/api/types.ts"#,
    );
    write_file(
        temp.path(),
        "modules/app.spec.yml",
        r#"version: "2.3"
module: app
boundaries:
  path: src/app/**
  contracts:
    - id: app_contract
      contract: contracts/app.json
      imports_contract:
        - "api:api_types"
      match:
        files:
          - src/app/main.ts"#,
    );
    write_file(
        temp.path(),
        "specgate.config.yml",
        r#"spec_version: "2.3"
spec_dirs:
  - modules"#,
    );

    write_file(temp.path(), "src/core/types.ts", "export interface CoreType { id: string; }");
    write_file(
        temp.path(),
        "src/api/types.ts",
        "import { CoreType } from '../core/types';\nexport interface ApiType { core: CoreType; }",
    );
    write_file(
        temp.path(),
        "src/app/main.ts",
        "import { ApiType } from '../api/types';\nexport const app = (x: ApiType) => x;",
    );

    write_file(temp.path(), "contracts/core.json", r#"{"type": "object"}"#);
    write_file(temp.path(), "contracts/api.json", r#"{"type": "object"}"#);
    write_file(temp.path(), "contracts/app.json", r#"{"type": "object"}"#);

    init_git_repo(temp.path());
    git_add_and_commit(temp.path(), "chore: initial commit");

    write_file(temp.path(), "src/core/types.ts", "export interface CoreType { id: number; }");
    git_add_and_commit(temp.path(), "chore: update core types");

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--since",
        "HEAD~1",
        "--format",
        "json",
        "--no-baseline",
    ]);

    let verdict: Value = parse_json(&result.stdout);
    assert_eq!(verdict["status"], "pass");
}

// =============================================================================
// Test Group 3: Contract Violations are Baselineable
// =============================================================================

#[test]
fn contract_violations_are_baselineable() {
    let temp = TempDir::new().expect("tempdir");

    fs::write(
        temp.path().join("specgate.config.yml"),
        "spec_version: \"2.3\"",
    )
    .expect("write config");

    let modules_dir = temp.path().join("modules").join("api");
    fs::create_dir_all(&modules_dir).expect("create api module");

    let spec = r#"version: "2.3"
module: api
boundaries:
  path: src/api/**
  contracts:
    - id: missing_contract
      contract: contracts/nonexistent.json
      match:
        files:
          - src/api/handler.ts"#;

    fs::write(modules_dir.join("api.spec.yml"), spec).expect("write spec");

    let src_dir = modules_dir.join("src").join("api");
    fs::create_dir_all(&src_dir).expect("create src/api");
    fs::write(src_dir.join("handler.ts"), "export const handler = 1;").expect("write handler.ts");

    let check_result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--format",
        "json",
        "--no-baseline",
    ]);

    assert_eq!(check_result.exit_code, EXIT_CODE_POLICY_VIOLATIONS);

    let verdict: Value = parse_json(&check_result.stdout);
    let violations = verdict["violations"].as_array().expect("violations");
    let contract_missing_violations: Vec<_> = violations
        .iter()
        .filter(|v| v["rule"].as_str() == Some("boundary.contract_missing"))
        .collect();
    assert!(
        !contract_missing_violations.is_empty(),
        "should have contract_missing violations without baseline"
    );

    let baseline_result = run([
        "specgate",
        "baseline",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);
    assert_eq!(baseline_result.exit_code, EXIT_CODE_PASS);

    let baseline_path = temp.path().join(".specgate-baseline.json");
    assert!(baseline_path.exists(), "baseline file should be created");

    let check_with_baseline = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--format",
        "json",
    ]);

    assert_eq!(check_with_baseline.exit_code, EXIT_CODE_PASS);

    let baseline_verdict: Value = parse_json(&check_with_baseline.stdout);
    // Baseline violations can be 1 or more (may include match_unresolved)
    let baseline_count = baseline_verdict["summary"]["baseline_violations"].as_u64().unwrap_or(0);
    assert!(
        baseline_count >= 1,
        "should have at least 1 baseline violation, got {baseline_count}",
    );
    assert_eq!(baseline_verdict["summary"]["new_violations"], 0);
}

#[test]
fn contract_ref_invalid_violations_are_baselineable() {
    let temp = TempDir::new().expect("tempdir");

    fs::write(
        temp.path().join("specgate.config.yml"),
        "spec_version: \"2.3\"",
    )
    .expect("write config");

    let modules_dir = temp.path().join("modules").join("service");
    fs::create_dir_all(&modules_dir).expect("create service module");

    let spec = r#"version: "2.3"
module: service
boundaries:
  path: src/service/**
  contracts:
    - id: dup_id
      contract: contracts/a.json
      match:
        files:
          - src/service/a.ts
    - id: dup_id
      contract: contracts/b.json
      match:
        files:
          - src/service/b.ts"#;

    fs::write(modules_dir.join("service.spec.yml"), spec).expect("write spec");

    let src_dir = modules_dir.join("src").join("service");
    fs::create_dir_all(&src_dir).expect("create src/service");
    fs::write(src_dir.join("a.ts"), "export const a = 1;").expect("write a.ts");
    fs::write(src_dir.join("b.ts"), "export const b = 2;").expect("write b.ts");

    let contracts_dir = modules_dir.join("contracts");
    fs::create_dir_all(&contracts_dir).expect("create contracts");
    fs::write(contracts_dir.join("a.json"), r#"{"type": "object"}"#).expect("write a.json");
    fs::write(contracts_dir.join("b.json"), r#"{"type": "object"}"#).expect("write b.json");

    // Validation errors can't be baselined - they prevent the check from running
    let baseline_result = run([
        "specgate",
        "baseline",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);

    // Baseline should fail due to validation errors
    assert_eq!(baseline_result.exit_code, EXIT_CODE_RUNTIME_ERROR);

    let verdict: Value = parse_json(&baseline_result.stdout);
    assert_eq!(verdict["status"], "error");
    let details = verdict["details"].as_array().expect("details");
    let has_dup_error = details.iter().any(|d| {
        d.as_str().unwrap_or("").contains("boundary.contract_ref_invalid")
    });
    assert!(has_dup_error, "baseline should report contract_ref_invalid error");
}

// match_unresolved is a regular violation and can be baselined
#[test]
fn match_unresolved_violations_are_baselineable() {
    let temp = TempDir::new().expect("tempdir");

    fs::write(
        temp.path().join("specgate.config.yml"),
        "spec_version: \"2.3\"",
    )
    .expect("write config");

    let modules_dir = temp.path().join("modules").join("api");
    fs::create_dir_all(&modules_dir).expect("create api module");

    let spec = r#"version: "2.3"
module: api
boundaries:
  path: src/api/**
  contracts:
    - id: unresolved_match
      contract: contracts/api.json
      match:
        files:
          - src/api/nonexistent.ts"#;

    fs::write(modules_dir.join("api.spec.yml"), spec).expect("write spec");

    let src_dir = modules_dir.join("src").join("api");
    fs::create_dir_all(&src_dir).expect("create src/api");
    fs::write(src_dir.join("other.ts"), "export const other = 1;").expect("write other.ts");

    let contracts_dir = modules_dir.join("contracts");
    fs::create_dir_all(&contracts_dir).expect("create contracts");
    fs::write(contracts_dir.join("api.json"), r#"{"type": "object"}"#).expect("write api.json");

    // Baseline should succeed and capture match_unresolved
    let baseline_result = run([
        "specgate",
        "baseline",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);

    assert_eq!(baseline_result.exit_code, EXIT_CODE_PASS);

    let check_result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--format",
        "json",
    ]);

    assert_eq!(check_result.exit_code, EXIT_CODE_PASS);

    let verdict: Value = parse_json(&check_result.stdout);
    let violations = verdict["violations"].as_array().expect("violations");
    let match_unresolved_baseline: Vec<_> = violations
        .iter()
        .filter(|v| {
            v["rule"].as_str() == Some("boundary.match_unresolved") &&
            v["disposition"].as_str() == Some("baseline")
        })
        .collect();

    assert!(
        !match_unresolved_baseline.is_empty(),
        "match_unresolved violations should be baselined"
    );
}

// =============================================================================
// Test Group 4: Fixture-Based Edge Case Tests
// =============================================================================

#[test]
fn fixture_duplicate_contract_ids_produces_ref_invalid() {
    let fixture = fixture_root("contract-project-edge");

    let result = run([
        "specgate",
        "check",
        "--project-root",
        fixture.to_str().expect("utf8"),
        "--format",
        "json",
        "--no-baseline",
    ]);

    // The fixture has duplicate contract IDs - this is a validation error
    assert_eq!(result.exit_code, EXIT_CODE_RUNTIME_ERROR);

    let verdict: Value = parse_json(&result.stdout);
    assert_eq!(verdict["status"], "error");

    let details = verdict["details"].as_array().expect("details");
    let has_duplicate = details.iter().any(|d| {
        let msg = d.as_str().unwrap_or("");
        msg.contains("boundary.contract_ref_invalid") && msg.contains("duplicate")
    });

    assert!(
        has_duplicate,
        "fixture should produce contract_ref_invalid for duplicate IDs: {details:?}",
    );
}

#[test]
fn fixture_invalid_imports_contract_produces_ref_invalid() {
    let fixture = fixture_root("contract-project-edge");

    let result = run([
        "specgate",
        "check",
        "--project-root",
        fixture.to_str().expect("utf8"),
        "--format",
        "json",
        "--no-baseline",
    ]);

    let verdict: Value = parse_json(&result.stdout);
    let details = verdict["details"].as_array().expect("details");

    let has_invalid = details.iter().any(|d| {
        let msg = d.as_str().unwrap_or("");
        msg.contains("boundary.contract_ref_invalid")
    });

    assert!(
        has_invalid,
        "fixture should produce contract_ref_invalid: {details:?}",
    );
}
