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

fn write_project_with_custom_import(root: &Path, import_specifier: &str) {
    write_project(root);
    write_file(
        root,
        "src/app/main.ts",
        &format!("import {{ core }} from '{import_specifier}';\nexport const app = core;\n"),
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

/// Validate all expected fields in a telemetry JSON object.
fn assert_telemetry_schema(telemetry: &Value) {
    // event field
    assert_eq!(
        telemetry["event"], "check_completed",
        "telemetry event must be 'check_completed'"
    );

    // schema_version field — must be a string "1"
    assert_eq!(
        telemetry["schema_version"], "1",
        "telemetry schema_version must be '1'"
    );

    // project_fingerprint — must be a non-empty string
    let fingerprint = telemetry["project_fingerprint"]
        .as_str()
        .expect("project_fingerprint must be a string");
    assert!(
        !fingerprint.is_empty(),
        "project_fingerprint must be non-empty"
    );

    // summary — must be an object with all expected numeric fields
    assert!(
        telemetry["summary"].is_object(),
        "telemetry summary must be an object"
    );
    let summary = &telemetry["summary"];
    for field in &[
        "total_violations",
        "new_violations",
        "baseline_violations",
        "new_error_violations",
        "new_warning_violations",
        "stale_baseline_entries",
    ] {
        assert!(
            summary[field].as_u64().is_some(),
            "telemetry summary.{field} must be a u64"
        );
    }
}

#[test]
fn telemetry_default_is_off() {
    let temp = TempDir::new().expect("tempdir");
    write_project(temp.path());
    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--no-baseline",
    ]);
    assert_eq!(result.exit_code, EXIT_CODE_PASS);
    assert!(
        !result.stdout.contains("\"telemetry\""),
        "telemetry must be absent by default"
    );
}

#[test]
fn telemetry_runtime_opt_in_produces_complete_event() {
    let temp = TempDir::new().expect("tempdir");
    write_project(temp.path());
    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--no-baseline",
        "--telemetry",
    ]);
    assert_eq!(result.exit_code, EXIT_CODE_PASS);
    let json = parse_json(&result.stdout);
    assert!(
        json.get("telemetry").is_some(),
        "telemetry must be present with --telemetry flag"
    );
    assert_telemetry_schema(&json["telemetry"]);
}

#[test]
fn telemetry_config_opt_in_produces_complete_event() {
    let temp = TempDir::new().expect("tempdir");
    write_project(temp.path());
    write_file(
        temp.path(),
        "specgate.config.yml",
        "spec_dirs:\n  - modules\nexclude: []\ntest_patterns: []\ntelemetry:\n  enabled: true\n",
    );
    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--no-baseline",
    ]);
    assert_eq!(result.exit_code, EXIT_CODE_PASS);
    let json = parse_json(&result.stdout);
    assert!(
        json.get("telemetry").is_some(),
        "telemetry must be present when config enables it"
    );
    assert_telemetry_schema(&json["telemetry"]);
}

