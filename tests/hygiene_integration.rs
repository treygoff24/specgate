use std::fs;
use std::path::Path;

use serde_json::Value;
use tempfile::TempDir;

use specgate::cli::{EXIT_CODE_PASS, EXIT_CODE_POLICY_VIOLATIONS, run};
use specgate::graph::DependencyGraph;
use specgate::resolver::ModuleResolver;
use specgate::rules::{RuleContext, evaluate_hygiene_rules};
use specgate::spec::config::{ImportHygieneConfig, TestBoundaryConfig, TestBoundaryMode};
use specgate::spec::{Boundaries, SpecConfig, SpecFile};

fn write_file(root: &Path, relative_path: &str, contents: &str) {
    let path = root.join(relative_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("mkdir parent");
    }
    fs::write(path, contents).expect("write file");
}

fn parse_json(stdout: &str) -> Value {
    serde_json::from_str(stdout).expect("parse json")
}

fn spec_with_boundaries(module: &str, path: &str, boundaries: Boundaries) -> SpecFile {
    SpecFile {
        version: "2.3".to_string(),
        module: module.to_string(),
        package: None,
        import_id: None,
        import_ids: Vec::new(),
        description: None,
        boundaries: Some(Boundaries {
            path: Some(path.to_string()),
            ..boundaries
        }),
        constraints: Vec::new(),
        spec_path: None,
    }
}

fn write_base_config(root: &Path, import_hygiene: &str) {
    write_file(
        root,
        "specgate.config.yml",
        &format!(
            "spec_dirs:\n  - modules\nexclude: []\ntest_patterns:\n  - \"**/*.test.ts\"\n  - \"**/__tests__/**\"\n{import_hygiene}\n"
        ),
    );
}

#[test]
fn deep_import_default_severity_is_warning() {
    let temp = TempDir::new().expect("tempdir");
    write_base_config(
        temp.path(),
        r#"import_hygiene:
  deny_deep_imports:
    - pattern: lodash/**
      max_depth: 1"#,
    );
    write_file(
        temp.path(),
        "modules/app.spec.yml",
        "version: \"2.3\"\nmodule: app\nboundaries:\n  path: src/app/**/*\nconstraints: []\n",
    );
    write_file(
        temp.path(),
        "src/app/main.ts",
        "import { value } from 'lodash/internal/deep';\nexport const x = value;\n",
    );

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--no-baseline",
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_PASS, "{}", result.stdout);
    let output = parse_json(&result.stdout);
    let violation = output["violations"]
        .as_array()
        .expect("violations")
        .iter()
        .find(|violation| violation["rule"] == "hygiene.deep_third_party_import")
        .expect("deep import violation");
    assert_eq!(violation["severity"], "warning");
}

#[test]
fn deep_import_severity_can_escalate_to_error() {
    let temp = TempDir::new().expect("tempdir");
    write_base_config(
        temp.path(),
        r#"import_hygiene:
  deny_deep_imports:
    - pattern: lodash/**
      max_depth: 1
      severity: error"#,
    );
    write_file(
        temp.path(),
        "modules/app.spec.yml",
        "version: \"2.3\"\nmodule: app\nboundaries:\n  path: src/app/**/*\nconstraints: []\n",
    );
    write_file(
        temp.path(),
        "src/app/main.ts",
        "import { value } from 'lodash/internal/deep';\nexport const x = value;\n",
    );

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--no-baseline",
    ]);

    assert_eq!(
        result.exit_code, EXIT_CODE_POLICY_VIOLATIONS,
        "{}",
        result.stdout
    );
    let output = parse_json(&result.stdout);
    let violation = output["violations"]
        .as_array()
        .expect("violations")
        .iter()
        .find(|violation| violation["rule"] == "hygiene.deep_third_party_import")
        .expect("deep import violation");
    assert_eq!(violation["severity"], "error");
}

