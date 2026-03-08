use std::fs;

use serde_json::Value;
use tempfile::TempDir;

use super::*;

fn write_file(root: &Path, relative_path: &str, content: &str) {
    let path = root.join(relative_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("mkdir parent");
    }
    fs::write(path, content).expect("write file");
}

fn write_basic_project(root: &Path) {
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
    write_file(root, "src/app/main.ts", "export const app = 1;\n");
    write_file(root, "src/core/index.ts", "export const core = 1;\n");
    write_file(
        root,
        "specgate.config.yml",
        "spec_dirs:\n  - modules\nexclude: []\ntest_patterns: []\n",
    );
}

fn write_basic_project_with_edge(root: &Path) {
    write_basic_project(root);
    write_file(
        root,
        "src/app/main.ts",
        "import { core } from '../core/index';\nexport const app = core;\n",
    );
}

fn parse_json(stdout: &str) -> Value {
    serde_json::from_str(stdout).expect("cli output json")
}

#[test]
fn validate_returns_exit_two_on_schema_errors() {
    let temp = TempDir::new().expect("tempdir");
    write_file(
        temp.path(),
        "modules/bad.spec.yml",
        "version: \"2.1\"\nmodule: bad\nconstraints: []\n",
    );
    write_file(
        temp.path(),
        "specgate.config.yml",
        "spec_dirs:\n  - modules\nexclude: []\ntest_patterns: []\n",
    );

    let result = run([
        "specgate",
        "validate",
        "--project-root",
        temp.path().to_str().expect("utf8 path"),
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_RUNTIME_ERROR);
    assert!(result.stdout.contains("\"status\": \"error\""));
}

#[test]
fn check_exit_codes_follow_policy_vs_runtime_contract() {
    let temp = TempDir::new().expect("tempdir");
    write_basic_project(temp.path());

    // Clean policy: exit 0.
    let pass = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8 path"),
        "--no-baseline",
    ]);
    assert_eq!(pass.exit_code, EXIT_CODE_PASS);

    // Introduce policy violation: app may never import core.
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

    let fail = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8 path"),
        "--no-baseline",
    ]);
    assert_eq!(fail.exit_code, EXIT_CODE_POLICY_VIOLATIONS);

    // Introduce runtime/config error.
    write_file(
        temp.path(),
        "specgate.config.yml",
        "spec_dirs:\n  - modules\nexclude:\n  - \"[\"\n",
    );
    let runtime = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8 path"),
        "--no-baseline",
    ]);
    assert_eq!(runtime.exit_code, EXIT_CODE_RUNTIME_ERROR);
}

#[test]
fn check_output_is_deterministic_by_default() {
    let temp = TempDir::new().expect("tempdir");
    write_basic_project(temp.path());
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

    let one = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8 path"),
        "--no-baseline",
    ]);
    let two = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8 path"),
        "--no-baseline",
    ]);

    assert_eq!(one.exit_code, EXIT_CODE_POLICY_VIOLATIONS);
    assert_eq!(one.stdout, two.stdout);
    assert!(!one.stdout.contains("\"metrics\""));
}

#[test]
fn boundary_constraint_severity_is_propagated_to_verdict() {
    let temp = TempDir::new().expect("tempdir");
    write_basic_project(temp.path());

    write_file(
        temp.path(),
        "modules/app.spec.yml",
        "version: \"2.2\"\nmodule: app\nboundaries:\n  path: src/app/**/*\n  never_imports:\n    - core\nconstraints:\n  - rule: boundary.never_imports\n    severity: warning\n",
    );
    write_file(
        temp.path(),
        "src/app/main.ts",
        "import { core } from '../core/index';\nexport const app = core;\n",
    );

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8 path"),
        "--no-baseline",
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_PASS);
    let output = parse_json(&result.stdout);
    assert_eq!(output["status"], "pass");
    assert_eq!(output["summary"]["new_error_violations"], 0);
    assert_eq!(output["summary"]["new_warning_violations"], 1);
    assert_eq!(output["violations"][0]["rule"], "boundary.never_imports");
    assert_eq!(output["violations"][0]["severity"], "warning");
}

#[test]
fn canonical_import_alias_constraint_maps_to_canonical_rule_id() {
    let temp = TempDir::new().expect("tempdir");
    write_basic_project(temp.path());

    write_file(
        temp.path(),
        "modules/core.spec.yml",
        "version: \"2.2\"\nmodule: core\nimport_id: '@app/core'\nboundaries:\n  path: src/core/**/*\n  enforce_canonical_imports: true\nconstraints:\n  - rule: boundary.canonical_imports\n    severity: warning\n",
    );
    write_file(
        temp.path(),
        "src/app/main.ts",
        "import { core } from '../core/index';\nexport const app = core;\n",
    );

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8 path"),
        "--no-baseline",
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_PASS);
    let output = parse_json(&result.stdout);
    assert_eq!(output["status"], "pass");
    assert_eq!(output["summary"]["new_error_violations"], 0);
    assert_eq!(output["summary"]["new_warning_violations"], 1);
    assert_eq!(
        output["violations"][0]["rule"],
        crate::rules::BOUNDARY_CANONICAL_IMPORT_RULE_ID
    );
    assert_eq!(output["violations"][0]["severity"], "warning");
}

