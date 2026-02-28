//! MVP gate baseline/new-violation behavior checks.
//!
//! This suite is intentionally narrow and CI-facing:
//! - baseline hits are report-only (exit 0)
//! - net-new violations fail policy gate (exit 1)

use std::fs;
use std::path::Path;

use serde_json::Value;
use tempfile::TempDir;

use specgate::cli::{run, EXIT_CODE_PASS, EXIT_CODE_POLICY_VIOLATIONS};

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

fn create_project_with_violation(root: &Path) {
    write_file(
        root,
        "modules/app.spec.yml",
        "version: \"2.2\"\nmodule: app\nboundaries:\n  path: src/app/**/*\n  never_imports:\n    - core\nconstraints: []\n",
    );
    write_file(
        root,
        "modules/core.spec.yml",
        "version: \"2.2\"\nmodule: core\nboundaries:\n  path: src/core/**/*\nconstraints: []\n",
    );
    write_file(
        root,
        "src/app/main.ts",
        "import { core } from '../core/index';\nexport const app = core;\n",
    );
    write_file(root, "src/core/index.ts", "export const core = 1;\n");
    write_file(
        root,
        "specgate.config.yml",
        "spec_dirs:\n  - modules\nexclude: []\ntest_patterns: []\n",
    );
}

#[test]
fn baseline_hit_is_report_only_for_gate() {
    let temp = TempDir::new().expect("tempdir");
    create_project_with_violation(temp.path());

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
    ]);
    assert_eq!(check_result.exit_code, EXIT_CODE_PASS);

    let output = parse_json(&check_result.stdout);
    assert_eq!(output["status"], "pass");
    assert!(output["summary"]["baseline_violations"].as_u64().unwrap() > 0);
    assert_eq!(output["summary"]["new_violations"], 0);
    assert_eq!(output["summary"]["new_error_violations"], 0);
}

#[test]
fn new_violation_after_baseline_fails_policy_gate() {
    let temp = TempDir::new().expect("tempdir");
    create_project_with_violation(temp.path());

    let baseline_result = run([
        "specgate",
        "baseline",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);
    assert_eq!(baseline_result.exit_code, EXIT_CODE_PASS);

    write_file(
        temp.path(),
        "src/app/another.ts",
        "import { core } from '../core/index';\nexport const another = core;\n",
    );

    let check_result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);
    assert_eq!(check_result.exit_code, EXIT_CODE_POLICY_VIOLATIONS);

    let output = parse_json(&check_result.stdout);
    assert_eq!(output["status"], "fail");
    assert_eq!(output["summary"]["new_violations"], 1);
    assert_eq!(output["summary"]["new_error_violations"], 1);
    assert_eq!(output["summary"]["baseline_violations"], 1);
}

#[test]
fn stale_baseline_policy_fail_blocks_gate_when_entries_are_stale() {
    let temp = TempDir::new().expect("tempdir");
    create_project_with_violation(temp.path());

    let baseline_result = run([
        "specgate",
        "baseline",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);
    assert_eq!(baseline_result.exit_code, EXIT_CODE_PASS);

    // Remove the violation so the baseline entry becomes stale.
    write_file(temp.path(), "src/app/main.ts", "export const app = 1;\n");
    write_file(
        temp.path(),
        "specgate.config.yml",
        "spec_dirs:\n  - modules\nexclude: []\ntest_patterns: []\nstale_baseline: fail\n",
    );

    let check_result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);
    assert_eq!(check_result.exit_code, EXIT_CODE_POLICY_VIOLATIONS);

    let output = parse_json(&check_result.stdout);
    assert_eq!(output["status"], "fail");
    assert_eq!(output["summary"]["new_violations"], 0);
    assert_eq!(output["summary"]["new_error_violations"], 0);
    assert_eq!(output["summary"]["stale_baseline_entries"], 1);
    assert_eq!(output["governance"]["stale_baseline_policy"], "fail");
}

#[test]
fn stale_baseline_policy_warn_does_not_block_gate_when_entries_are_stale() {
    let temp = TempDir::new().expect("tempdir");
    create_project_with_violation(temp.path());

    let baseline_result = run([
        "specgate",
        "baseline",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);
    assert_eq!(baseline_result.exit_code, EXIT_CODE_PASS);

    // Remove the violation so the baseline entry becomes stale.
    write_file(temp.path(), "src/app/main.ts", "export const app = 1;\n");
    write_file(
        temp.path(),
        "specgate.config.yml",
        "spec_dirs:\n  - modules\nexclude: []\ntest_patterns: []\nstale_baseline: warn\n",
    );

    let check_result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);
    assert_eq!(check_result.exit_code, EXIT_CODE_PASS);

    let output = parse_json(&check_result.stdout);
    assert_eq!(output["status"], "pass");
    assert_eq!(output["summary"]["new_violations"], 0);
    assert_eq!(output["summary"]["new_error_violations"], 0);
    assert_eq!(output["summary"]["stale_baseline_entries"], 1);
    assert_eq!(output["governance"]["stale_baseline_policy"], "warn");
}

#[test]
fn baseline_refresh_prunes_stale_entries() {
    let temp = TempDir::new().expect("tempdir");
    create_project_with_violation(temp.path());

    let baseline_result = run([
        "specgate",
        "baseline",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);
    assert_eq!(baseline_result.exit_code, EXIT_CODE_PASS);

    // Resolve violation so baseline entry is stale.
    write_file(temp.path(), "src/app/main.ts", "export const app = 1;\n");

    let refresh_result = run([
        "specgate",
        "baseline",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--refresh",
    ]);
    assert_eq!(refresh_result.exit_code, EXIT_CODE_PASS);

    let refresh_output = parse_json(&refresh_result.stdout);
    assert_eq!(refresh_output["refreshed"], true);
    assert_eq!(refresh_output["stale_entries_pruned"], 1);

    let baseline_file = fs::read_to_string(temp.path().join(".specgate-baseline.json"))
        .expect("baseline file should exist");
    let baseline_json = parse_json(&baseline_file);
    assert_eq!(
        baseline_json["entries"]
            .as_array()
            .expect("entries array")
            .len(),
        0
    );
}