#[test]
fn telemetry_runtime_no_telemetry_overrides_config() {
    let temp = TempDir::new().expect("tempdir");
    write_project(temp.path());
    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--no-baseline",
        "--no-telemetry",
    ]);
    assert_eq!(result.exit_code, EXIT_CODE_PASS);
    assert!(
        !result.stdout.contains("\"telemetry\""),
        "telemetry must be absent with --no-telemetry flag"
    );
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
    write_file(
        temp.path(),
        "trace.json",
        r#"{
  "schema_version": "1",
  "edges": [
    {
      "from": "src/app/main.ts",
      "to": "src/core/wrong.ts"
    }
  ]
}
"#,
    );

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
fn doctor_compare_without_trace_payload_reports_skipped_status_contract() {
    let temp = TempDir::new().expect("tempdir");
    write_project_with_edge(temp.path());

    let result = run([
        "specgate",
        "doctor",
        "compare",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_PASS);
    let output = parse_json(&result.stdout);
    assert_eq!(output["status"], "skipped");
    assert_eq!(output["parity_verdict"], "SKIPPED");
    assert!(
        output["mismatch_category"].is_null(),
        "mismatch_category must be absent on skipped compare"
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
fn doctor_compare_focus_mismatch_categorizes_paths_aliases() {
    let temp = TempDir::new().expect("tempdir");
    write_project_with_custom_import(temp.path(), "@core/index");
    write_file(
        temp.path(),
        "tsconfig.json",
        r#"{
  "compilerOptions": {
    "baseUrl": ".",
    "paths": {
      "@core/*": ["src/core/*"]
    }
  }
}
"#,
    );
    write_file(
        temp.path(),
        "trace.json",
        "[{\"from\":\"src/app/main.ts\",\"to\":\"src/core/alias-mismatch.ts\"}]\n",
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
        "@core/index",
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_DOCTOR_MISMATCH);
    assert!(result.stdout.contains("\"mismatch_category\": \"paths\""));
}

#[test]
fn doctor_compare_focus_mismatch_categorizes_exports_resolution() {
    let temp = TempDir::new().expect("tempdir");
    write_project_with_custom_import(temp.path(), "left-pad");
    write_file(
        temp.path(),
        "tsconfig.json",
        r#"{
  "compilerOptions": {
    "baseUrl": ".",
    "paths": {
      "left-pad": ["src/core/index.ts"]
    }
  }
}
"#,
    );
    write_file(
        temp.path(),
        "trace.json",
        "[{\"from\":\"src/app/main.ts\",\"to\":\"src/core/alias-mismatch.ts\"}]\n",
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
        "left-pad",
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_DOCTOR_MISMATCH);
    assert!(result.stdout.contains("\"mismatch_category\": \"exports\""));
}

#[test]
fn doctor_compare_focus_mismatch_categorizes_condition_names() {
    let temp = TempDir::new().expect("tempdir");
    write_project_with_custom_import(temp.path(), "typed-package");
    write_file(
        temp.path(),
        "src/core/index.d.ts",
        "export declare const typed: number;\n",
    );
    write_file(
        temp.path(),
        "tsconfig.json",
        r#"{
  "compilerOptions": {
    "baseUrl": ".",
    "paths": {
      "typed-package": ["src/core/index.d.ts"]
    }
  }
}
"#,
    );
    write_file(
        temp.path(),
        "trace.json",
        "[{\"from\":\"src/app/main.ts\",\"to\":\"src/core/alt-types.d.ts\"}]\n",
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
        "typed-package",
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_DOCTOR_MISMATCH);
    assert!(
        result
            .stdout
            .contains("\"mismatch_category\": \"condition_names\"")
    );
}

#[test]
fn doctor_compare_structured_snapshot_in_and_out_paths_work() {
    let temp = TempDir::new().expect("tempdir");
    write_project_with_edge(temp.path());
    write_file(
        temp.path(),
        "structured-input.json",
        r#"{
  "schema_version": "1",
  "edges": [
    { "from": "src/app/main.ts", "to": "src/core/index.ts" }
  ],
  "resolutions": [
    {
      "from": "src/app/main.ts",
      "import_specifier": "../core/index",
      "result_kind": "first_party",
      "resolved_to": "src/core/index.ts",
      "trace": ["fixture"]
    }
  ]
}
"#,
    );

    let result = run([
        "specgate",
        "doctor",
        "compare",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--structured-snapshot-in",
        temp.path()
            .join("structured-input.json")
            .to_str()
            .expect("utf8"),
        "--structured-snapshot-out",
        "structured-output/out.json",
        "--parser-mode",
        "structured",
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_PASS);
    assert!(result.stdout.contains("\"status\": \"match\""));
    assert!(
        result
            .stdout
            .contains("\"trace_parser\": \"structured_snapshot\"")
    );
    assert!(
        result
            .stdout
            .contains("\"structured_snapshot_out\": \"structured-output/out.json\"")
    );

    let structured_output = fs::read_to_string(temp.path().join("structured-output/out.json"))
        .expect("structured snapshot output should exist");
    let snapshot = parse_json(&structured_output);
    assert_eq!(snapshot["schema_version"], "1");
    assert_eq!(snapshot["edges"][0]["from"], "src/app/main.ts");
    assert_eq!(snapshot["edges"][0]["to"], "src/core/index.ts");
}

