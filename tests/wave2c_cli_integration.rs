use std::fs;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

use serde_json::Value;
use specgate::cli::{
    EXIT_CODE_DOCTOR_MISMATCH, EXIT_CODE_PASS, EXIT_CODE_POLICY_VIOLATIONS,
    EXIT_CODE_RUNTIME_ERROR, run,
};

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

fn fixture_root(relative_path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(relative_path)
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

fn write_project_with_edge(root: &Path) {
    write_project(root);
    write_file(
        root,
        "src/app/main.ts",
        "import { core } from '../core/index';\nexport const app = core;\n",
    );
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
fn check_with_custom_baseline_path_marks_report_only() {
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
        "--output",
        "custom-baseline.json",
    ]);
    assert_eq!(baseline.exit_code, EXIT_CODE_PASS);

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--baseline",
        "custom-baseline.json",
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_PASS);
    assert!(result.stdout.contains("\"baseline_violations\": 1"));
}

#[test]
fn check_with_malformed_baseline_file_is_runtime_error() {
    let temp = TempDir::new().expect("tempdir");
    write_project(temp.path());
    write_file(temp.path(), "broken-baseline.json", "this is not json\n");

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--baseline",
        "broken-baseline.json",
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_RUNTIME_ERROR);
    assert!(result.stdout.contains("\"code\": \"baseline\""));
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