#[test]
fn module_overrides_can_allow_and_tighten_deep_import_rules() {
    let temp = TempDir::new().expect("tempdir");
    write_base_config(
        temp.path(),
        r#"import_hygiene:
  deny_deep_imports:
    - pattern: internal-sdk/**
      severity: error
    - pattern: lodash/**
      max_depth: 2
      severity: error"#,
    );
    write_file(
        temp.path(),
        "modules/app.spec.yml",
        r#"version: "2.3"
module: app
boundaries:
  path: src/app/**/*
  import_hygiene:
    deny_deep_imports:
      - pattern: internal-sdk/**
        allow: true
      - pattern: lodash/**
        max_depth: 0
constraints: []
"#,
    );
    write_file(
        temp.path(),
        "src/app/main.ts",
        "import { allowed } from 'internal-sdk/testing';\nimport { fp } from 'lodash/fp';\nexport const x = allowed ?? fp;\n",
    );

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--no-baseline",
    ]);

    assert_eq!(
        result.exit_code, EXIT_CODE_POLICY_VIOLATIONS,
        "{}",
        result.stdout
    );
    let output = parse_json(&result.stdout);
    let deep_imports: Vec<_> = output["violations"]
        .as_array()
        .expect("violations")
        .iter()
        .filter(|violation| violation["rule"] == "hygiene.deep_third_party_import")
        .collect();
    assert_eq!(deep_imports.len(), 1, "{output:?}");
    assert!(
        deep_imports[0]["message"]
            .as_str()
            .unwrap()
            .contains("lodash/fp")
    );
}

#[test]
fn bidirectional_mode_blocks_cross_module_test_internal_imports() {
    let temp = TempDir::new().expect("tempdir");
    write_file(
        temp.path(),
        "src/app/main.test.ts",
        "import { secret } from '../core/internal';\nexport const x = secret;\n",
    );
    write_file(
        temp.path(),
        "src/core/internal.ts",
        "export const secret = 1;\n",
    );
    write_file(temp.path(), "src/core/index.ts", "export const api = 1;\n");

    let specs = vec![
        spec_with_boundaries("app", "src/app/**/*", Boundaries::default()),
        spec_with_boundaries(
            "core",
            "src/core/**/*",
            Boundaries {
                public_api: vec!["src/core/index.ts".to_string()],
                ..Boundaries::default()
            },
        ),
    ];
    let config = SpecConfig {
        import_hygiene: ImportHygieneConfig {
            test_boundary: TestBoundaryConfig {
                enabled: true,
                mode: TestBoundaryMode::Bidirectional,
                test_patterns: Vec::new(),
            },
            ..ImportHygieneConfig::default()
        },
        ..SpecConfig::default()
    };
    let mut resolver = ModuleResolver::new(temp.path(), &specs).expect("resolver");
    let graph = DependencyGraph::build(temp.path(), &mut resolver, &config).expect("graph");
    let ctx = RuleContext {
        project_root: temp.path(),
        config: &config,
        specs: &specs,
        graph: &graph,
    };

    let violations = evaluate_hygiene_rules(&ctx);
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert_eq!(violations[0].rule, "hygiene.test_in_production");
    assert!(violations[0].message.contains("non-public file"));
}