#[test]
fn doctor_compare_structured_snapshot_out_absolute_path() {
    let temp = TempDir::new().expect("tempdir");
    write_project_with_edge(temp.path());

    write_file(
        temp.path(),
        "structured-input.json",
        r#"{
  "schema_version": "1",
  "edges": [
    {
      "from": "src/app/main.ts",
      "to": "src/core/index.ts"
    }
  ],
  "resolutions": [
    {
      "from": "src/app/main.ts",
      "import_specifier": "../core/index",
      "result_kind": "first_party",
      "resolved_to": "src/core/index.ts",
      "trace": ["fixture"]
    }
  ]
}
"#,
    );

    let out_dir = TempDir::new().expect("tempdir");
    let abs_out_path = out_dir.path().join("abs-out.json");

    let result = run([
        "specgate",
        "doctor",
        "compare",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--structured-snapshot-in",
        temp.path()
            .join("structured-input.json")
            .to_str()
            .expect("utf8"),
        "--structured-snapshot-out",
        abs_out_path.to_str().expect("utf8"),
        "--parser-mode",
        "structured",
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_PASS);
    assert!(result.stdout.contains("\"status\": \"match\""));

    let structured_output = fs::read_to_string(&abs_out_path)
        .expect("structured snapshot output should exist at absolute path");
    let snapshot = parse_json(&structured_output);
    assert_eq!(snapshot["schema_version"], "1");
    assert_eq!(snapshot["edges"][0]["from"], "src/app/main.ts");
    assert_eq!(snapshot["edges"][0]["to"], "src/core/index.ts");
}

#[test]
fn doctor_compare_structured_snapshot_round_trip_multiple_edges() {
    let temp = TempDir::new().expect("tempdir");
    write_project_with_edge(temp.path());
    write_file(
        temp.path(),
        "src/app/main.ts",
        "import { core } from '../core/index';\nimport { utils } from '../shared/utils';\nexport const app = core;\n",
    );
    write_file(
        temp.path(),
        "src/shared/utils.ts",
        "export const utils = 1;\n",
    );

    write_file(
        temp.path(),
        "structured-input.json",
        r#"{
  "schema_version": "1",
  "edges": [
    {
      "from": "src/app/main.ts",
      "to": "src/core/index.ts"
    },
    {
      "from": "src/app/main.ts",
      "to": "src/shared/utils.ts"
    }
  ],
  "resolutions": [
    {
      "from": "src/app/main.ts",
      "import_specifier": "../core/index",
      "result_kind": "first_party",
      "resolved_to": "src/core/index.ts",
      "trace": ["fixture"]
    },
    {
      "from": "src/app/main.ts",
      "import_specifier": "../shared/utils",
      "result_kind": "first_party",
      "resolved_to": "src/shared/utils.ts",
      "trace": ["fixture2"]
    }
  ]
}
"#,
    );

    let result = run([
        "specgate",
        "doctor",
        "compare",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--structured-snapshot-in",
        temp.path()
            .join("structured-input.json")
            .to_str()
            .expect("utf8"),
        "--structured-snapshot-out",
        "structured-output/out.json",
        "--parser-mode",
        "structured",
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_PASS);

    let structured_output = fs::read_to_string(temp.path().join("structured-output/out.json"))
        .expect("structured snapshot output should exist");
    let snapshot = parse_json(&structured_output);
    assert_eq!(snapshot["schema_version"], "1");

    let edges = snapshot["edges"]
        .as_array()
        .expect("edges should be an array");
    assert_eq!(edges.len(), 2);

    // Edges are sorted by `from` then `to`
    assert_eq!(edges[0]["from"], "src/app/main.ts");
    assert_eq!(edges[0]["to"], "src/core/index.ts");

    assert_eq!(edges[1]["from"], "src/app/main.ts");
    assert_eq!(edges[1]["to"], "src/shared/utils.ts");
}