#[test]
fn baseline_generation_and_check_classification_work_together() {
    let temp = TempDir::new().expect("tempdir");
    write_basic_project(temp.path());

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
        temp.path().to_str().expect("utf8 path"),
    ]);
    assert_eq!(baseline.exit_code, EXIT_CODE_PASS);

    let with_baseline = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8 path"),
    ]);
    assert_eq!(with_baseline.exit_code, EXIT_CODE_PASS);
    assert!(with_baseline.stdout.contains("\"baseline_violations\": 1"));

    write_file(
        temp.path(),
        "src/app/another.ts",
        "import { core } from '../core/index';\nexport const another = core;\n",
    );

    let new_violation = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8 path"),
    ]);

    assert_eq!(new_violation.exit_code, EXIT_CODE_POLICY_VIOLATIONS);
    assert!(new_violation.stdout.contains("\"new_violations\": 1"));
}

#[test]
fn baseline_refresh_rewrites_unknown_governance_hashes() {
    let temp = TempDir::new().expect("tempdir");
    write_basic_project(temp.path());
    write_file(
        temp.path(),
        "legacy-baseline.json",
        r#"{
  "version": "1",
  "generated_from": {
    "tool_version": "legacy",
    "git_sha": "legacy",
    "config_hash": "sha256:unknown",
    "spec_hash": "sha256:unknown"
  },
  "entries": []
}
"#,
    );

    let refreshed = run([
        "specgate",
        "baseline",
        "--project-root",
        temp.path().to_str().expect("utf8 path"),
        "--output",
        "legacy-baseline.json",
        "--refresh",
    ]);
    assert_eq!(refreshed.exit_code, EXIT_CODE_PASS);

    let baseline = fs::read_to_string(temp.path().join("legacy-baseline.json"))
        .expect("refreshed baseline output");
    let parsed = parse_json(&baseline);
    let config_hash = parsed["generated_from"]["config_hash"]
        .as_str()
        .expect("config hash string");
    let spec_hash = parsed["generated_from"]["spec_hash"]
        .as_str()
        .expect("spec hash string");

    assert_ne!(config_hash, "sha256:unknown");
    assert_ne!(spec_hash, "sha256:unknown");
    assert!(config_hash.starts_with("sha256:"));
    assert!(spec_hash.starts_with("sha256:"));
}

