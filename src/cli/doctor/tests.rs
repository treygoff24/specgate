use std::fs;
use std::path::Path;

use tempfile::TempDir;

use super::trace_parser::parse_structured_trace_data;
use crate::cli::{EXIT_CODE_PASS, run};

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
