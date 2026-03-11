use tempfile::TempDir;

use super::trace_parser::parse_structured_trace_data;
use crate::cli::test_support::{write_basic_project_with_edge, write_file};
use crate::cli::{EXIT_CODE_PASS, run};

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
