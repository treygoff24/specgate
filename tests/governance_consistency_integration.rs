//! Integration tests for `specgate doctor governance-consistency`.

use std::fs;
use std::path::Path;

use serde_json::Value;
use tempfile::TempDir;

use specgate::cli::{EXIT_CODE_PASS, EXIT_CODE_POLICY_VIOLATIONS, EXIT_CODE_RUNTIME_ERROR, run};

fn write_file(root: &Path, relative_path: &str, content: &str) {
    let path = root.join(relative_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("mkdir parent");
    }
    fs::write(path, content).expect("write file");
}

fn parse_json(source: &str) -> Value {
    serde_json::from_str(source).expect("valid json")
}

fn run_governance_json(root: &Path) -> (i32, Value) {
    let result = run([
        "specgate",
        "doctor",
        "governance-consistency",
        "--project-root",
        root.to_str().unwrap(),
        "--format",
        "json",
    ]);
    let json = parse_json(&result.stdout);
    (result.exit_code, json)
}

fn run_governance_human(root: &Path) -> (i32, String) {
    let result = run([
        "specgate",
        "doctor",
        "governance-consistency",
        "--project-root",
        root.to_str().unwrap(),
        "--format",
        "human",
    ]);
    (result.exit_code, result.stdout)
}

#[test]
fn no_conflicts_returns_pass() {
    let temp = TempDir::new().expect("tempdir");

    write_file(
        temp.path(),
        "specs/alpha.spec.yml",
        "version: \"2.2\"\nmodule: alpha\nboundaries:\n  path: \"src/alpha/**\"\n  allow_imports_from: [\"beta\"]\nconstraints: []\n",
    );
    write_file(
        temp.path(),
        "specs/beta.spec.yml",
        "version: \"2.2\"\nmodule: beta\nboundaries:\n  path: \"src/beta/**\"\nconstraints: []\n",
    );
    write_file(temp.path(), "src/alpha/index.ts", "export const x = 1;\n");
    write_file(temp.path(), "src/beta/index.ts", "export const y = 2;\n");
    write_file(
        temp.path(),
        "specgate.config.yml",
        "spec_dirs:\n  - specs\n",
    );

    let (exit_code, json) = run_governance_json(temp.path());

    assert_eq!(exit_code, EXIT_CODE_PASS);
    assert_eq!(json["conflict_count"].as_u64().unwrap(), 0);
    assert_eq!(json["status"].as_str().unwrap(), "ok");
}

#[test]
fn allow_never_overlap_detected_via_cli() {
    let temp = TempDir::new().expect("tempdir");

    write_file(
        temp.path(),
        "specs/app.spec.yml",
        "version: \"2.2\"\nmodule: app\nboundaries:\n  path: \"src/app/**\"\n  allow_imports_from: [\"core\"]\n  never_imports: [\"core\"]\nconstraints: []\n",
    );
    write_file(temp.path(), "src/app/index.ts", "export const x = 1;\n");
    write_file(
        temp.path(),
        "specgate.config.yml",
        "spec_dirs:\n  - specs\n",
    );

    let (exit_code, json) = run_governance_json(temp.path());

    assert_eq!(exit_code, EXIT_CODE_POLICY_VIOLATIONS);
    let conflict_count = json["conflict_count"].as_u64().unwrap();
    assert!(
        conflict_count >= 1,
        "expected at least 1 conflict, got {conflict_count}"
    );
    let conflicts = json["conflicts"].as_array().unwrap();
    let has_allow_never = conflicts
        .iter()
        .any(|c| c["conflict_type"].as_str().unwrap() == "allow_never_overlap");
    assert!(
        has_allow_never,
        "expected conflict_type allow_never_overlap, got: {conflicts:?}"
    );
}

#[test]
fn private_with_allow_imported_by_detected_via_cli() {
    let temp = TempDir::new().expect("tempdir");

    write_file(
        temp.path(),
        "specs/internal.spec.yml",
        "version: \"2.2\"\nmodule: internal\nboundaries:\n  path: \"src/internal/**\"\n  visibility: private\n  allow_imported_by: [\"friend\"]\nconstraints: []\n",
    );
    write_file(
        temp.path(),
        "src/internal/index.ts",
        "export const x = 1;\n",
    );
    write_file(
        temp.path(),
        "specgate.config.yml",
        "spec_dirs:\n  - specs\n",
    );

    let (exit_code, json) = run_governance_json(temp.path());

    assert_eq!(exit_code, EXIT_CODE_POLICY_VIOLATIONS);
    let conflict_count = json["conflict_count"].as_u64().unwrap();
    assert!(
        conflict_count >= 1,
        "expected at least 1 conflict, got {conflict_count}"
    );
    let conflicts = json["conflicts"].as_array().unwrap();
    let has_conflict = conflicts
        .iter()
        .any(|c| c["conflict_type"].as_str().unwrap() == "private_with_allow_imported_by");
    assert!(
        has_conflict,
        "expected conflict_type private_with_allow_imported_by, got: {conflicts:?}"
    );
}

