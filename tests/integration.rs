//! Phase 3 Integration Tests
//!
//! Comprehensive integration tests for check, init, validate commands
//! with focus on diff mode, determinism, and exit code semantics.

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

fn create_basic_project(root: &Path) {
    write_file(
        root,
        "modules/app.spec.yml",
        "version: \"2.2\"\nmodule: app\nboundaries:\n  path: src/app/**/*\nconstraints: []\n",
    );
    write_file(
        root,
        "modules/core.spec.yml",
        "version: \"2.2\"\nmodule: core\nboundaries:\n  path: src/core/**/*\nconstraints: []\n",
    );
    write_file(
        root,
        "specgate.config.yml",
        "spec_dirs:\n  - modules\nexclude: []\ntest_patterns: []\n",
    );
    write_file(root, "src/app/main.ts", "export const app = 1;\n");
    write_file(root, "src/core/index.ts", "export const core = 1;\n");
}

fn create_project_with_violation(root: &Path) {
    create_basic_project(root);
    write_file(
        root,
        "modules/app.spec.yml",
        "version: \"2.2\"\nmodule: app\nboundaries:\n  path: src/app/**/*\n  never_imports:\n    - core\nconstraints: []\n",
    );
    write_file(
        root,
        "src/app/main.ts",
        "import { core } from '../core/index';\nexport const app = core;\n",
    );
}

// ============================================================================
// INIT COMMAND TESTS
// ============================================================================

#[test]
fn init_creates_config_and_spec_files() {
    let temp = TempDir::new().expect("tempdir");

    let result = run([
        "specgate",
        "init",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_PASS);
    assert!(temp.path().join("specgate.config.yml").exists());
    assert!(temp.path().join("modules/app.spec.yml").exists());
}

#[test]
fn init_respects_custom_module_name() {
    let temp = TempDir::new().expect("tempdir");

    let result = run([
        "specgate",
        "init",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--module",
        "my-module",
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_PASS);
    assert!(temp.path().join("modules/my-module.spec.yml").exists());
}

#[test]
fn init_does_not_overwrite_without_force() {
    let temp = TempDir::new().expect("tempdir");

    // First init
    run([
        "specgate",
        "init",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);

    // Modify a file
    let config_path = temp.path().join("specgate.config.yml");
    fs::write(&config_path, "modified").expect("write config");

    // Second init without force
    let result = run([
        "specgate",
        "init",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_PASS);
    assert_eq!(fs::read_to_string(&config_path).expect("read"), "modified");
    assert!(result.stdout.contains("\"skipped_existing\""));
}

#[test]
fn init_force_overwrites_files() {
    let temp = TempDir::new().expect("tempdir");

    // First init
    run([
        "specgate",
        "init",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);

    // Modify a file
    let config_path = temp.path().join("specgate.config.yml");
    fs::write(&config_path, "modified").expect("write config");

    // Second init with force
    let result = run([
        "specgate",
        "init",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--force",
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_PASS);
    let content = fs::read_to_string(&config_path).expect("read");
    assert_ne!(content, "modified");
    assert!(result.stdout.contains("\"created\""));
}

// ============================================================================
// VALIDATE COMMAND TESTS
// ============================================================================

#[test]
fn validate_passes_with_valid_specs() {
    let temp = TempDir::new().expect("tempdir");
    create_basic_project(temp.path());

    let result = run([
        "specgate",
        "validate",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_PASS);
    let output = parse_json(&result.stdout);
    assert_eq!(output["status"], "ok");
    assert_eq!(output["error_count"], 0);
}

#[test]
fn validate_fails_with_invalid_spec_version() {
    let temp = TempDir::new().expect("tempdir");
    create_basic_project(temp.path());

    // Write invalid spec
    write_file(
        temp.path(),
        "modules/invalid.spec.yml",
        "version: \"1.0\"\nmodule: invalid\nconstraints: []\n",
    );

    let result = run([
        "specgate",
        "validate",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_RUNTIME_ERROR);
    let output = parse_json(&result.stdout);
    assert_eq!(output["status"], "error");
    assert!(output["error_count"].as_u64().unwrap() > 0);
}

#[test]
fn validate_reports_multiple_issues() {
    let temp = TempDir::new().expect("tempdir");

    write_file(
        temp.path(),
        "specgate.config.yml",
        "spec_dirs:\n  - modules\nexclude: []\ntest_patterns: []\n",
    );

    // Multiple invalid specs
    write_file(
        temp.path(),
        "modules/bad1.spec.yml",
        "version: \"1.0\"\nmodule: bad1\nconstraints: []\n",
    );
    write_file(
        temp.path(),
        "modules/bad2.spec.yml",
        "version: \"1.0\"\nmodule: bad2\nconstraints: []\n",
    );

    let result = run([
        "specgate",
        "validate",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_RUNTIME_ERROR);
    let output = parse_json(&result.stdout);
    assert!(output["error_count"].as_u64().unwrap() >= 2);
}

// ============================================================================
// CHECK COMMAND DETERMINISM TESTS
// ============================================================================

#[test]
fn check_output_is_deterministic_across_runs() {
    let temp = TempDir::new().expect("tempdir");
    create_project_with_violation(temp.path());

    let result1 = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--no-baseline",
    ]);

    let result2 = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--no-baseline",
    ]);

    assert_eq!(result1.exit_code, EXIT_CODE_POLICY_VIOLATIONS);
    assert_eq!(result1.stdout, result2.stdout);
}

#[test]
fn check_violation_order_is_deterministic() {
    let temp = TempDir::new().expect("tempdir");
    create_basic_project(temp.path());

    // Create multiple violations
    write_file(
        temp.path(),
        "modules/app.spec.yml",
        "version: \"2.2\"\nmodule: app\nboundaries:\n  path: src/app/**/*\n  never_imports:\n    - core\nconstraints: []\n",
    );
    write_file(
        temp.path(),
        "modules/core.spec.yml",
        "version: \"2.2\"\nmodule: core\nboundaries:\n  path: src/core/**/*\n  never_imports:\n    - app\nconstraints: []\n",
    );
    write_file(
        temp.path(),
        "src/app/main.ts",
        "import { core } from '../core/index';\nexport const app = core;\n",
    );
    write_file(
        temp.path(),
        "src/core/index.ts",
        "import { app } from '../app/main';\nexport const core = app;\n",
    );

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--no-baseline",
    ]);

    let output = parse_json(&result.stdout);
    let violations = output["violations"].as_array().expect("violations array");

    // Verify deterministic ordering
    for i in 1..violations.len() {
        let prev = &violations[i - 1];
        let curr = &violations[i];

        // Should be sorted by from_file, then line, then column
        let prev_file = prev["from_file"].as_str().expect("from_file string");
        let curr_file = curr["from_file"].as_str().expect("from_file string");
        assert!(prev_file <= curr_file);
    }
}

#[test]
fn check_verdict_includes_schema_version() {
    let temp = TempDir::new().expect("tempdir");
    create_basic_project(temp.path());

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--no-baseline",
    ]);

    let output = parse_json(&result.stdout);
    assert_eq!(output["schema_version"], "2.2");
}

// ============================================================================
// CHECK COMMAND EXIT CODE SEMANTICS
// ============================================================================

#[test]
fn check_exits_zero_on_pass() {
    let temp = TempDir::new().expect("tempdir");
    create_basic_project(temp.path());

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--no-baseline",
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_PASS);
    let output = parse_json(&result.stdout);
    assert_eq!(output["status"], "pass");
}