#[test]
fn doctor_compare_mismatch_uses_diagnostic_exit_code() {
    let temp = TempDir::new().expect("tempdir");
    write_project_with_edge(temp.path());
    write_file(temp.path(), "trace.json", "[]\n");

    let result = run([
        "specgate",
        "doctor",
        "compare",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--tsc-trace",
        temp.path().join("trace.json").to_str().expect("utf8"),
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_DOCTOR_MISMATCH);
    assert!(result.stdout.contains("\"status\": \"mismatch\""));
    assert!(result.stdout.contains("\"parity_verdict\": \"DIFF\""));
}

#[test]
fn doctor_compare_supports_successful_tsc_command_path() {
    let temp = TempDir::new().expect("tempdir");
    write_project_with_edge(temp.path());

    let result = run([
        "specgate",
        "doctor",
        "compare",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--tsc-command",
        "printf '[{\"from\":\"src/app/main.ts\",\"to\":\"src/core/index.ts\"}]'",
        "--allow-shell",
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_PASS);
    assert!(result.stdout.contains("\"status\": \"match\""));
}

#[test]
fn doctor_compare_supports_single_import_focus_mode() {
    let temp = TempDir::new().expect("tempdir");
    write_project_with_edge(temp.path());
    write_file(
        temp.path(),
        "trace.json",
        "[{\"from\":\"src/app/main.ts\",\"to\":\"src/core/index.ts\"}]\n",
    );

    let result = run([
        "specgate",
        "doctor",
        "compare",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--tsc-trace",
        temp.path().join("trace.json").to_str().expect("utf8"),
        "--from",
        "src/app/main.ts",
        "--import",
        "../core/index",
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_PASS);
    assert!(result.stdout.contains("\"focus\""));
    assert!(result.stdout.contains("\"parity_verdict\": \"MATCH\""));
    assert!(result.stdout.contains("\"specgate_resolution\""));
    assert!(result.stdout.contains("\"tsc_trace_resolution\""));
    assert!(
        result
            .stdout
            .contains("\"resolution_kind\": \"first_party\"")
    );
}

#[test]
fn doctor_compare_focus_mismatch_includes_actionable_hint() {
    let temp = TempDir::new().expect("tempdir");
    write_project_with_edge(temp.path());
    write_file(
        temp.path(),
        "trace.json",
        "[{\"from\":\"src/app/main.ts\",\"to\":\"src/core/other.ts\"}]\n",
    );

    let result = run([
        "specgate",
        "doctor",
        "compare",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--tsc-trace",
        temp.path().join("trace.json").to_str().expect("utf8"),
        "--from",
        "src/app/main.ts",
        "--import",
        "../core/index",
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_DOCTOR_MISMATCH);
    assert!(result.stdout.contains("\"parity_verdict\": \"DIFF\""));
    assert!(result.stdout.contains("\"actionable_mismatch_hint\""));
    assert!(result.stdout.contains("tsconfig"));
}

#[test]
fn doctor_compare_focus_supports_raw_tsc_trace_text_in_monorepo_layout() {
    let temp = TempDir::new().expect("tempdir");

    write_file(
        temp.path(),
        "modules/web.spec.yml",
        "version: \"2.2\"\nmodule: web\nboundaries:\n  path: packages/web/src/**/*\nconstraints: []\n",
    );
    write_file(
        temp.path(),
        "modules/shared.spec.yml",
        "version: \"2.2\"\nmodule: shared\nboundaries:\n  path: packages/shared/src/**/*\nconstraints: []\n",
    );
    write_file(
        temp.path(),
        "specgate.config.yml",
        "spec_dirs:\n  - modules\nexclude: []\ntest_patterns: []\n",
    );
    write_file(
        temp.path(),
        "tsconfig.json",
        r#"{
  "files": [],
  "references": [
    { "path": "./packages/shared" },
    { "path": "./packages/web" }
  ],
  "compilerOptions": {
    "baseUrl": ".",
    "paths": {
      "@shared/*": ["packages/shared/src/*"]
    }
  }
}
"#,
    );
    write_file(
        temp.path(),
        "packages/shared/tsconfig.json",
        "{\"compilerOptions\":{\"composite\":true},\"include\":[\"src\"]}\n",
    );
    write_file(
        temp.path(),
        "packages/web/tsconfig.json",
        "{\"compilerOptions\":{\"composite\":true},\"references\":[{\"path\":\"../shared\"}],\"include\":[\"src\"]}\n",
    );
    write_file(
        temp.path(),
        "packages/shared/src/util.ts",
        "export const util = 1;\n",
    );
    write_file(
        temp.path(),
        "packages/web/src/app.ts",
        "import { util } from '@shared/util';\nexport const app = util;\n",
    );

    let web_app = temp.path().join("packages/web/src/app.ts");
    let shared_util = temp.path().join("packages/shared/src/util.ts");
    let trace = format!(
        "======== Resolving module '@shared/util' from '{}'. ========\nLoading module '@shared/util' from 'paths' option.\n======== Module name '@shared/util' was successfully resolved to '{}'. ========\n",
        web_app.display(),
        shared_util.display(),
    );
    write_file(temp.path(), "trace.log", &trace);

    let result = run([
        "specgate",
        "doctor",
        "compare",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--tsc-trace",
        temp.path().join("trace.log").to_str().expect("utf8"),
        "--from",
        "packages/web/src/app.ts",
        "--import",
        "@shared/util",
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_PASS);
    assert!(result.stdout.contains("\"status\": \"match\""));
    assert!(result.stdout.contains("\"parity_verdict\": \"MATCH\""));
    assert!(result.stdout.contains("\"source\": \"tsc_trace\""));
}

#[test]
fn doctor_compare_focus_supports_project_reference_trace_fixture() {
    let fixture = fixture_root("doctor-compare/monorepo-project-reference");

    let result = run([
        "specgate",
        "doctor",
        "compare",
        "--project-root",
        fixture.to_str().expect("utf8"),
        "--tsc-trace",
        fixture.join("trace.log").to_str().expect("utf8"),
        "--from",
        "packages/web/src/app.ts",
        "--import",
        "@shared/util",
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_PASS);
    assert!(result.stdout.contains("\"status\": \"match\""));
    assert!(result.stdout.contains("\"parity_verdict\": \"MATCH\""));
    assert!(result.stdout.contains("\"source\": \"tsc_trace\""));
    assert!(
        result
            .stdout
            .contains("\"resolved_to\": \"packages/shared/src/util.ts\"")
    );
}