#[test]
fn doctor_compare_auto_mode_blocks_raw_trace_text_without_beta_hook() {
    let temp = TempDir::new().expect("tempdir");
    write_project_with_edge(temp.path());

    let app = temp.path().join("src/app/main.ts");
    let core = temp.path().join("src/core/index.ts");
    let trace = format!(
        "======== Resolving module '../core/index' from '{}'. ========\n======== Module name '../core/index' was successfully resolved to '{}'. ========\n",
        app.display(),
        core.display()
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
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_RUNTIME_ERROR);
    assert!(result.stdout.contains("\"status\": \"error\""));
    assert!(result.stdout.contains("beta-only"));
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
        "spec_dirs:\n  - modules\nexclude: []\ntest_patterns: []\nrelease_channel: beta\n",
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

#[test]
fn baseline_refresh_prunes_stale_entries_and_updates_baseline() {
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

    let baseline_result = run([
        "specgate",
        "baseline",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);
    assert_eq!(baseline_result.exit_code, EXIT_CODE_PASS);

    write_file(temp.path(), "src/app/main.ts", "export const app = 1;\n");

    let refresh_result = run([
        "specgate",
        "baseline",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--refresh",
    ]);
    assert_eq!(refresh_result.exit_code, EXIT_CODE_PASS);
    assert!(refresh_result.stdout.contains("\"stale_entries_pruned\""));

    let refresh_output = parse_json(&refresh_result.stdout);
    assert_eq!(refresh_output["stale_entries_pruned"], 1);
    assert_eq!(refresh_output["entry_count"], 0);

    let check_after_refresh = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);
    assert_eq!(check_after_refresh.exit_code, EXIT_CODE_PASS);

    let check_output = parse_json(&check_after_refresh.stdout);
    assert_eq!(check_output["status"], "pass");
    assert_eq!(check_output["summary"]["stale_baseline_entries"], 0);
}

#[test]
fn check_diff_mode_with_stale_baseline_fail_blocks_gate() {
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

    let baseline_result = run([
        "specgate",
        "baseline",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);
    assert_eq!(baseline_result.exit_code, EXIT_CODE_PASS);

    // Remove the violation so the baseline entry becomes stale
    write_file(temp.path(), "src/app/main.ts", "export const app = 1;\n");
    write_file(
        temp.path(),
        "specgate.config.yml",
        "spec_dirs:\n  - modules\nexclude: []\ntest_patterns: []\nstale_baseline: fail\n",
    );

    let check_with_diff = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--baseline-diff",
    ]);
    assert_eq!(check_with_diff.exit_code, EXIT_CODE_POLICY_VIOLATIONS);

    // Diff mode outputs text, not JSON. Verify governance info is present.
    assert!(check_with_diff.stdout.contains("Stale baseline entries: 1"));
    assert!(
        check_with_diff
            .stdout
            .contains("Stale baseline policy is `fail`")
    );
    assert!(check_with_diff.stdout.contains("governance:"));
    assert!(
        check_with_diff
            .stdout
            .contains("stale_baseline_policy: fail")
    );
    assert!(check_with_diff.stdout.contains("stale_baseline_entries: 1"));
}

#[test]
fn check_diff_mode_with_stale_baseline_warn_passes_gate() {
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

    let baseline_result = run([
        "specgate",
        "baseline",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);
    assert_eq!(baseline_result.exit_code, EXIT_CODE_PASS);

    // Remove the violation so the baseline entry becomes stale
    write_file(temp.path(), "src/app/main.ts", "export const app = 1;\n");
    write_file(
        temp.path(),
        "specgate.config.yml",
        "spec_dirs:\n  - modules\nexclude: []\ntest_patterns: []\nstale_baseline: warn\n",
    );

    let check_with_diff = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--baseline-diff",
    ]);
    assert_eq!(check_with_diff.exit_code, EXIT_CODE_PASS);

    // Stale entries should be mentioned but not cause failure in warn mode
    assert!(check_with_diff.stdout.contains("Stale baseline entries: 1"));
    // governance block should NOT appear (only appears on stale_policy_failure)
    assert!(!check_with_diff.stdout.contains("governance:"));
}