#[test]
fn check_exits_one_on_policy_violation() {
    let temp = TempDir::new().expect("tempdir");
    create_project_with_violation(temp.path());

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--no-baseline",
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_POLICY_VIOLATIONS);
    let output = parse_json(&result.stdout);
    assert_eq!(output["status"], "fail");
}

#[test]
fn check_exits_two_on_config_error() {
    let temp = TempDir::new().expect("tempdir");

    write_file(
        temp.path(),
        "specgate.config.yml",
        "spec_dirs:\n  - modules\nexclude:\n  - \"[\"\ntest_patterns: []\n",
    );

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--no-baseline",
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_RUNTIME_ERROR);
    let output = parse_json(&result.stdout);
    assert_eq!(output["status"], "error");
}

#[test]
fn check_warning_only_exits_zero() {
    let temp = TempDir::new().expect("tempdir");
    create_project_with_violation(temp.path());

    // Override constraint to warning
    write_file(
        temp.path(),
        "modules/app.spec.yml",
        "version: \"2.2\"\nmodule: app\nboundaries:\n  path: src/app/**/*\n  never_imports:\n    - core\nconstraints:\n  - rule: boundary.never_imports\n    severity: warning\n",
    );

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--no-baseline",
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_PASS);
    let output = parse_json(&result.stdout);
    assert_eq!(output["status"], "pass");
    assert!(
        output["summary"]["new_warning_violations"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert_eq!(output["summary"]["new_error_violations"], 0);
}

// ============================================================================
// BASELINE CLASSIFICATION TESTS
// ============================================================================

#[test]
fn baseline_classifies_existing_violations() {
    let temp = TempDir::new().expect("tempdir");
    create_project_with_violation(temp.path());

    // Generate baseline
    let baseline_result = run([
        "specgate",
        "baseline",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);
    assert_eq!(baseline_result.exit_code, EXIT_CODE_PASS);

    // Check with baseline
    let check_result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);

    assert_eq!(check_result.exit_code, EXIT_CODE_PASS);
    let output = parse_json(&check_result.stdout);
    assert!(output["summary"]["baseline_violations"].as_u64().unwrap() > 0);
    assert_eq!(output["summary"]["new_violations"], 0);
}

#[test]
fn baseline_detects_new_violations() {
    let temp = TempDir::new().expect("tempdir");
    create_project_with_violation(temp.path());

    // Generate baseline for existing violation
    run([
        "specgate",
        "baseline",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);

    // Add new violation
    write_file(
        temp.path(),
        "src/app/another.ts",
        "import { core } from '../core/index';\nexport const another = core;\n",
    );

    // Check with baseline
    let check_result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);

    assert_eq!(check_result.exit_code, EXIT_CODE_POLICY_VIOLATIONS);
    let output = parse_json(&check_result.stdout);
    assert_eq!(output["summary"]["new_violations"], 1);
}

// ============================================================================
// METRICS MODE TESTS
// ============================================================================

#[test]
fn check_metrics_includes_timing() {
    let temp = TempDir::new().expect("tempdir");
    create_basic_project(temp.path());

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--output-mode",
        "metrics",
        "--no-baseline",
    ]);

    let output = parse_json(&result.stdout);
    assert!(output["metrics"].is_object());
    assert!(output["metrics"]["timings_ms"].is_object());
    assert!(output["metrics"]["total_ms"].as_u64().unwrap() > 0);
}