#[test]
fn production_only_mode_flags_prod_to_test_but_not_test_to_internal() {
    let temp = TempDir::new().expect("tempdir");
    write_base_config(
        temp.path(),
        r#"import_hygiene:
  test_boundary:
    enabled: true
    mode: production_only"#,
    );
    write_file(
        temp.path(),
        "modules/app.spec.yml",
        "version: \"2.3\"\nmodule: app\nboundaries:\n  path: src/app/**/*\nconstraints: []\n",
    );
    write_file(
        temp.path(),
        "modules/core.spec.yml",
        "version: \"2.3\"\nmodule: core\nboundaries:\n  path: src/core/**/*\nconstraints: []\n",
    );
    write_file(
        temp.path(),
        "src/app/main.ts",
        "import { helper } from '../core/helper.test';\nexport const x = helper;\n",
    );
    write_file(
        temp.path(),
        "src/app/main.test.ts",
        "import { secret } from '../core/internal';\nexport const y = secret;\n",
    );
    write_file(
        temp.path(),
        "src/core/helper.test.ts",
        "export const helper = 1;\n",
    );
    write_file(
        temp.path(),
        "src/core/internal.ts",
        "export const secret = 1;\n",
    );
    write_file(temp.path(), "src/core/index.ts", "export const api = 1;\n");

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--no-baseline",
    ]);

    assert_eq!(
        result.exit_code, EXIT_CODE_POLICY_VIOLATIONS,
        "{}",
        result.stdout
    );
    let output = parse_json(&result.stdout);
    let hygiene_violations: Vec<_> = output["violations"]
        .as_array()
        .expect("violations")
        .iter()
        .filter(|violation| violation["rule"] == "hygiene.test_in_production")
        .collect();
    assert_eq!(hygiene_violations.len(), 1, "{output:?}");
    assert!(
        hygiene_violations[0]["message"]
            .as_str()
            .unwrap()
            .contains("test file")
    );
}

#[test]
fn module_level_off_override_disables_test_boundary_checks() {
    let temp = TempDir::new().expect("tempdir");
    write_base_config(
        temp.path(),
        r#"import_hygiene:
  test_boundary:
    enabled: true
    mode: production_only"#,
    );
    write_file(
        temp.path(),
        "modules/app.spec.yml",
        r#"version: "2.3"
module: app
boundaries:
  path: src/app/**/*
  import_hygiene:
    test_boundary:
      mode: off
constraints: []
"#,
    );
    write_file(
        temp.path(),
        "modules/core.spec.yml",
        "version: \"2.3\"\nmodule: core\nboundaries:\n  path: src/core/**/*\nconstraints: []\n",
    );
    write_file(
        temp.path(),
        "src/app/main.ts",
        "import { helper } from '../core/helper.test';\nexport const x = helper;\n",
    );
    write_file(
        temp.path(),
        "src/core/helper.test.ts",
        "export const helper = 1;\n",
    );

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--no-baseline",
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_PASS, "{}", result.stdout);
    let output = parse_json(&result.stdout);
    assert!(
        output["violations"]
            .as_array()
            .expect("violations")
            .is_empty()
    );
}

#[test]
fn doctor_reports_canonical_import_dangling() {
    let temp = TempDir::new().expect("tempdir");
    write_base_config(temp.path(), "");
    write_file(
        temp.path(),
        "tsconfig.json",
        r#"{
  "compilerOptions": {
    "baseUrl": ".",
    "paths": {
      "@app/core": ["src/core/internal.ts"]
    }
  }
}
"#,
    );
    write_file(
        temp.path(),
        "modules/app.spec.yml",
        "version: \"2.3\"\nmodule: app\nboundaries:\n  path: src/app/**/*\nconstraints: []\n",
    );
    write_file(
        temp.path(),
        "modules/core.spec.yml",
        "version: \"2.3\"\nmodule: core\nimport_id: \"@app/core\"\nboundaries:\n  path: src/core/**/*\n  public_api:\n    - src/core/index.ts\n  enforce_canonical_imports: true\nconstraints: []\n",
    );
    write_file(temp.path(), "src/app/main.ts", "export const app = 1;\n");
    write_file(temp.path(), "src/core/index.ts", "export const api = 1;\n");
    write_file(
        temp.path(),
        "src/core/internal.ts",
        "export const secret = 1;\n",
    );

    let result = run([
        "specgate",
        "doctor",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_PASS, "{}", result.stdout);
    let output = parse_json(&result.stdout);
    let finding = output["findings"]
        .as_array()
        .expect("findings")
        .iter()
        .find(|finding| finding["rule"] == "boundary.canonical_import_dangling")
        .expect("dangling canonical finding");
    assert_eq!(finding["severity"], "warning");
    assert!(
        finding["message"]
            .as_str()
            .unwrap()
            .contains("src/core/internal.ts")
    );
}
