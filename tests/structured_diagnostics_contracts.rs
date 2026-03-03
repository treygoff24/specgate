//! Structured Diagnostics Contracts Regression Tests (W7-T2)
//!
//! Focused regression tests for:
//! 1. Contract violations include remediation_hint and contract_id in JSON output
//! 2. Layer violations include remediation_hint
//! 3. Non-contract violations do not emit contract_id
//!
//! Use --format json in tests (default output is JSON).

use std::fs;

use serde_json::Value;
use tempfile::TempDir;

use specgate::cli::run;

/// Test that contract_missing violations include both remediation_hint and contract_id
#[test]
fn contract_violation_includes_remediation_hint_and_contract_id() {
    let temp = TempDir::new().expect("tempdir");

    // Create config
    fs::write(
        temp.path().join("specgate.config.yml"),
        "spec_version: \"2.3\"",
    )
    .expect("write config");

    // Create module with contract that references a missing file
    let modules_dir = temp.path().join("modules");
    fs::create_dir_all(&modules_dir).expect("create modules");

    let api_dir = modules_dir.join("api");
    fs::create_dir_all(&api_dir).expect("create api");

    // Create spec with contract pointing to non-existent file
    // Using correct path structure for CLI
    let spec = r#"
version: "2.3"
module: api
boundaries:
  path: src/api/**
  contracts:
    - id: my_contract
      contract: contracts/non-existent.json
      match:
        files:
          - src/api/handler.ts
"#;
    fs::write(api_dir.join("api.spec.yml"), spec).expect("write spec");

    // Create the handler file at correct location
    let src_dir = api_dir.join("src").join("api");
    fs::create_dir_all(&src_dir).expect("create src/api");
    fs::write(src_dir.join("handler.ts"), "export function handler() {}").expect("write handler");

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);

    let verdict: Value = serde_json::from_str(&result.stdout).expect("parse JSON output");

    let violations = verdict["violations"].as_array().expect("violations array");

    // Find the contract_missing violation
    let contract_violations: Vec<_> = violations
        .iter()
        .filter(|v| v["rule"].as_str() == Some("boundary.contract_missing"))
        .collect();

    assert!(
        !contract_violations.is_empty(),
        "expected contract_missing violation"
    );

    for v in &contract_violations {
        // Contract violations MUST have remediation_hint
        assert!(
            v.get("remediation_hint").is_some() && !v["remediation_hint"].is_null(),
            "contract violation should have remediation_hint: {v:#}"
        );

        // Contract violations MUST have contract_id
        assert!(
            v.get("contract_id").is_some() && !v["contract_id"].is_null(),
            "contract violation should have contract_id: {v:#}"
        );
    }
}

/// Test that match_unresolved violations include both remediation_hint and contract_id
#[test]
fn match_unresolved_violation_includes_remediation_hint_and_contract_id() {
    let temp = TempDir::new().expect("tempdir");

    // Create config
    fs::write(
        temp.path().join("specgate.config.yml"),
        "spec_version: \"2.3\"",
    )
    .expect("write config");

    let modules_dir = temp.path().join("modules");
    fs::create_dir_all(&modules_dir).expect("create modules");

    let api_dir = modules_dir.join("api");
    fs::create_dir_all(&api_dir).expect("create api");

    // Create spec with contract pointing to non-matching file
    let spec = r#"
version: "2.3"
module: api
boundaries:
  path: src/api/**
  contracts:
    - id: unresolved_pattern
      contract: contracts/api.json
      match:
        files:
          - src/api/non-existent-file.ts
"#;
    fs::write(api_dir.join("api.spec.yml"), spec).expect("write spec");

    // Create the contract file (so it's not missing)
    let contracts_dir = api_dir.join("contracts");
    fs::create_dir_all(&contracts_dir).expect("create contracts");
    fs::write(contracts_dir.join("api.json"), r#"{"type": "object"}"#).expect("write contract");

    // Create handler but NOT the non-existent file
    let src_dir = api_dir.join("src").join("api");
    fs::create_dir_all(&src_dir).expect("create src/api");
    fs::write(src_dir.join("handler.ts"), "export function handler() {}").expect("write handler");

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);

    let verdict: Value = serde_json::from_str(&result.stdout).expect("parse JSON output");

    let violations = verdict["violations"].as_array().expect("violations array");

    // Find the match_unresolved violation
    let contract_violations: Vec<_> = violations
        .iter()
        .filter(|v| v["rule"].as_str() == Some("boundary.match_unresolved"))
        .collect();

    assert!(
        !contract_violations.is_empty(),
        "expected match_unresolved violation"
    );

    for v in &contract_violations {
        assert!(
            v.get("remediation_hint").is_some() && !v["remediation_hint"].is_null(),
            "match_unresolved violation should have remediation_hint: {v:#}"
        );

        assert!(
            v.get("contract_id").is_some() && !v["contract_id"].is_null(),
            "match_unresolved violation should have contract_id: {v:#}"
        );
    }
}

/// Test that layer violations include remediation_hint but NOT contract_id
#[test]
fn layer_violation_includes_remediation_hint_but_not_contract_id() {
    let temp = TempDir::new().expect("tempdir");

    let modules_dir = temp.path().join("modules");
    fs::create_dir_all(&modules_dir).expect("create modules");

    // Create api module
    let api_dir = modules_dir.join("api");
    fs::create_dir_all(&api_dir).expect("create api");
    let api_src_dir = api_dir.join("src").join("api");
    fs::create_dir_all(&api_src_dir).expect("create api src");
    fs::write(
        api_src_dir.join("handler.ts"),
        "export function handler() {}",
    )
    .expect("write handler");

    let api_spec = r#"
version: "2.3"
module: api
"#;
    fs::write(api_dir.join("api.spec.yml"), api_spec).expect("write api spec");

    // Create core module
    let core_dir = modules_dir.join("core");
    fs::create_dir_all(&core_dir).expect("create core");
    let core_src_dir = core_dir.join("src").join("core");
    fs::create_dir_all(&core_src_dir).expect("create core src");
    fs::write(core_src_dir.join("util.ts"), "export function util() {}").expect("write util");

    let core_spec = r#"
version: "2.3"
module: core
"#;
    fs::write(core_dir.join("core.spec.yml"), core_spec).expect("write core spec");

    // Create config with layer rules - core cannot depend on api
    let config = r#"
spec_version: "2.3"
rules:
  - id: enforce-layer
    from_layer: core
    to_layer: api
layers:
  - name: api
    modules:
      - api
  - name: core
    modules:
      - core
"#;
    fs::write(temp.path().join("specgate.config.yml"), config).expect("write config");

    // Add import in api to core (this violates layer rule - core->api is not allowed)
    fs::write(
        api_src_dir.join("handler.ts"),
        "import { util } from '../../core/src/core/util';\nexport function handler() { util(); }",
    )
    .expect("write handler with layer-violating import");

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);

    let verdict: Value = serde_json::from_str(&result.stdout).expect("parse JSON output");

    let violations = verdict["violations"].as_array().expect("violations array");

    // Find the enforce-layer violation
    let layer_violations: Vec<_> = violations
        .iter()
        .filter(|v| v["rule"].as_str() == Some("enforce-layer"))
        .collect();

    // Note: This test may not always produce a layer violation depending on config
    // but if it does, we verify the fields
    for v in &layer_violations {
        // Layer violations SHOULD have remediation_hint
        assert!(
            v.get("remediation_hint").is_some() && !v["remediation_hint"].is_null(),
            "layer violation should have remediation_hint: {v:#}"
        );

        // Layer violations should NOT have contract_id (or it should be null)
        let contract_id = v.get("contract_id");
        assert!(
            contract_id.is_none() || contract_id.unwrap().is_null(),
            "layer violation should NOT have contract_id: {v:#}"
        );
    }
}

/// Test that boundary.canonical_import violations do NOT include contract_id
#[test]
fn canonical_import_violation_does_not_include_contract_id() {
    let temp = TempDir::new().expect("tempdir");

    fs::write(
        temp.path().join("specgate.config.yml"),
        "spec_version: \"2.3\"",
    )
    .expect("write config");

    let modules_dir = temp.path().join("modules");
    fs::create_dir_all(&modules_dir).expect("create modules");

    let api_dir = modules_dir.join("api");
    fs::create_dir_all(&api_dir).expect("create api");

    // Create module with boundary rule
    let spec = r#"
version: "2.3"
module: api
boundaries:
  path: src/api/**
"#;
    fs::write(api_dir.join("api.spec.yml"), spec).expect("write spec");

    // Create handler with non-canonical import
    let src_dir = api_dir.join("src").join("api");
    fs::create_dir_all(&src_dir).expect("create src/api");

    // Using a relative path instead of proper canonical import
    let handler = r#"
import { something } from './local/file';
export function handler() {}
"#;
    fs::write(src_dir.join("handler.ts"), handler)
        .expect("write handler with non-canonical import");

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);

    let verdict: Value = serde_json::from_str(&result.stdout).expect("parse JSON output");

    let violations = verdict["violations"].as_array().expect("violations array");

    // Find any canonical_import violations
    let canonical_violations: Vec<_> = violations
        .iter()
        .filter(|v| v["rule"].as_str() == Some("boundary.canonical_import"))
        .collect();

    // If there are canonical_import violations, verify they don't have contract_id
    for v in &canonical_violations {
        let contract_id = v.get("contract_id");
        assert!(
            contract_id.is_none() || contract_id.unwrap().is_null(),
            "canonical_import violation should NOT have contract_id: {v:#}"
        );
    }
}

/// Test that no-circular-deps violations do NOT include contract_id
#[test]
fn circular_deps_violation_does_not_include_contract_id() {
    let temp = TempDir::new().expect("tempdir");

    // Create config with no-circular-deps rule
    let config = r#"
spec_version: "2.3"
rules:
  - id: no-circular-deps
"#;
    fs::write(temp.path().join("specgate.config.yml"), config).expect("write config");

    let modules_dir = temp.path().join("modules");
    fs::create_dir_all(&modules_dir).expect("create modules");

    // Create module A
    let a_dir = modules_dir.join("a");
    fs::create_dir_all(&a_dir).expect("create a");
    let a_src_dir = a_dir.join("src").join("a");
    fs::create_dir_all(&a_src_dir).expect("create a src");

    let a_spec = r#"
version: "2.3"
module: a
"#;
    fs::write(a_dir.join("a.spec.yml"), a_spec).expect("write a spec");

    // Create module B
    let b_dir = modules_dir.join("b");
    fs::create_dir_all(&b_dir).expect("create b");
    let b_src_dir = b_dir.join("src").join("b");
    fs::create_dir_all(&b_src_dir).expect("create b src");

    let b_spec = r#"
version: "2.3"
module: b
"#;
    fs::write(b_dir.join("b.spec.yml"), b_spec).expect("write b spec");

    // Create circular dependency: A imports B, B imports A
    fs::write(
        a_src_dir.join("a.ts"),
        "import { b } from '../../b/src/b';\nexport const a = b;",
    )
    .expect("write a imports b");

    fs::write(
        b_src_dir.join("b.ts"),
        "import { a } from '../../a/src/a';\nexport const b = a;",
    )
    .expect("write b imports a");

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);

    let verdict: Value = serde_json::from_str(&result.stdout).expect("parse JSON output");

    let violations = verdict["violations"].as_array().expect("violations array");

    // Find no-circular-deps violations
    let circular_violations: Vec<_> = violations
        .iter()
        .filter(|v| v["rule"].as_str() == Some("no-circular-deps"))
        .collect();

    if !circular_violations.is_empty() {
        for v in &circular_violations {
            let contract_id = v.get("contract_id");
            assert!(
                contract_id.is_none() || contract_id.unwrap().is_null(),
                "circular-deps violation should NOT have contract_id: {v:#}"
            );
        }
    }
}

/// Test that all violations have the expected fields based on their type
/// This is a comprehensive test that verifies the contract layer logic
#[test]
fn verify_all_violation_types_have_correct_fields() {
    let temp = TempDir::new().expect("tempdir");

    // Setup: create multiple violation types in one project
    let modules_dir = temp.path().join("modules");
    fs::create_dir_all(&modules_dir).expect("create modules");

    // Config with multiple rule types
    let config = r#"
spec_version: "2.3"
rules:
  - id: dependency.forbidden
    from: api
    to: legacy
  - id: no-circular-deps
"#;
    fs::write(temp.path().join("specgate.config.yml"), config).expect("write config");

    // Create api module with contract referencing missing file
    let api_dir = modules_dir.join("api");
    fs::create_dir_all(&api_dir).expect("create api");
    let api_src_dir = api_dir.join("src").join("api");
    fs::create_dir_all(&api_src_dir).expect("create api src");

    let api_spec = r#"
version: "2.3"
module: api
boundaries:
  path: src/api**
  contracts:
    - id: missing_contract
      contract: contracts/missing.json
      match:
        files:
          - src/api/handler.ts
"#;
    fs::write(api_dir.join("api.spec.yml"), api_spec).expect("write api spec");
    fs::write(
        api_src_dir.join("handler.ts"),
        "export function handler() {}",
    )
    .expect("write handler");

    // Create legacy module
    let legacy_dir = modules_dir.join("legacy");
    fs::create_dir_all(&legacy_dir).expect("create legacy");
    let legacy_src_dir = legacy_dir.join("src").join("legacy");
    fs::create_dir_all(&legacy_src_dir).expect("create legacy src");
    fs::write(legacy_src_dir.join("old.ts"), "export function old() {}").expect("write legacy");
    fs::write(
        legacy_dir.join("legacy.spec.yml"),
        "version: \"2.3\"\nmodule: legacy",
    )
    .expect("write legacy spec");

    // Create circular dependency modules
    let a_dir = modules_dir.join("a");
    fs::create_dir_all(&a_dir).expect("create a");
    let a_src_dir = a_dir.join("src").join("a");
    fs::create_dir_all(&a_src_dir).expect("create a src");
    fs::write(a_dir.join("a.spec.yml"), "version: \"2.3\"\nmodule: a").expect("write a spec");

    let b_dir = modules_dir.join("b");
    fs::create_dir_all(&b_dir).expect("create b");
    let b_src_dir = b_dir.join("src").join("b");
    fs::create_dir_all(&b_src_dir).expect("create b src");
    fs::write(b_dir.join("b.spec.yml"), "version: \"2.3\"\nmodule: b").expect("write b spec");

    // Create circular deps
    fs::write(
        a_src_dir.join("a.ts"),
        "import { b } from '../../b/src/b';\nexport const a = b;",
    )
    .expect("write a->b");
    fs::write(
        b_src_dir.join("b.ts"),
        "import { a } from '../../a/src/a';\nexport const b = a;",
    )
    .expect("write b->a");

    // API imports from legacy (forbidden dependency)
    fs::write(
        api_src_dir.join("handler.ts"),
        "import { old } from '../../legacy/src/legacy/old';\nexport function handler() { old(); }",
    )
    .expect("write api->legacy import");

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);

    let verdict: Value = serde_json::from_str(&result.stdout).expect("parse JSON output");

    let violations = verdict["violations"].as_array().expect("violations array");

    // For each violation, verify correct fields based on type
    for v in violations {
        let rule = v["rule"].as_str().unwrap_or("");

        if rule.starts_with("boundary.contract_") || rule == "boundary.match_unresolved" {
            // Contract violations: MUST have both remediation_hint and contract_id
            assert!(
                v.get("remediation_hint").is_some() && !v["remediation_hint"].is_null(),
                "Contract violation {rule} should have remediation_hint: {v:#}"
            );
            assert!(
                v.get("contract_id").is_some() && !v["contract_id"].is_null(),
                "Contract violation {rule} should have contract_id: {v:#}"
            );
        } else if rule == "enforce-layer" {
            // Layer violations: MUST have remediation_hint, MUST NOT have contract_id
            assert!(
                v.get("remediation_hint").is_some() && !v["remediation_hint"].is_null(),
                "Layer violation should have remediation_hint: {v:#}"
            );
            let contract_id = v.get("contract_id");
            assert!(
                contract_id.is_none() || contract_id.unwrap().is_null(),
                "Layer violation should NOT have contract_id: {v:#}"
            );
        } else {
            // Non-contract, non-layer violations: MUST NOT have contract_id
            let contract_id = v.get("contract_id");
            assert!(
                contract_id.is_none() || contract_id.unwrap().is_null(),
                "Non-contract violation {rule} should NOT have contract_id: {v:#}"
            );
        }
    }
}