#[test]
fn cross_module_duplicate_contract_id_detected_via_cli() {
    let temp = TempDir::new().expect("tempdir");

    write_file(
        temp.path(),
        "specs/provider-x.spec.yml",
        "version: \"2.3\"\nmodule: provider-x\nboundaries:\n  path: \"src/provider-x/**\"\n  contracts:\n    - id: shared_api\n      contract: contracts/api.json\n      direction: inbound\n      envelope: required\n      match:\n        files: [\"src/provider-x/**\"]\nconstraints: []\n",
    );
    write_file(
        temp.path(),
        "specs/provider-y.spec.yml",
        "version: \"2.3\"\nmodule: provider-y\nboundaries:\n  path: \"src/provider-y/**\"\n  contracts:\n    - id: shared_api\n      contract: contracts/api2.json\n      direction: outbound\n      envelope: optional\n      match:\n        files: [\"src/provider-y/**\"]\nconstraints: []\n",
    );
    write_file(
        temp.path(),
        "src/provider-x/index.ts",
        "export const x = 1;\n",
    );
    write_file(
        temp.path(),
        "src/provider-y/index.ts",
        "export const y = 2;\n",
    );
    write_file(
        temp.path(),
        "contracts/api.json",
        "{\"$schema\":\"https://json-schema.org/draft/2020-12/schema\",\"type\":\"object\"}\n",
    );
    write_file(
        temp.path(),
        "contracts/api2.json",
        "{\"$schema\":\"https://json-schema.org/draft/2020-12/schema\",\"type\":\"object\"}\n",
    );
    write_file(
        temp.path(),
        "specgate.config.yml",
        "spec_dirs:\n  - specs\n",
    );

    let (exit_code, json) = run_governance_json(temp.path());

    assert_eq!(exit_code, EXIT_CODE_POLICY_VIOLATIONS);
    let conflicts = json["conflicts"].as_array().unwrap();
    let has_dup = conflicts
        .iter()
        .any(|c| c["conflict_type"].as_str().unwrap() == "duplicate_contract_id");
    assert!(
        has_dup,
        "expected conflict_type duplicate_contract_id, got: {conflicts:?}"
    );
}

#[test]
fn json_output_has_schema_version() {
    let temp = TempDir::new().expect("tempdir");

    write_file(
        temp.path(),
        "specs/app.spec.yml",
        "version: \"2.2\"\nmodule: app\nboundaries:\n  path: \"src/app/**\"\nconstraints: []\n",
    );
    write_file(temp.path(), "src/app/index.ts", "export const x = 1;\n");
    write_file(
        temp.path(),
        "specgate.config.yml",
        "spec_dirs:\n  - specs\n",
    );

    let (_, json) = run_governance_json(temp.path());

    assert_eq!(
        json["schema_version"].as_str().unwrap(),
        "1.0",
        "expected schema_version 1.0 in JSON output"
    );
}

#[test]
fn human_output_format_works() {
    let temp = TempDir::new().expect("tempdir");

    write_file(
        temp.path(),
        "specs/app.spec.yml",
        "version: \"2.2\"\nmodule: app\nboundaries:\n  path: \"src/app/**\"\n  allow_imports_from: [\"core\"]\n  never_imports: [\"core\"]\nconstraints: []\n",
    );
    write_file(temp.path(), "src/app/index.ts", "export const x = 1;\n");
    write_file(
        temp.path(),
        "specgate.config.yml",
        "spec_dirs:\n  - specs\n",
    );

    let (exit_code, output) = run_governance_human(temp.path());

    assert_eq!(exit_code, EXIT_CODE_POLICY_VIOLATIONS);
    assert!(
        output.contains("Governance Consistency Report"),
        "expected 'Governance Consistency Report' in output, got: {output}"
    );
    assert!(
        output.contains("allow_never_overlap") || output.contains("allow_imports_from"),
        "expected conflict description text in human output, got: {output}"
    );
}

#[test]
fn validation_errors_return_runtime_error() {
    let temp = TempDir::new().expect("tempdir");

    // version "2.0" is unsupported and triggers a validation error
    write_file(
        temp.path(),
        "specs/app.spec.yml",
        "version: \"2.0\"\nmodule: app\nboundaries:\n  path: \"src/app/**\"\nconstraints: []\n",
    );
    write_file(temp.path(), "src/app/index.ts", "export const x = 1;\n");
    write_file(
        temp.path(),
        "specgate.config.yml",
        "spec_dirs:\n  - specs\n",
    );

    let result = run([
        "specgate",
        "doctor",
        "governance-consistency",
        "--project-root",
        temp.path().to_str().unwrap(),
        "--format",
        "json",
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_RUNTIME_ERROR);
}
