//! Monorepo integration tests for multi-tsconfig check.
//!
//! Tests workspace-aware resolution, per-package tsconfig aliasing,
//! boundary violation detection, workspace_packages verdict field,
//! deterministic output, and custom tsconfig_filename support.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use tempfile::TempDir;

use specgate::cli::{EXIT_CODE_PASS, EXIT_CODE_POLICY_VIOLATIONS, EXIT_CODE_RUNTIME_ERROR, run};

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("monorepo-multi-tsconfig")
}

fn parse_json(raw: &str) -> Value {
    serde_json::from_str(raw).expect("valid json")
}

fn write_file(root: &Path, relative_path: &str, content: &str) {
    let path = root.join(relative_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("mkdir parent");
    }
    fs::write(path, content).expect("write file");
}

fn run_check(project_root: &Path) -> (specgate::cli::CliRunResult, Value) {
    let result = run([
        "specgate",
        "check",
        "--project-root",
        project_root.to_str().expect("utf8 path"),
        "--no-baseline",
    ]);
    let json = parse_json(&result.stdout);
    (result, json)
}

/// Copy the monorepo fixture to a temp dir, optionally excluding specific files.
fn copy_fixture_excluding(exclude_files: &[&str]) -> TempDir {
    let temp = TempDir::new().expect("tempdir");
    let src = fixture_root();
    copy_dir_recursive(&src, temp.path(), &src, exclude_files);
    temp
}

fn copy_dir_recursive(src: &Path, dst: &Path, root: &Path, exclude_files: &[&str]) {
    for entry in fs::read_dir(src).expect("read dir") {
        let entry = entry.expect("dir entry");
        let src_path = entry.path();
        let relative = src_path.strip_prefix(root).expect("strip prefix");
        let dst_path = dst.join(relative);

        if src_path.is_dir() {
            fs::create_dir_all(&dst_path).expect("mkdir");
            copy_dir_recursive(&src_path, dst, root, exclude_files);
        } else {
            let rel_str = relative.to_str().expect("utf8");
            let normalized = rel_str.replace('\\', "/");
            if exclude_files.iter().any(|exc| normalized == *exc) {
                continue;
            }
            if let Some(parent) = dst_path.parent() {
                fs::create_dir_all(parent).expect("mkdir parent");
            }
            fs::copy(&src_path, &dst_path).expect("copy file");
        }
    }
}

#[test]
fn monorepo_check_passes_with_valid_cross_package_imports() {
    let temp = copy_fixture_excluding(&["packages/shared/src/sneaky.ts"]);

    let (result, verdict) = run_check(temp.path());

    assert_eq!(
        result.exit_code,
        EXIT_CODE_PASS,
        "valid cross-package imports should pass; stdout={stdout}, stderr={stderr}",
        stdout = result.stdout,
        stderr = result.stderr
    );
    assert_eq!(verdict["status"], "pass");
    assert_eq!(verdict["summary"]["total_violations"], 0);
}