#[test]
fn baseline_diff_flag_with_stale_baseline_fail_blocks_gate() {
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

    let baseline_result = run([
        "specgate",
        "baseline",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);
    assert_eq!(baseline_result.exit_code, EXIT_CODE_PASS);

    write_file(temp.path(), "src/app/main.ts", "export const app = 1;\n");
    write_file(
        temp.path(),
        "specgate.config.yml",
        "spec_dirs:\n  - modules\nexclude: []\ntest_patterns: []\nstale_baseline: fail\n",
    );

    let check_with_diff = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);
    assert_eq!(check_with_diff.exit_code, EXIT_CODE_POLICY_VIOLATIONS);

    let output = parse_json(&check_with_diff.stdout);
    assert_eq!(output["status"], "fail");
    assert_eq!(output["summary"]["stale_baseline_entries"], 1);
    assert_eq!(output["governance"]["stale_baseline_policy"], "fail");
}

#[test]
fn baseline_diff_flag_with_stale_baseline_warn_passes_gate() {
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

    let baseline_result = run([
        "specgate",
        "baseline",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);
    assert_eq!(baseline_result.exit_code, EXIT_CODE_PASS);

    write_file(temp.path(), "src/app/main.ts", "export const app = 1;\n");
    write_file(
        temp.path(),
        "specgate.config.yml",
        "spec_dirs:\n  - modules\nexclude: []\ntest_patterns: []\nstale_baseline: warn\n",
    );

    let check_with_diff = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);
    assert_eq!(check_with_diff.exit_code, EXIT_CODE_PASS);

    let output = parse_json(&check_with_diff.stdout);
    assert_eq!(output["status"], "pass");
    assert_eq!(output["summary"]["stale_baseline_entries"], 1);
    assert_eq!(output["governance"]["stale_baseline_policy"], "warn");
}

#[test]
fn doctor_compare_focus_mismatch_categorizes_extension_alias() {
    let temp = TempDir::new().expect("tempdir");
    write_project_with_custom_import(temp.path(), "./helper.js");
    write_file(
        temp.path(),
        "src/app/helper.ts",
        "export const helper = 1;\n",
    );
    write_file(
        temp.path(),
        "tsconfig.json",
        r#"{ "compilerOptions": { "baseUrl": "." } }"#,
    );
    write_file(
        temp.path(),
        "trace.json",
        "[{\"from\":\"src/app/main.ts\",\"to\":\"src/app/helper-alt.ts\"}]\n",
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
        "./helper.js",
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_DOCTOR_MISMATCH);
    assert!(
        result
            .stdout
            .contains("\"mismatch_category\": \"extension_alias\""),
        "expected extension_alias mismatch category, got: {}",
        result.stdout
    );
}

