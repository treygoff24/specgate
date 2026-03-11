use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use tempfile::TempDir;

use specgate::cli::{EXIT_CODE_PASS, EXIT_CODE_POLICY_VIOLATIONS, run};
use specgate::graph::{EdgeKind, EdgeType, UnresolvedImportRecord};

fn parse_json(source: &str) -> Value {
    serde_json::from_str(source).expect("valid json")
}

fn write_file(root: &Path, relative_path: &str, content: &str) {
    let path = root.join(relative_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("mkdir parent");
    }
    fs::write(path, content).expect("write file");
}

fn setup_project(policy: &str) -> TempDir {
    let temp = TempDir::new().expect("tempdir");

    write_file(
        temp.path(),
        "specgate.config.yml",
        &format!("spec_version: \"2.3\"\nunresolved_edge_policy: {policy}\n"),
    );

    write_file(
        temp.path(),
        "modules/app/app.spec.yml",
        r#"version: "2.3"
module: app
boundaries:
  path: src/app/**
"#,
    );
    write_file(temp.path(), "src/app/util.ts", "export const util = 1;\n");
    write_file(
        temp.path(),
        "src/app/types.ts",
        "export type UtilType = number;\n",
    );
    write_file(
        temp.path(),
        "src/app/main.ts",
        "import { util } from './util';\nimport type { UtilType } from './types';\nimport fs from 'node:fs';\nimport missing from './missing';\n\nexport async function loadMissing(): Promise<unknown> {\n  return import('./lazy-missing');\n}\n\nexport const value: UtilType = util;\nexport const external = fs;\nexport const unresolved = missing;\n",
    );

    temp
}

fn run_check_json(root: &Path, extra_args: &[&str]) -> (specgate::cli::CliRunResult, Value) {
    let mut args = vec![
        "specgate",
        "check",
        "--project-root",
        root.to_str().expect("utf8"),
        "--format",
        "json",
    ];
    args.extend_from_slice(extra_args);

    let result = run(args);
    let json = parse_json(&result.stdout);
    (result, json)
}

#[test]
fn edge_type_enum_exists() {
    let _resolved = EdgeType::Resolved;
    let _literal = EdgeType::UnresolvedLiteral;
    let _dynamic = EdgeType::UnresolvedDynamic;
    let _external = EdgeType::External;
}

#[test]
fn unresolved_import_record_derives_edge_type() {
    let from = PathBuf::from("src/app/main.ts");

    let external = UnresolvedImportRecord {
        from: from.clone(),
        specifier: "react".to_string(),
        kind: EdgeKind::RuntimeImport,
        line: Some(1),
        is_external: true,
        ignored_by_comment: false,
    };
    assert_eq!(external.edge_type(), EdgeType::External);

    let dynamic = UnresolvedImportRecord {
        from: from.clone(),
        specifier: "./lazy-missing".to_string(),
        kind: EdgeKind::DynamicImport,
        line: Some(2),
        is_external: false,
        ignored_by_comment: false,
    };
    assert_eq!(dynamic.edge_type(), EdgeType::UnresolvedDynamic);

    let literal = UnresolvedImportRecord {
        from,
        specifier: "./missing".to_string(),
        kind: EdgeKind::RuntimeImport,
        line: Some(3),
        is_external: false,
        ignored_by_comment: false,
    };
    assert_eq!(literal.edge_type(), EdgeType::UnresolvedLiteral);
}

#[test]
fn unresolved_edge_policy_matrix_emits_hygiene_findings() {
    for (policy, exit_code, status, severity, count) in [
        ("warn", EXIT_CODE_PASS, "pass", Some("warning"), 2usize),
        (
            "error",
            EXIT_CODE_POLICY_VIOLATIONS,
            "fail",
            Some("error"),
            2usize,
        ),
        ("ignore", EXIT_CODE_PASS, "pass", None, 0usize),
    ] {
        let temp = setup_project(policy);
        let (result, verdict) = run_check_json(temp.path(), &["--no-baseline"]);

        assert_eq!(result.exit_code, exit_code, "policy {policy}");
        assert_eq!(verdict["status"], status, "policy {policy}");

        let unresolved = verdict["violations"]
            .as_array()
            .expect("violations array")
            .iter()
            .filter(|violation| violation["rule"] == "hygiene.unresolved_import")
            .collect::<Vec<_>>();

        assert_eq!(unresolved.len(), count, "policy {policy}");
        assert!(
            verdict["violations"]
                .as_array()
                .expect("violations array")
                .iter()
                .all(|violation| violation["rule"] != "edge.unresolved"),
            "legacy alias should not be emitted"
        );

        if let Some(expected_severity) = severity {
            assert!(
                unresolved
                    .iter()
                    .all(|violation| violation["severity"] == expected_severity)
            );
        }

        assert!(
            unresolved
                .iter()
                .all(|violation| violation["actual"] != "node:fs"),
            "external imports should not emit hygiene.unresolved_import findings"
        );
    }
}

#[test]
fn verdict_includes_per_edge_detail() {
    let temp = setup_project("warn");
    let (result, verdict) = run_check_json(temp.path(), &["--no-baseline"]);

    assert_eq!(result.exit_code, EXIT_CODE_PASS);
    assert_eq!(verdict["edge_classification"]["resolved"], 1);
    assert_eq!(verdict["edge_classification"]["type_only"], 1);
    assert_eq!(verdict["edge_classification"]["external"], 1);
    assert_eq!(verdict["edge_classification"]["unresolved_literal"], 1);
    assert_eq!(verdict["edge_classification"]["unresolved_dynamic"], 1);

    let edges = verdict["edges"].as_array().expect("edges array");
    assert_eq!(edges.len(), 4, "type-only edges stay in summary only");

    let edge_types = edges
        .iter()
        .map(|edge| edge["edge_type"].as_str().expect("edge_type"))
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        edge_types,
        std::collections::BTreeSet::from([
            "resolved",
            "external",
            "unresolved_dynamic",
            "unresolved_literal",
        ])
    );

    let resolved = edges
        .iter()
        .find(|edge| edge["edge_type"] == "resolved")
        .expect("resolved edge");
    assert_eq!(resolved["from_module"], "app");
    assert_eq!(resolved["to_module"], "app");
    assert_eq!(resolved["import_path"], "./util");
    assert_eq!(resolved["file"], "src/app/main.ts");

    let external = edges
        .iter()
        .find(|edge| edge["edge_type"] == "external")
        .expect("external edge");
    assert!(external["to_module"].is_null());
    assert_eq!(external["import_path"], "node:fs");

    let unresolved_dynamic = edges
        .iter()
        .find(|edge| edge["edge_type"] == "unresolved_dynamic")
        .expect("dynamic edge");
    assert_eq!(unresolved_dynamic["import_path"], "./lazy-missing");
}

#[test]
fn sarif_includes_edge_type_property_for_unresolved_imports() {
    let temp = setup_project("error");
    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--format",
        "sarif",
        "--no-baseline",
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_POLICY_VIOLATIONS);

    let sarif = parse_json(&result.stdout);
    let results = sarif["runs"][0]["results"]
        .as_array()
        .expect("sarif results");

    let unresolved = results
        .iter()
        .find(|entry| entry["ruleId"] == "hygiene.unresolved_import")
        .expect("unresolved import result");
    let edge_type = unresolved["properties"]["edge_type"]
        .as_str()
        .expect("edge_type property");
    assert!(
        edge_type == "unresolved_literal" || edge_type == "unresolved_dynamic",
        "unexpected edge type {edge_type}"
    );
}

#[test]
fn legacy_edge_unresolved_baseline_alias_matches_new_rule() {
    let temp = TempDir::new().expect("tempdir");
    write_file(
        temp.path(),
        "specgate.config.yml",
        "spec_version: \"2.3\"\nunresolved_edge_policy: error\n",
    );
    write_file(
        temp.path(),
        "modules/app/app.spec.yml",
        r#"version: "2.3"
module: app
boundaries:
  path: src/app/**
"#,
    );
    write_file(
        temp.path(),
        "src/app/main.ts",
        "import missing from './missing';\nexport const unresolved = missing;\n",
    );
    write_file(
        temp.path(),
        ".specgate-baseline.json",
        r#"{
  "version": "1",
  "entries": [
    {
      "fingerprint": "legacy-edge-unresolved",
      "rule": "edge.unresolved",
      "severity": "error",
      "message": "unresolved import: './missing'",
      "from_file": "src/app/main.ts",
      "from_module": "app"
    }
  ]
}
"#,
    );

    let (result, verdict) = run_check_json(temp.path(), &[]);

    assert_eq!(result.exit_code, EXIT_CODE_PASS);
    assert_eq!(verdict["status"], "pass");

    let violations = verdict["violations"].as_array().expect("violations array");
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0]["rule"], "hygiene.unresolved_import");
    assert_eq!(violations[0]["disposition"], "baseline");
}

#[test]
fn ignored_unresolved_imports_stay_out_of_verdict_counts_and_findings() {
    let temp = TempDir::new().expect("tempdir");
    write_file(
        temp.path(),
        "specgate.config.yml",
        "spec_version: \"2.3\"\nunresolved_edge_policy: error\n",
    );
    write_file(
        temp.path(),
        "modules/app/app.spec.yml",
        r#"version: "2.3"
module: app
boundaries:
  path: src/app/**
"#,
    );
    write_file(
        temp.path(),
        "src/app/main.ts",
        "// @specgate-ignore: tracked elsewhere\nimport ignored from './ignored-missing';\nimport reported from './reported-missing';\n\nexport const values = [ignored, reported];\n",
    );

    let (result, verdict) = run_check_json(temp.path(), &["--no-baseline"]);

    assert_eq!(result.exit_code, EXIT_CODE_POLICY_VIOLATIONS);
    assert_eq!(verdict["edge_classification"]["unresolved_literal"], 1);
    assert_eq!(verdict["edge_classification"]["unresolved_dynamic"], 0);

    let violations = verdict["violations"].as_array().expect("violations array");
    let unresolved = violations
        .iter()
        .filter(|violation| violation["rule"] == "hygiene.unresolved_import")
        .collect::<Vec<_>>();
    assert_eq!(unresolved.len(), 1, "{verdict:#}");
    assert_eq!(unresolved[0]["actual"], "./reported-missing");

    let edges = verdict["edges"].as_array().expect("edges array");
    assert!(
        edges
            .iter()
            .all(|edge| edge["import_path"] != "./ignored-missing"),
        "ignored unresolved import should stay out of edge detail"
    );
}
