use std::fs;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

use serde_json::Value;
use specgate::cli::{
    run, EXIT_CODE_DOCTOR_MISMATCH, EXIT_CODE_PASS, EXIT_CODE_POLICY_VIOLATIONS,
    EXIT_CODE_RUNTIME_ERROR,
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
fn telemetry_is_opt_in_via_config_or_runtime_flag() {
    // Sub-case 1: Default is off.
    let temp1 = TempDir::new().expect("tempdir");
    write_project(temp1.path());
    let default_result = run([
        "specgate",
        "check",
        "--project-root",
        temp1.path().to_str().expect("utf8"),
        "--no-baseline",
    ]);
    assert_eq!(default_result.exit_code, EXIT_CODE_PASS);
    assert!(!default_result.stdout.contains("\"telemetry\""));

    // Sub-case 2: Runtime opt-in turns telemetry on.
    let temp2 = TempDir::new().expect("tempdir");
    write_project(temp2.path());
    let runtime_opt_in = run([
        "specgate",
        "check",
        "--project-root",
        temp2.path().to_str().expect("utf8"),
        "--no-baseline",
        "--telemetry",
    ]);
    assert_eq!(runtime_opt_in.exit_code, EXIT_CODE_PASS);
    let runtime_opt_in_json = parse_json(&runtime_opt_in.stdout);
    assert!(runtime_opt_in_json.get("telemetry").is_some());
    let telemetry = &runtime_opt_in_json["telemetry"];
    assert_eq!(telemetry["event"], "check_completed");
    assert_eq!(telemetry["schema_version"], "1");
    assert!(telemetry["project_fingerprint"].as_str().is_some());
    assert!(telemetry["summary"].is_object());
    assert!(telemetry["summary"]["total_violations"].as_u64().is_some());
    assert!(telemetry["summary"]["new_violations"].as_u64().is_some());
    assert!(telemetry["summary"]["baseline_violations"]
        .as_u64()
        .is_some());
    assert!(telemetry["summary"]["new_error_violations"]
        .as_u64()
        .is_some());
    assert!(telemetry["summary"]["new_warning_violations"]
        .as_u64()
        .is_some());
    assert!(telemetry["summary"]["stale_baseline_entries"]
        .as_u64()
        .is_some());

    // Sub-case 3: Config opt-in also turns telemetry on.
    let temp3 = TempDir::new().expect("tempdir");
    write_project(temp3.path());
    write_file(
        temp3.path(),
        "specgate.config.yml",
        "spec_dirs:\n  - modules\nexclude: []\ntest_patterns: []\ntelemetry:\n  enabled: true\n",
    );
    let config_opt_in = run([
        "specgate",
        "check",
        "--project-root",
        temp3.path().to_str().expect("utf8"),
        "--no-baseline",
    ]);
    assert_eq!(config_opt_in.exit_code, EXIT_CODE_PASS);
    let config_opt_in_json = parse_json(&config_opt_in.stdout);
    assert!(config_opt_in_json.get("telemetry").is_some());

    // Sub-case 4: Runtime explicit off wins.
    let temp4 = TempDir::new().expect("tempdir");
    write_project(temp4.path());
    let runtime_force_off = run([
        "specgate",
        "check",
        "--project-root",
        temp4.path().to_str().expect("utf8"),
        "--no-baseline",
        "--no-telemetry",
    ]);
    assert_eq!(runtime_force_off.exit_code, EXIT_CODE_PASS);
    assert!(!runtime_force_off.stdout.contains("\"telemetry\""));
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
    assert!(baseline_json["generated_from"]["tool_version"]
        .as_str()
        .is_some());
    assert!(baseline_json["generated_from"]["git_sha"]
        .as_str()
        .is_some());
    assert!(baseline_json["generated_from"]["config_hash"]
        .as_str()
        .expect("config hash")
        .starts_with("sha256:"));
    assert!(baseline_json["generated_from"]["spec_hash"]
        .as_str()
        .expect("spec hash")
        .starts_with("sha256:"));

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
    assert!(verdict["config_hash"]
        .as_str()
        .expect("config hash")
        .starts_with("sha256:"));
    assert!(verdict["spec_hash"]
        .as_str()
        .expect("spec hash")
        .starts_with("sha256:"));

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
    assert!(result
        .stdout
        .contains("\"resolution_kind\": \"first_party\""));
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
    assert!(result
        .stdout
        .contains("\"trace_parser\": \"structured_snapshot\""));
    assert!(result
        .stdout
        .contains("\"structured_snapshot_out\": \"structured-output/out.json\""));

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
    assert!(result
        .stdout
        .contains("\"resolved_to\": \"packages/shared/src/util.ts\""));
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