#[test]
fn doctor_compare_structured_snapshot_golden_schema_shape() {
    let temp = TempDir::new().expect("tempdir");
    write_project_with_edge(temp.path());
    write_file(
        temp.path(),
        "snapshot.json",
        r#"{
  "schema_version": "1",
  "edges": [
    { "from": "src/app/main.ts", "to": "src/core/index.ts" }
  ],
  "resolutions": [
    {
      "from": "src/app/main.ts",
      "import_specifier": "../core/index",
      "result_kind": "first_party",
      "resolved_to": "src/core/index.ts",
      "trace": ["step1", "step2"]
    }
  ]
}
"#,
    );

    let result = run([
        "specgate",
        "doctor",
        "compare",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--structured-snapshot-in",
        temp.path().join("snapshot.json").to_str().expect("utf8"),
        "--structured-snapshot-out",
        "golden/out.json",
        "--parser-mode",
        "structured",
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_PASS);

    let golden_output = fs::read_to_string(temp.path().join("golden/out.json"))
        .expect("golden snapshot output should exist");
    let snapshot = parse_json(&golden_output);

    // Lock the schema version
    assert_eq!(
        snapshot["schema_version"], "1",
        "structured snapshot schema_version must be '1'"
    );

    // Lock the top-level shape: must have edges and resolutions arrays
    assert!(
        snapshot["edges"].is_array(),
        "snapshot must have 'edges' array"
    );
    assert!(
        snapshot["resolutions"].is_array(),
        "snapshot must have 'resolutions' array"
    );

    // Lock edge record shape
    let edge = &snapshot["edges"][0];
    assert!(edge["from"].is_string(), "edge must have string 'from'");
    assert!(edge["to"].is_string(), "edge must have string 'to'");

    // Lock resolution record shape
    let resolution = &snapshot["resolutions"][0];
    assert!(
        resolution["from"].is_string(),
        "resolution must have string 'from'"
    );
    assert!(
        resolution["import_specifier"].is_string(),
        "resolution must have string 'import_specifier'"
    );
    assert!(
        resolution["result_kind"].is_string(),
        "resolution must have string 'result_kind'"
    );
    assert!(
        resolution["resolved_to"].is_string(),
        "resolution must have string 'resolved_to'"
    );
    assert!(
        resolution["trace"].is_array(),
        "resolution must have array 'trace'"
    );

    // Verify no extra top-level keys leak into the output schema
    let snapshot_map = snapshot.as_object().expect("snapshot is an object");
    let allowed_keys: std::collections::BTreeSet<&str> = ["schema_version", "edges", "resolutions"]
        .iter()
        .copied()
        .collect();
    let actual_keys: std::collections::BTreeSet<&str> =
        snapshot_map.keys().map(|k| k.as_str()).collect();
    assert_eq!(
        actual_keys, allowed_keys,
        "snapshot top-level keys must be exactly {{schema_version, edges, resolutions}}"
    );
}