#[test]
fn monorepo_check_detects_boundary_violation() {
    // Create a monorepo fixture where web has allow_imports_from: [shared]
    // and shared has allow_imports_from: [] (empty = deny all cross-module).
    // Then shared importing from web is a violation.
    let temp = TempDir::new().expect("tempdir");

    write_file(
        temp.path(),
        "package.json",
        r#"{"name": "test-monorepo", "private": true, "workspaces": ["packages/*"]}"#,
    );
    write_file(
        temp.path(),
        "tsconfig.json",
        r#"{"files":[],"references":[{"path":"packages/shared"},{"path":"packages/web"}]}"#,
    );
    write_file(
        temp.path(),
        "specgate.config.yml",
        "spec_dirs:\n  - packages/shared/modules\n  - packages/web/modules\nexclude: []\ntest_patterns: []\n",
    );

    write_file(
        temp.path(),
        "packages/shared/package.json",
        r#"{"name": "@test/shared"}"#,
    );
    write_file(
        temp.path(),
        "packages/shared/tsconfig.json",
        r#"{"compilerOptions":{"composite":true,"baseUrl":".","paths":{"@shared/*":["src/*"]}},"include":["src"]}"#,
    );
    write_file(
        temp.path(),
        "packages/shared/modules/shared.spec.yml",
        r#"version: "2.2"
module: shared
boundaries:
  path: packages/shared/src/**/*
  allow_imports_from: []
constraints: []
"#,
    );
    write_file(
        temp.path(),
        "packages/shared/src/index.ts",
        "export function greet(): string { return 'hello'; }\n",
    );
    write_file(
        temp.path(),
        "packages/shared/src/sneaky.ts",
        "import { helper } from '../../web/src/helper';\nexport const s = helper;\n",
    );

    write_file(
        temp.path(),
        "packages/web/package.json",
        r#"{"name": "@test/web"}"#,
    );
    write_file(
        temp.path(),
        "packages/web/tsconfig.json",
        r#"{"compilerOptions":{"composite":true,"baseUrl":".","paths":{"@web/*":["src/*"],"@shared/*":["../shared/src/*"]}},"include":["src"],"references":[{"path":"../shared"}]}"#,
    );
    write_file(
        temp.path(),
        "packages/web/modules/web.spec.yml",
        r#"version: "2.2"
module: web
boundaries:
  path: packages/web/src/**/*
  allow_imports_from:
    - shared
constraints: []
"#,
    );
    write_file(
        temp.path(),
        "packages/web/src/helper.ts",
        "export function helper(): string { return 'help'; }\n",
    );
    write_file(
        temp.path(),
        "packages/web/src/index.ts",
        "import { greet } from '@shared/greet';\nexport const main = greet;\n",
    );

    let (result, verdict) = run_check(temp.path());

    assert_eq!(
        result.exit_code,
        EXIT_CODE_POLICY_VIOLATIONS,
        "shared->web import should be a violation; stdout={stdout}, stderr={stderr}",
        stdout = result.stdout,
        stderr = result.stderr
    );
    assert_eq!(verdict["status"], "fail");

    let violations = verdict["violations"].as_array().expect("violations array");
    let shared_violation = violations.iter().find(|v| {
        v["from_module"].as_str() == Some("shared")
            && v["rule"].as_str() == Some("boundary.allow_imports_from")
    });
    assert!(
        shared_violation.is_some(),
        "expected boundary.allow_imports_from violation from shared module; violations={violations:?}"
    );
}

#[test]
fn monorepo_check_resolves_per_package_aliases_correctly() {
    // Verify per-package tsconfig alias resolution by checking that
    // web/index.ts importing @shared/greet resolves correctly via
    // web's tsconfig (which maps @shared/* -> ../shared/src/*).
    // If it resolves, no violations occur from web importing shared
    // because web has allow_imports_from: [shared].
    let temp = copy_fixture_excluding(&["packages/shared/src/sneaky.ts"]);

    let (result, verdict) = run_check(temp.path());

    assert_eq!(
        result.exit_code,
        EXIT_CODE_PASS,
        "per-package aliases should resolve correctly; stdout={stdout}, stderr={stderr}",
        stdout = result.stdout,
        stderr = result.stderr
    );
    assert_eq!(verdict["status"], "pass");

    let violations = verdict["violations"].as_array().expect("violations array");
    assert_eq!(
        violations.len(),
        0,
        "no violations expected; violations={violations:?}"
    );
    assert_eq!(verdict["summary"]["total_violations"], 0);

    // Verify both packages were discovered as workspace packages, proving the
    // monorepo was analyzed end-to-end and both tsconfigs were loaded.
    let workspace_packages = verdict["workspace_packages"]
        .as_array()
        .expect("workspace_packages should be present in a monorepo verdict");
    let pkg_names: Vec<&str> = workspace_packages
        .iter()
        .filter_map(|pkg| pkg["name"].as_str())
        .collect();
    assert!(
        pkg_names.contains(&"shared"),
        "workspace_packages should include 'shared'; found={pkg_names:?}"
    );
    assert!(
        pkg_names.contains(&"web"),
        "workspace_packages should include 'web'; found={pkg_names:?}"
    );

    // Each package must expose a tsconfig path, confirming per-package tsconfig
    // loading (not a single root tsconfig fallback).
    for pkg in workspace_packages {
        let pkg_name = pkg["name"].as_str().unwrap_or("<unknown>");
        assert!(
            pkg["tsconfig"].as_str().is_some(),
            "workspace package '{pkg_name}' should have a tsconfig path, \
             confirming per-package tsconfig was loaded"
        );
    }
}