#[test]
fn check_deterministic_omits_metrics() {
    let temp = TempDir::new().expect("tempdir");
    create_basic_project(temp.path());

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--no-baseline",
    ]);

    let output = parse_json(&result.stdout);
    assert!(output["metrics"].is_null());
}

// ============================================================================
// DIFF MODE TESTS
// ============================================================================

#[test]
fn check_diff_outputs_git_style_format() {
    let temp = TempDir::new().expect("tempdir");
    create_project_with_violation(temp.path());

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--diff",
        "--no-baseline",
    ]);

    // Diff mode should output plain text, not JSON
    assert_eq!(result.exit_code, EXIT_CODE_POLICY_VIOLATIONS);
    assert!(!result.stdout.starts_with('{'));
    assert!(result.stdout.starts_with('+'));
}

#[test]
fn check_diff_uses_tab_separated_format() {
    let temp = TempDir::new().expect("tempdir");
    create_project_with_violation(temp.path());

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--diff",
        "--no-baseline",
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_POLICY_VIOLATIONS);

    // Each line should have tab-separated fields
    let lines: Vec<&str> = result.stdout.lines().collect();
    let violation_lines: Vec<&&str> = lines.iter().filter(|l| l.starts_with('+')).collect();
    assert!(!violation_lines.is_empty());

    // Check format: +path\tline\tcol\tseverity\trule\tmodule_from\tmodule_to\tto_path\tmessage
    for line in &violation_lines {
        let parts: Vec<&str> = line.split('\t').collect();
        assert!(parts.len() >= 9, "Expected at least 9 tab-separated fields, got {}", parts.len());
        assert!(parts[0].starts_with('+'));
    }
}

#[test]
fn check_diff_new_only_filters_to_new_violations() {
    let temp = TempDir::new().expect("tempdir");
    create_project_with_violation(temp.path());

    // Generate baseline for existing violation
    run([
        "specgate",
        "baseline",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);

    // Add new violation
    write_file(
        temp.path(),
        "src/app/another.ts",
        "import { core } from '../core/index';\nexport const another = core;\n",
    );

    // Check with diff-new-only
    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--diff",
        "--diff-new-only",
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_POLICY_VIOLATIONS);

    // Should only show new violations (prefixed with +)
    let lines: Vec<&str> = result.stdout.lines().collect();
    let new_violation_lines: Vec<&&str> = lines.iter().filter(|l| l.starts_with('+')).collect();
    let baseline_lines: Vec<&&str> = lines.iter().filter(|l| l.starts_with(' ')).collect();

    // Should have exactly 1 new violation
    assert_eq!(new_violation_lines.len(), 1);
    // Should have no baseline violations shown in new-only mode
    assert_eq!(baseline_lines.len(), 0);
}

#[test]
fn check_diff_shows_baseline_with_space_prefix() {
    let temp = TempDir::new().expect("tempdir");
    create_project_with_violation(temp.path());

    // Generate baseline
    run([
        "specgate",
        "baseline",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);

    // Add new violation
    write_file(
        temp.path(),
        "src/app/another.ts",
        "import { core } from '../core/index';\nexport const another = core;\n",
    );

    // Check with full diff mode
    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--diff",
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_POLICY_VIOLATIONS);

    let lines: Vec<&str> = result.stdout.lines().collect();
    let new_violation_lines: Vec<&&str> = lines.iter().filter(|l| l.starts_with('+')).collect();
    let baseline_lines: Vec<&&str> = lines.iter().filter(|l| l.starts_with(' ')).collect();

    // Should have 1 new and 1 baseline violation
    assert_eq!(new_violation_lines.len(), 1);
    assert_eq!(baseline_lines.len(), 1);
}

#[test]
fn check_diff_includes_summary() {
    let temp = TempDir::new().expect("tempdir");
    create_project_with_violation(temp.path());

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--diff",
        "--no-baseline",
    ]);

    // Should include Summary line at the end
    assert!(result.stdout.contains("Summary:"));
    assert!(result.stdout.contains("total"));
}

#[test]
fn check_diff_mode_without_violations_passes() {
    let temp = TempDir::new().expect("tempdir");
    create_basic_project(temp.path());

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--diff",
        "--no-baseline",
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_PASS);
    assert!(result.stdout.contains("Summary:"));
    assert!(result.stdout.contains("0 total"));
}