#[test]
fn doctor_compare_npm_wrapper_snapshot_round_trip_with_categories() {
    let temp = TempDir::new().expect("tempdir");
    write_project_with_custom_import(temp.path(), "@core/utils");
    write_file(
        temp.path(),
        "tsconfig.json",
        r#"{
  "compilerOptions": {
    "baseUrl": ".",
    "paths": {
      "@core/*": ["src/core/*"]
    }
  }
}
"#,
    );

    // Provide a file that specgate will resolve @core/utils to
    write_file(
        temp.path(),
        "src/core/utils.ts",
        "export const utils = 1;\n",
    );

    // Create a snapshot in the exact format the npm wrapper produces.
    // The npm wrapper includes extra fields (snapshot_kind, producer, generated_at,
    // project_root, tsconfig_path, focus, source) that Rust must silently ignore.
    // The resolved_to intentionally differs from what specgate resolves to trigger a mismatch.
    write_file(
        temp.path(),
        "npm-snapshot.json",
        r#"{
  "schema_version": "1",
  "snapshot_kind": "doctor_compare_tsc_resolution_focus",
  "producer": "specgate-npm-wrapper",
  "generated_at": "2026-03-04T00:00:00.000Z",
  "project_root": "/fake/project",
  "tsconfig_path": "tsconfig.json",
  "focus": {
    "from": "src/app/main.ts",
    "import_specifier": "@core/utils"
  },
  "resolutions": [
    {
      "source": "tsc_compiler_api",
      "from": "src/app/main.ts",
      "import_specifier": "@core/utils",
      "result_kind": "first_party",
      "resolved_to": "src/core/mismatch-target.ts",
      "trace": [
        "tsconfig: tsconfig.json",
        "module_resolution: Bundler",
        "from: src/app/main.ts",
        "import: @core/utils",
        "result_kind: first_party",
        "resolved_to: src/core/mismatch-target.ts"
      ]
    }
  ],
  "edges": [
    {
      "from": "src/app/main.ts",
      "to": "src/core/mismatch-target.ts"
    }
  ]
}
"#,
    );

    let result = run([
        "specgate",
        "doctor",
        "compare",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--structured-snapshot-in",
        temp.path()
            .join("npm-snapshot.json")
            .to_str()
            .expect("utf8"),
        "--from",
        "src/app/main.ts",
        "--import",
        "@core/utils",
        "--parser-mode",
        "structured",
    ]);

    // The specgate resolver and the npm snapshot disagree on the target,
    // so we expect a mismatch with the "paths" category tag.
    assert_eq!(result.exit_code, EXIT_CODE_DOCTOR_MISMATCH);
    let output = parse_json(&result.stdout);
    assert_eq!(output["status"], "mismatch");
    assert_eq!(output["parity_verdict"], "DIFF");
    assert_eq!(output["parser_mode"], "structured");
    assert_eq!(output["trace_parser"], "structured_snapshot");
    assert_eq!(output["mismatch_category"], "paths");

    // Verify the structured snapshot was parsed correctly and resolution data flows through
    assert!(
        output["tsc_trace_resolution"].is_object(),
        "tsc_trace_resolution must be present for focused compare"
    );
    assert_eq!(
        output["tsc_trace_resolution"]["source"], "tsc_trace",
        "resolution source should be set by doctor compare"
    );
    assert_eq!(output["tsc_trace_resolution"]["result_kind"], "first_party");

    // Verify focus section
    assert!(output["focus"].is_object(), "focus section must be present");
    assert_eq!(output["focus"]["from"], "src/app/main.ts");
    assert_eq!(output["focus"]["import_specifier"], "@core/utils");

    // Verify the actionable hint is present for mismatch
    assert!(
        output["actionable_mismatch_hint"].is_string(),
        "actionable_mismatch_hint must be present on mismatch"
    );
}

#[test]
fn doctor_compare_npm_wrapper_snapshot_match_round_trip() {
    let temp = TempDir::new().expect("tempdir");
    write_project_with_edge(temp.path());

    // Create a snapshot in npm wrapper format that matches what specgate resolves
    write_file(
        temp.path(),
        "npm-snapshot.json",
        r#"{
  "schema_version": "1",
  "snapshot_kind": "doctor_compare_tsc_resolution_focus",
  "producer": "specgate-npm-wrapper",
  "generated_at": "2026-03-04T00:00:00.000Z",
  "project_root": "/fake/project",
  "tsconfig_path": "tsconfig.json",
  "focus": {
    "from": "src/app/main.ts",
    "import_specifier": "../core/index"
  },
  "resolutions": [
    {
      "source": "tsc_compiler_api",
      "from": "src/app/main.ts",
      "import_specifier": "../core/index",
      "result_kind": "first_party",
      "resolved_to": "src/core/index.ts",
      "trace": [
        "tsconfig: tsconfig.json",
        "from: src/app/main.ts",
        "import: ../core/index",
        "result_kind: first_party",
        "resolved_to: src/core/index.ts"
      ]
    }
  ],
  "edges": [
    {
      "from": "src/app/main.ts",
      "to": "src/core/index.ts"
    }
  ]
}
"#,
    );

    let result = run([
        "specgate",
        "doctor",
        "compare",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--structured-snapshot-in",
        temp.path()
            .join("npm-snapshot.json")
            .to_str()
            .expect("utf8"),
        "--parser-mode",
        "structured",
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_PASS);
    let output = parse_json(&result.stdout);
    assert_eq!(output["status"], "match");
    assert_eq!(output["parity_verdict"], "MATCH");
    assert_eq!(output["trace_parser"], "structured_snapshot");

    // On match, mismatch_category must be absent
    assert!(
        output["mismatch_category"].is_null(),
        "mismatch_category must be absent on match"
    );
}
