use std::fs;
use std::path::Path;

use tempfile::TempDir;

use serde_json::Value;
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

fn write_project(root: &Path) {
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

#[test]
fn check_with_metrics_includes_timing_metadata() {
    let temp = TempDir::new().expect("tempdir");
    write_project(temp.path());

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--metrics",
        "--no-baseline",
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_PASS);
    assert!(result.stdout.contains("\"metrics\""));
    assert!(result.stdout.contains("\"timings_ms\""));

    let parsed = parse_json(&result.stdout);
    assert_eq!(parsed["output_mode"], "metrics");
    assert!(parsed["tool_version"].as_str().is_some());
    assert!(parsed["config_hash"].as_str().is_some());
    assert!(parsed["spec_hash"].as_str().is_some());
    assert_eq!(parsed["spec_files_changed"], Value::Array(Vec::new()));
    assert_eq!(parsed["rule_deltas"], Value::Array(Vec::new()));
    assert_eq!(parsed["policy_change_detected"], Value::Bool(false));
}

#[test]
fn baseline_file_is_stable_and_used_for_report_only() {
    let temp = TempDir::new().expect("tempdir");
    write_project(temp.path());
    write_file(
        temp.path(),
        "modules/app.spec.yml",
        "version: \"2.2\"\nmodule: app\nboundaries:\n  path: src/app/**/*\n  never_imports:\n    - core\nconstraints: []\n",
    );
    write_file(
        temp.path(),
        "src/app/main.ts",
        "import { core } from '../core/index';\nexport const app = core;\n",
    );

    let baseline = run([
        "specgate",
        "baseline",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);
    assert_eq!(baseline.exit_code, EXIT_CODE_PASS);

    let baseline_file = fs::read_to_string(temp.path().join(".specgate-baseline.json"))
        .expect("baseline file exists");
    let baseline_json = parse_json(&baseline_file);
    assert!(
        baseline_json["generated_from"]["tool_version"]
            .as_str()
            .is_some()
    );
    assert!(
        baseline_json["generated_from"]["git_sha"]
            .as_str()
            .is_some()
    );
    assert!(
        baseline_json["generated_from"]["config_hash"]
            .as_str()
            .expect("config hash")
            .starts_with("sha256:")
    );
    assert!(
        baseline_json["generated_from"]["spec_hash"]
            .as_str()
            .expect("spec hash")
            .starts_with("sha256:")
    );

    let first = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);
    let second = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);

    assert_eq!(first.exit_code, EXIT_CODE_PASS);
    assert_eq!(first.stdout, second.stdout);
    assert!(first.stdout.contains("\"baseline_violations\": 1"));

    let verdict = parse_json(&first.stdout);
    assert_eq!(verdict["output_mode"], "deterministic");
    assert!(
        verdict["config_hash"]
            .as_str()
            .expect("config hash")
            .starts_with("sha256:")
    );
    assert!(
        verdict["spec_hash"]
            .as_str()
            .expect("spec hash")
            .starts_with("sha256:")
    );

    write_file(
        temp.path(),
        "src/app/extra.ts",
        "import { core } from '../core/index';\nexport const extra = core;\n",
    );

    let with_new = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);
    assert_eq!(with_new.exit_code, EXIT_CODE_POLICY_VIOLATIONS);
}

#[test]
fn doctor_compare_invalid_trace_json_is_runtime_error() {
    let temp = TempDir::new().expect("tempdir");
    write_project(temp.path());
    write_file(temp.path(), "trace.json", "this is not json\n");

    let result = run([
        "specgate",
        "doctor",
        "compare",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--tsc-trace",
        temp.path().join("trace.json").to_str().expect("utf8"),
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_RUNTIME_ERROR);
    assert!(result.stdout.contains("\"status\": \"error\""));
}