#[test]
fn doctor_compare_skips_gracefully_when_tsc_missing() {
    let temp = TempDir::new().expect("tempdir");
    write_basic_project(temp.path());

    let result = run([
        "specgate",
        "doctor",
        "compare",
        "--project-root",
        temp.path().to_str().expect("utf8 path"),
        "--tsc-command",
        "__specgate_missing_tsc__ --generateTrace .specgate-trace --noEmit",
        "--allow-shell",
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_PASS);
    assert!(result.stdout.contains("\"status\": \"skipped\""));
    assert!(result.stdout.contains("\"parity_verdict\": \"SKIPPED\""));
}

#[test]
fn doctor_compare_legacy_parser_mode_requires_beta_channel() {
    let temp = TempDir::new().expect("tempdir");
    write_basic_project_with_edge(temp.path());
    let from = temp.path().join("src/app/main.ts");
    let to = temp.path().join("src/core/index.ts");
    write_file(
        temp.path(),
        "trace.log",
        &format!(
            "======== Resolving module '../core/index' from '{}'. ========\n======== Module name '../core/index' was successfully resolved to '{}'. ========\n",
            from.display(),
            to.display()
        ),
    );

    let result = run([
        "specgate",
        "doctor",
        "compare",
        "--project-root",
        temp.path().to_str().expect("utf8 path"),
        "--tsc-trace",
        temp.path().join("trace.log").to_str().expect("utf8 path"),
        "--parser-mode",
        "legacy",
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_RUNTIME_ERROR);
    assert!(result.stdout.contains("beta-only"));
}

#[test]
fn doctor_compare_legacy_parser_mode_succeeds_with_beta_channel() {
    let temp = TempDir::new().expect("tempdir");
    write_basic_project_with_edge(temp.path());
    write_file(
        temp.path(),
        "specgate.config.yml",
        "spec_dirs:\n  - modules\nexclude: []\ntest_patterns: []\nrelease_channel: beta\n",
    );
    let from = temp.path().join("src/app/main.ts");
    let to = temp.path().join("src/core/index.ts");
    write_file(
        temp.path(),
        "trace.log",
        &format!(
            "======== Resolving module '../core/index' from '{}'. ========\n======== Module name '../core/index' was successfully resolved to '{}'. ========\n",
            from.display(),
            to.display()
        ),
    );

    let result = run([
        "specgate",
        "doctor",
        "compare",
        "--project-root",
        temp.path().to_str().expect("utf8 path"),
        "--tsc-trace",
        temp.path().join("trace.log").to_str().expect("utf8 path"),
        "--parser-mode",
        "legacy",
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_PASS);
    assert!(result.stdout.contains("\"status\": \"match\""));
    assert!(
        result
            .stdout
            .contains("\"trace_parser\": \"legacy_trace_text\"")
    );
}

#[test]
fn doctor_compare_writes_structured_snapshot_output() {
    let temp = TempDir::new().expect("tempdir");
    write_basic_project_with_edge(temp.path());
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
      "resolved_to": "src/core/index.ts"
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
        temp.path().to_str().expect("utf8 path"),
        "--structured-snapshot-in",
        temp.path()
            .join("snapshot.json")
            .to_str()
            .expect("utf8 path"),
        "--structured-snapshot-out",
        "snapshots/normalized.json",
        "--parser-mode",
        "structured",
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_PASS);
    assert!(
        result
            .stdout
            .contains("\"trace_parser\": \"structured_snapshot\"")
    );
    assert!(
        result
            .stdout
            .contains("\"structured_snapshot_out\": \"snapshots/normalized.json\"")
    );

    let snapshot = fs::read_to_string(temp.path().join("snapshots/normalized.json"))
        .expect("structured snapshot output");
    let parsed = parse_json(&snapshot);
    assert_eq!(parsed["schema_version"], STRUCTURED_TRACE_SCHEMA_VERSION);
    assert_eq!(parsed["edges"][0]["from"], "src/app/main.ts");
    assert_eq!(parsed["edges"][0]["to"], "src/core/index.ts");
}

#[test]
fn check_output_mode_metrics_includes_timings() {
    let temp = TempDir::new().expect("tempdir");
    write_basic_project(temp.path());

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8 path"),
        "--output-mode",
        "metrics",
        "--no-baseline",
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_PASS);
    assert!(result.stdout.contains("\"metrics\""));
}

#[test]
fn boundary_public_api_uses_provider_constraint_severity() {
    let temp = TempDir::new().expect("tempdir");
    write_basic_project(temp.path());

    write_file(
        temp.path(),
        "modules/app.spec.yml",
        "version: \"2.2\"\nmodule: app\nboundaries:\n  path: src/app/**/*\nconstraints:\n  - rule: boundary.public_api\n    severity: error\n",
    );
    write_file(
        temp.path(),
        "modules/core.spec.yml",
        "version: \"2.2\"\nmodule: core\nboundaries:\n  path: src/core/**/*\n  public_api:\n    - src/core/public/**/*\nconstraints:\n  - rule: boundary.public_api\n    severity: warning\n",
    );
    write_file(
        temp.path(),
        "src/core/internal.ts",
        "export const internal = 1;\n",
    );
    write_file(
        temp.path(),
        "src/app/main.ts",
        "import { internal } from '../core/internal';\nexport const app = internal;\n",
    );

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8 path"),
        "--no-baseline",
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_PASS);
    let output = parse_json(&result.stdout);
    assert_eq!(output["summary"]["new_error_violations"], 0);
    assert_eq!(output["summary"]["new_warning_violations"], 1);
    assert_eq!(output["violations"][0]["rule"], "boundary.public_api");
    assert_eq!(output["violations"][0]["severity"], "warning");
}

#[test]
fn init_creates_scaffold_and_then_skips_existing_files() {
    let temp = TempDir::new().expect("tempdir");

    let first = run([
        "specgate",
        "init",
        "--project-root",
        temp.path().to_str().expect("utf8 path"),
    ]);
    assert_eq!(first.exit_code, EXIT_CODE_PASS);
    assert!(temp.path().join("specgate.config.yml").exists());
    assert!(temp.path().join("modules/app.spec.yml").exists());
    assert!(first.stdout.contains("\"created\""));

    let second = run([
        "specgate",
        "init",
        "--project-root",
        temp.path().to_str().expect("utf8 path"),
    ]);
    assert_eq!(second.exit_code, EXIT_CODE_PASS);
    assert!(second.stdout.contains("\"skipped_existing\""));
    assert!(second.stdout.contains("specgate.config.yml"));
}

#[test]
fn init_scaffold_includes_version_2_3_and_empty_contracts() {
    let temp = TempDir::new().expect("tempdir");

    let result = run([
        "specgate",
        "init",
        "--project-root",
        temp.path().to_str().expect("utf8 path"),
    ]);
    assert_eq!(result.exit_code, EXIT_CODE_PASS);

    let spec_path = temp.path().join("modules/app.spec.yml");
    assert!(spec_path.exists(), "scaffold spec file should exist");

    let spec_content = fs::read_to_string(&spec_path).expect("read scaffold spec");

    // Verify scaffold uses current spec version (2.3)
    assert!(
        spec_content.contains("version: \"2.3\""),
        "scaffold should use CURRENT_SPEC_VERSION (2.3), got: {spec_content}"
    );

    // Verify scaffold includes empty contracts array (new in 2.3)
    assert!(
        spec_content.contains("contracts: []"),
        "scaffold should include empty contracts array, got: {spec_content}"
    );

    // Verify scaffold structure
    assert!(spec_content.contains("module: \"app\""));
    assert!(spec_content.contains("boundaries:"));
    assert!(spec_content.contains("constraints: []"));
}
