use tempfile::TempDir;

use super::trace_parser::parse_structured_trace_data;
use crate::cli::test_support::{write_basic_project_with_edge, write_file};
use crate::cli::{EXIT_CODE_PASS, run};
use serde_json::Value;

fn parse_json(source: &str) -> Value {
    serde_json::from_str(source).expect("valid json")
}

#[test]
fn doctor_compare_auto_mode_scans_nested_json_trace_payload() {
    let temp = TempDir::new().expect("tempdir");
    write_basic_project_with_edge(temp.path());
    write_file(
        temp.path(),
        "nested-trace.json",
        r#"{
  "metadata": {
    "source": "tsc"
  },
  "payload": {
    "trace": {
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
  }
}
"#,
    );

    let result = run([
        "specgate",
        "doctor",
        "compare",
        "--project-root",
        temp.path().to_str().expect("utf8 path"),
        "--tsc-trace",
        temp.path()
            .join("nested-trace.json")
            .to_str()
            .expect("utf8 path"),
        "--parser-mode",
        "auto",
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_PASS);
    assert!(result.stdout.contains("\"status\": \"match\""));
    assert!(result.stdout.contains("\"trace_edge_count\": 1"));
}

#[test]
fn parse_structured_snapshot_keeps_schema_version_validation() {
    let temp = TempDir::new().expect("tempdir");
    let error = parse_structured_trace_data(
        temp.path(),
        r#"{
  "schema_version": "999",
  "edges": [],
  "resolutions": []
}
"#,
    )
    .expect_err("unsupported schema version should fail");

    assert!(error.contains("schema_version '999' is not supported"));
}

#[test]
fn doctor_compare_without_trace_payload_serializes_skipped_status() {
    let temp = TempDir::new().expect("tempdir");
    write_basic_project_with_edge(temp.path());

    let result = run([
        "specgate",
        "doctor",
        "compare",
        "--project-root",
        temp.path().to_str().expect("utf8 path"),
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_PASS);
    let output = parse_json(&result.stdout);
    assert_eq!(output["status"], "skipped");
    assert_eq!(output["parity_verdict"], "SKIPPED");
    assert!(
        output["mismatch_category"].is_null(),
        "mismatch category must be absent for skipped compares"
    );
}

#[test]
fn doctor_overview_canonical_warning_calls_out_representative_probe() {
    let temp = TempDir::new().expect("tempdir");
    write_file(
        temp.path(),
        "modules/app.spec.yml",
        "version: \"2.2\"\nmodule: app\nboundaries:\n  path: src/app/**/*\nconstraints: []\n",
    );
    write_file(
        temp.path(),
        "modules/core.spec.yml",
        "version: \"2.2\"\nmodule: core\nimport_id: \"@app/core\"\nboundaries:\n  path: src/core/**/*\n  enforce_canonical_imports: true\nconstraints:\n  - rule: boundary.canonical_imports\n    severity: warning\n",
    );
    write_file(
        temp.path(),
        "specgate.config.yml",
        "spec_dirs:\n  - modules\nexclude: []\ntest_patterns: []\n",
    );
    write_file(temp.path(), "src/app/main.ts", "export const app = 1;\n");
    write_file(
        temp.path(),
        "src/core/internal.ts",
        "export const hidden = 1;\n",
    );

    let result = run([
        "specgate",
        "doctor",
        "--project-root",
        temp.path().to_str().expect("utf8 path"),
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_PASS);
    let output = parse_json(&result.stdout);
    let findings = output["findings"].as_array().expect("findings array");
    let finding = findings
        .iter()
        .find(|finding| finding["rule"] == "boundary.canonical_import_dangling")
        .expect("canonical dangling finding");

    let message = finding["message"].as_str().expect("message string");
    assert!(
        message.contains("representative importer probe"),
        "warning should disclose probe-based uncertainty: {message}"
    );
}