#[test]
fn monorepo_check_verdict_includes_workspace_packages() {
    let temp = copy_fixture_excluding(&["packages/shared/src/sneaky.ts"]);

    let (_, verdict) = run_check(temp.path());

    let workspace_packages = verdict["workspace_packages"]
        .as_array()
        .expect("workspace_packages should be present in verdict");

    assert_eq!(
        workspace_packages.len(),
        2,
        "should have 2 workspace packages (shared, web); workspace_packages={workspace_packages:?}"
    );

    let names: Vec<&str> = workspace_packages
        .iter()
        .filter_map(|pkg| pkg["name"].as_str())
        .collect();
    assert!(
        names.contains(&"shared"),
        "workspace_packages should include shared"
    );
    assert!(
        names.contains(&"web"),
        "workspace_packages should include web"
    );

    for pkg in workspace_packages {
        assert!(
            pkg["path"].as_str().is_some(),
            "each workspace package should have a path field"
        );
        assert!(
            pkg.get("tsconfig").is_some(),
            "each workspace package should have a tsconfig field"
        );
    }
}

#[test]
fn monorepo_check_deterministic_across_runs() {
    // This test validates deterministic output regardless of pass/fail status.
    // It intentionally uses fixture_root() directly (not a temp copy), which
    // means sneaky.ts is present and causes a boundary violation. The goal is
    // to prove that determinism holds even when violations are present — not to
    // assert what the output contains, only that it is byte-identical across runs.
    let root = fixture_root();
    let root_str = root.to_str().expect("utf8");

    let results: Vec<String> = (0..3)
        .map(|_| {
            let result = run([
                "specgate",
                "check",
                "--project-root",
                root_str,
                "--no-baseline",
            ]);
            result.stdout
        })
        .collect();

    assert_eq!(
        results[0], results[1],
        "run 1 and 2 should produce identical output"
    );
    assert_eq!(
        results[1], results[2],
        "run 2 and 3 should produce identical output"
    );
}

#[test]
fn check_respects_tsconfig_filename_config() {
    let temp = TempDir::new().expect("tempdir");

    write_file(
        temp.path(),
        "specgate.config.yml",
        r#"spec_dirs:
  - modules
exclude: []
test_patterns: []
tsconfig_filename: "tsconfig.build.json"
"#,
    );
    write_file(
        temp.path(),
        "modules/app.spec.yml",
        r#"version: "2.2"
module: app
boundaries:
  path: src/app/**/*
constraints: []
"#,
    );
    write_file(
        temp.path(),
        "modules/lib.spec.yml",
        r#"version: "2.2"
module: lib
boundaries:
  path: src/lib/**/*
constraints: []
"#,
    );

    write_file(
        temp.path(),
        "tsconfig.build.json",
        r#"{
  "compilerOptions": {
    "baseUrl": ".",
    "paths": {
      "@lib/*": ["src/lib/*"]
    }
  }
}"#,
    );

    write_file(
        temp.path(),
        "src/app/main.ts",
        "import { util } from '@lib/util';\nexport const app = util;\n",
    );
    write_file(temp.path(), "src/lib/util.ts", "export const util = 1;\n");

    let (result, verdict) = run_check(temp.path());

    assert_eq!(
        result.exit_code,
        EXIT_CODE_PASS,
        "aliases via tsconfig.build.json should resolve correctly; stdout={stdout}, stderr={stderr}",
        stdout = result.stdout,
        stderr = result.stderr
    );
    assert_eq!(verdict["status"], "pass");

    // Violations array must be present and empty.
    let violations = verdict["violations"].as_array().expect("violations array");
    assert_eq!(
        violations.len(),
        0,
        "no violations expected; violations={violations:?}"
    );
    assert_eq!(verdict["summary"]["total_violations"], 0);

    // Confirm tsconfig.json was never created — resolution relied solely on
    // the custom tsconfig.build.json.  If the resolver had fallen back to a
    // missing tsconfig.json the alias would be unknown and app.ts would
    // import an unresolved specifier; any boundary check on that import
    // would either pass vacuously or fail — the only way we get 0 violations
    // with 0 unresolved noise is if tsconfig.build.json was loaded correctly.
    assert!(
        !temp.path().join("tsconfig.json").exists(),
        "tsconfig.json should not exist; resolution must use tsconfig.build.json"
    );
}

