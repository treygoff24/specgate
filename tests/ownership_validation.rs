//! Integration tests for `specgate doctor ownership`.

use std::fs;
use std::path::Path;

use serde_json::Value;
use tempfile::TempDir;

use specgate::cli::{EXIT_CODE_PASS, EXIT_CODE_POLICY_VIOLATIONS, run};

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

fn run_ownership_json(root: &Path) -> (i32, Value) {
    let result = run([
        "specgate",
        "doctor",
        "ownership",
        "--project-root",
        root.to_str().unwrap(),
        "--format",
        "json",
    ]);
    let json = parse_json(&result.stdout);
    (result.exit_code, json)
}

/// Build a fixture with:
/// - api/core spec covering src/api/**
/// - ui/shared spec covering src/ui/**
/// - src/api/index.ts (claimed by api/core)
/// - src/ui/button.tsx (claimed by ui/shared)
/// - src/shared/utils.ts (claimed by both → overlap)
/// - src/legacy/old.ts (claimed by nobody → unclaimed)
/// - old-module spec covering src/old/** (no matching files → orphaned)
fn setup_mixed_fixture(root: &Path) {
    // Specs
    write_file(
        root,
        "specs/api-core.spec.yml",
        "version: \"2.2\"\nmodule: api/core\nboundaries:\n  path: \"src/api/**\"\nconstraints: []\n",
    );
    write_file(
        root,
        "specs/ui-shared.spec.yml",
        "version: \"2.2\"\nmodule: ui/shared\nboundaries:\n  path: \"src/ui/**\"\nconstraints: []\n",
    );
    write_file(
        root,
        "specs/shared-a.spec.yml",
        "version: \"2.2\"\nmodule: shared/a\nboundaries:\n  path: \"src/shared/**\"\nconstraints: []\n",
    );
    write_file(
        root,
        "specs/shared-b.spec.yml",
        "version: \"2.2\"\nmodule: shared/b\nboundaries:\n  path: \"src/shared/**\"\nconstraints: []\n",
    );
    write_file(
        root,
        "specs/old-module.spec.yml",
        "version: \"2.2\"\nmodule: old-module\nboundaries:\n  path: \"src/old/**\"\nconstraints: []\n",
    );

    // Source files
    write_file(root, "src/api/index.ts", "export const x = 1;\n");
    write_file(root, "src/ui/button.tsx", "export const Button = () => null;\n");
    write_file(root, "src/shared/utils.ts", "export const util = () => {};\n");
    write_file(root, "src/legacy/old.ts", "export const old = 1;\n");

    // Config pointing at specs dir
    write_file(
        root,
        "specgate.config.yml",
        "spec_dirs:\n  - specs\n",
    );
}

#[test]
fn test_ownership_reports_unclaimed_files() {
    let temp = TempDir::new().expect("tempdir");
    setup_mixed_fixture(temp.path());

    let (exit_code, json) = run_ownership_json(temp.path());

    assert_eq!(exit_code, EXIT_CODE_PASS); // no strict_ownership
    let unclaimed = &json["report"]["unclaimed_files"];
    let unclaimed_list: Vec<&str> = unclaimed
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(
        unclaimed_list.contains(&"src/legacy/old.ts"),
        "expected src/legacy/old.ts in unclaimed, got: {unclaimed_list:?}"
    );
}

#[test]
fn test_ownership_reports_overlapping_files() {
    let temp = TempDir::new().expect("tempdir");
    setup_mixed_fixture(temp.path());

    let (exit_code, json) = run_ownership_json(temp.path());

    assert_eq!(exit_code, EXIT_CODE_PASS);
    let overlapping = json["report"]["overlapping_files"].as_array().unwrap();
    assert!(
        !overlapping.is_empty(),
        "expected overlapping files for src/shared/utils.ts"
    );
    let overlap = overlapping.iter().find(|e| {
        e["file"].as_str().unwrap() == "src/shared/utils.ts"
    });
    assert!(overlap.is_some(), "expected src/shared/utils.ts in overlapping_files");
    let claimants = overlap.unwrap()["claimed_by"].as_array().unwrap();
    let claimant_ids: Vec<&str> = claimants.iter().map(|v| v.as_str().unwrap()).collect();
    assert!(claimant_ids.contains(&"shared/a"));
    assert!(claimant_ids.contains(&"shared/b"));
}

#[test]
fn test_ownership_reports_orphaned_specs() {
    let temp = TempDir::new().expect("tempdir");
    setup_mixed_fixture(temp.path());

    let (exit_code, json) = run_ownership_json(temp.path());

    assert_eq!(exit_code, EXIT_CODE_PASS);
    let orphaned = json["report"]["orphaned_specs"].as_array().unwrap();
    let orphaned_ids: Vec<&str> = orphaned
        .iter()
        .map(|e| e["module_id"].as_str().unwrap())
        .collect();
    assert!(
        orphaned_ids.contains(&"old-module"),
        "expected old-module in orphaned_specs, got: {orphaned_ids:?}"
    );
}

#[test]
fn test_strict_ownership_exits_nonzero_when_findings() {
    let temp = TempDir::new().expect("tempdir");
    setup_mixed_fixture(temp.path());

    // Override config with strict_ownership: true
    write_file(
        temp.path(),
        "specgate.config.yml",
        "spec_dirs:\n  - specs\nstrict_ownership: true\n",
    );

    let (exit_code, _) = run_ownership_json(temp.path());
    assert_eq!(exit_code, EXIT_CODE_POLICY_VIOLATIONS);
}

#[test]
fn test_clean_ownership_exits_zero_with_strict() {
    let temp = TempDir::new().expect("tempdir");

    // A single spec claiming exactly one file, nothing else
    write_file(
        temp.path(),
        "specs/api.spec.yml",
        "version: \"2.2\"\nmodule: api\nboundaries:\n  path: \"src/**\"\nconstraints: []\n",
    );
    write_file(temp.path(), "src/index.ts", "export const x = 1;\n");
    write_file(
        temp.path(),
        "specgate.config.yml",
        "spec_dirs:\n  - specs\nstrict_ownership: true\n",
    );

    let (exit_code, json) = run_ownership_json(temp.path());
    assert_eq!(exit_code, EXIT_CODE_PASS);
    assert_eq!(json["status"].as_str().unwrap(), "ok");
}

#[test]
fn test_human_output_format_contains_summary() {
    let temp = TempDir::new().expect("tempdir");
    setup_mixed_fixture(temp.path());

    let result = run([
        "specgate",
        "doctor",
        "ownership",
        "--project-root",
        temp.path().to_str().unwrap(),
        "--format",
        "human",
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_PASS);
    assert!(result.stdout.contains("Ownership Report:"));
    assert!(result.stdout.contains("Source files:"));
}