#[test]
fn check_rejects_invalid_tsconfig_filename_in_config() {
    let temp = TempDir::new().expect("tempdir");

    // Use a tsconfig_filename containing a path separator, which is invalid.
    write_file(
        temp.path(),
        "specgate.config.yml",
        "tsconfig_filename: \"sub/tsconfig.json\"\n",
    );

    // Minimal .spec.yml so the CLI gets past spec discovery.
    write_file(
        temp.path(),
        "app.spec.yml",
        "version: \"2.2\"\nmodule: app\nboundaries:\n  path: src/**/*\nconstraints: []\n",
    );

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8 path"),
        "--no-baseline",
    ]);

    assert_eq!(
        result.exit_code,
        EXIT_CODE_RUNTIME_ERROR,
        "invalid tsconfig_filename should produce exit code 2; stdout={stdout}, stderr={stderr}",
        stdout = result.stdout,
        stderr = result.stderr
    );
    // Config errors are rendered as JSON to stdout (not stderr).
    assert!(
        result.stdout.contains("must be a plain filename"),
        "stdout should mention 'must be a plain filename'; stdout={}",
        result.stdout
    );
}

#[test]
fn check_reports_malformed_workspace_config_as_runtime_error() {
    let temp = TempDir::new().expect("tempdir");

    write_file(
        temp.path(),
        "package.json",
        r#"{"name":"broken-monorepo","private":true,"workspaces":17}"#,
    );
    write_file(
        temp.path(),
        "specgate.config.yml",
        "spec_dirs:\n  - modules\nexclude: []\ntest_patterns: []\n",
    );
    write_file(
        temp.path(),
        "modules/app.spec.yml",
        "version: \"2.2\"\nmodule: app\nboundaries:\n  path: src/**/*\nconstraints: []\n",
    );
    write_file(temp.path(), "src/index.ts", "export const app = 1;\n");

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8 path"),
        "--no-baseline",
    ]);

    assert_eq!(
        result.exit_code,
        EXIT_CODE_RUNTIME_ERROR,
        "malformed workspace config should fail closed; stdout={stdout}, stderr={stderr}",
        stdout = result.stdout,
        stderr = result.stderr
    );
    assert!(
        result.stdout.contains("package.json"),
        "stdout should identify the malformed workspace source; stdout={}",
        result.stdout
    );
    assert!(
        result.stdout.contains("workspaces"),
        "stdout should mention the malformed workspaces field; stdout={}",
        result.stdout
    );
}

#[test]
fn check_allows_valid_workspace_config_with_no_matching_packages() {
    let temp = TempDir::new().expect("tempdir");

    write_file(
        temp.path(),
        "package.json",
        r#"{"name":"empty-monorepo","private":true,"workspaces":["packages/*"]}"#,
    );
    write_file(
        temp.path(),
        "specgate.config.yml",
        "spec_dirs:\n  - modules\nexclude: []\ntest_patterns: []\n",
    );
    write_file(
        temp.path(),
        "modules/app.spec.yml",
        "version: \"2.2\"\nmodule: app\nboundaries:\n  path: src/**/*\nconstraints: []\n",
    );
    write_file(temp.path(), "src/index.ts", "export const app = 1;\n");

    let (result, verdict) = run_check(temp.path());

    assert_eq!(
        result.exit_code,
        EXIT_CODE_PASS,
        "valid but empty workspace discovery should remain a success; stdout={stdout}, stderr={stderr}",
        stdout = result.stdout,
        stderr = result.stderr
    );
    assert_eq!(verdict["status"], "pass");
    assert!(
        verdict.get("workspace_packages").is_none() || verdict["workspace_packages"].is_null(),
        "workspace_packages should stay absent for legitimate empty discovery; verdict={verdict:?}"
    );
}
