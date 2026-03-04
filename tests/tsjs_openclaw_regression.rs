use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::Value;
use tempfile::TempDir;

use specgate::cli::{EXIT_CODE_PASS, EXIT_CODE_POLICY_VIOLATIONS, EXIT_CODE_RUNTIME_ERROR, run};

fn fixture_root(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(relative)
}

fn parse_json(source: &str) -> Value {
    serde_json::from_str(source).expect("valid json")
}

fn copy_dir_recursive(from: &Path, to: &Path) {
    for entry in walkdir::WalkDir::new(from)
        .follow_links(false)
        .into_iter()
        .filter_map(std::result::Result::ok)
    {
        let src_path = entry.path();
        let rel = src_path.strip_prefix(from).expect("strip fixture prefix");
        let dst_path = to.join(rel);

        if entry.file_type().is_dir() {
            fs::create_dir_all(&dst_path).expect("mkdir fixture copy dir");
            continue;
        }

        if let Some(parent) = dst_path.parent() {
            fs::create_dir_all(parent).expect("mkdir fixture copy parent");
        }
        fs::copy(src_path, &dst_path).expect("copy fixture file");
    }
}

fn copy_dir_recursive_excluding(from: &Path, to: &Path, exclude: &[&str]) {
    for entry in walkdir::WalkDir::new(from)
        .follow_links(false)
        .into_iter()
        .filter_map(std::result::Result::ok)
    {
        let src_path = entry.path();
        let rel = src_path.strip_prefix(from).expect("strip prefix");
        let rel_str = rel.to_str().expect("utf8");
        let normalized = rel_str.replace('\\', "/");

        if exclude.iter().any(|exc| normalized.ends_with(exc)) {
            continue;
        }

        let dst_path = to.join(rel);

        if entry.file_type().is_dir() {
            fs::create_dir_all(&dst_path).expect("mkdir");
            continue;
        }

        if let Some(parent) = dst_path.parent() {
            fs::create_dir_all(parent).expect("mkdir parent");
        }
        fs::copy(src_path, &dst_path).expect("copy file");
    }
}

fn write_file(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("mkdir parent");
    }
    fs::write(path, content).expect("write file");
}

#[test]
fn openclaw_scale_init_generates_root_and_workspace_specs() {
    let fixture = fixture_root("openclaw-scale/seed");
    let temp = TempDir::new().expect("tempdir");
    copy_dir_recursive(&fixture, temp.path());

    let result = run([
        "specgate",
        "init",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_PASS);

    let root_spec = fs::read_to_string(temp.path().join("modules/root.spec.yml")).expect("root");
    let web_spec = fs::read_to_string(temp.path().join("modules/web.spec.yml")).expect("web");
    let alpha_spec = fs::read_to_string(temp.path().join("modules/alpha.spec.yml")).expect("alpha");

    assert!(root_spec.contains("path: \"src/**/*\""));
    assert!(web_spec.contains("path: \"packages/web/src/**/*\""));
    assert!(alpha_spec.contains("path: \"extensions/alpha/src/**/*\""));
}

#[test]
fn openclaw_scale_init_discovers_three_workspace_packages() {
    let fixture = fixture_root("openclaw-scale/seed");
    let temp = TempDir::new().expect("tempdir");
    copy_dir_recursive(&fixture, temp.path());

    let result = run([
        "specgate",
        "init",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_PASS);

    let spec_files = fs::read_dir(temp.path().join("modules"))
        .expect("modules dir")
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "yml")
                && path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().ends_with(".spec.yml"))
            {
                Some(())
            } else {
                None
            }
        })
        .count();

    assert!(
        spec_files >= 3,
        "expected at least 3 workspace specs, found {spec_files}"
    );
}

#[test]
fn openclaw_scale_check_is_deterministic_for_alias_dynamic_type_and_barrels() {
    let fixture = fixture_root("openclaw-scale/seed");
    let temp = TempDir::new().expect("tempdir");
    copy_dir_recursive(&fixture, temp.path());

    let init = run([
        "specgate",
        "init",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);
    assert_eq!(init.exit_code, EXIT_CODE_PASS);

    let first = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--no-baseline",
    ]);
    let second = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--no-baseline",
    ]);

    assert_eq!(
        first.exit_code, EXIT_CODE_PASS,
        "first run output: {}",
        first.stdout
    );
    assert_eq!(
        second.exit_code, EXIT_CODE_PASS,
        "second run output: {}",
        second.stdout
    );
    assert_eq!(
        first.stdout, second.stdout,
        "check output must be deterministic"
    );

    let parsed = parse_json(&first.stdout);
    assert_eq!(parsed["status"], "pass");
    assert_eq!(parsed["output_mode"], "deterministic");
}

#[test]
fn openclaw_scale_doctor_compare_matches_structured_snapshot() {
    let fixture = fixture_root("openclaw-scale/seed");
    let temp = TempDir::new().expect("tempdir");
    copy_dir_recursive(&fixture, temp.path());

    let init = run([
        "specgate",
        "init",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);
    assert_eq!(init.exit_code, EXIT_CODE_PASS);

    write_file(
        temp.path(),
        "trace.json",
        r#"{
  "schema_version": "1",
  "edges": [
    { "from": "packages/web/src/app.ts", "to": "packages/web/src/barrel.ts" },
    { "from": "packages/web/src/app.ts", "to": "packages/web/src/lazy.ts" },
    { "from": "packages/web/src/app.ts", "to": "extensions/alpha/src/types.ts" },
    { "from": "packages/web/src/barrel.ts", "to": "packages/web/src/core.ts" },
    { "from": "packages/web/src/barrel.ts", "to": "packages/web/src/types.ts" },
    { "from": "packages/web/src/reexports.ts", "to": "packages/web/src/core.ts" },
    { "from": "packages/web/src/reexports.ts", "to": "packages/web/src/types.ts" },
    { "from": "packages/web/src/reexports.ts", "to": "packages/shared/src/util.ts" },
    { "from": "packages/web/src/core.ts", "to": "packages/shared/src/util.ts" },
    { "from": "packages/shared/src/index.ts", "to": "packages/shared/src/util.ts" },
    { "from": "packages/shared/src/index.ts", "to": "packages/shared/src/types.ts" },
    { "from": "packages/shared/src/violation.ts", "to": "packages/web/src/core.ts" },
    { "from": "packages/shared/src/cycle.ts", "to": "packages/web/src/core.ts" },
    { "from": "extensions/alpha/src/types.ts", "to": "packages/web/src/types.ts" }
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
        "--parser-mode",
        "structured",
        "--structured-snapshot-in",
        temp.path().join("trace.json").to_str().expect("utf8"),
    ]);

    assert_eq!(
        result.exit_code, EXIT_CODE_PASS,
        "doctor output: {}",
        result.stdout
    );
    assert!(result.stdout.contains("\"status\": \"match\""));
    assert!(result.stdout.contains("\"parity_verdict\": \"MATCH\""));
}

#[test]
fn openclaw_scale_cross_package_imports_resolve_without_violations() {
    let fixture = fixture_root("openclaw-scale/seed");
    let temp = TempDir::new().expect("tempdir");
    copy_dir_recursive_excluding(
        &fixture,
        temp.path(),
        &[
            "packages/shared/src/violation.ts",
            "packages/shared/src/cycle.ts",
        ],
    );

    let init = run([
        "specgate",
        "init",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);
    assert_eq!(init.exit_code, EXIT_CODE_PASS);

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--no-baseline",
    ]);

    assert_eq!(
        result.exit_code, EXIT_CODE_PASS,
        "check output: {}",
        result.stdout
    );
}

#[test]
fn openclaw_scale_detects_cross_package_boundary_violation() {
    let fixture = fixture_root("openclaw-scale/seed");
    let temp = TempDir::new().expect("tempdir");
    copy_dir_recursive(&fixture, temp.path());

    let init = run([
        "specgate",
        "init",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);
    assert_eq!(init.exit_code, EXIT_CODE_PASS);

    write_file(
        temp.path(),
        "modules/shared.spec.yml",
        r#"version: "2.3"
module: shared
boundaries:
  path: "packages/shared/src/**/*"
  allow_imports_from: []
constraints: []
"#,
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
        "check output: {}",
        result.stdout
    );
}

#[test]
fn openclaw_scale_reexport_chain_resolves_correctly() {
    let fixture = fixture_root("openclaw-scale/seed");
    let temp = TempDir::new().expect("tempdir");
    copy_dir_recursive_excluding(
        &fixture,
        temp.path(),
        &[
            "packages/shared/src/violation.ts",
            "packages/shared/src/cycle.ts",
        ],
    );

    let init = run([
        "specgate",
        "init",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);
    assert_eq!(init.exit_code, EXIT_CODE_PASS);

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--no-baseline",
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_PASS);

    let file_count = walkdir::WalkDir::new(temp.path())
        .into_iter()
        .filter_map(std::result::Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .filter(|entry| {
            matches!(
                entry.path().extension().and_then(|ext| ext.to_str()),
                Some("ts") | Some("tsx") | Some("js") | Some("jsx")
            )
        })
        .count();

    write_file(
        temp.path(),
        "trace.json",
        r#"{
  "schema_version": "1",
  "edges": [
    { "from": "packages/web/src/app.ts", "to": "extensions/alpha/src/types.ts" },
    { "from": "packages/web/src/app.ts", "to": "packages/web/src/barrel.ts" },
    { "from": "packages/web/src/app.ts", "to": "packages/web/src/lazy.ts" },
    { "from": "packages/web/src/barrel.ts", "to": "packages/web/src/core.ts" },
    { "from": "packages/web/src/barrel.ts", "to": "packages/web/src/types.ts" },
    { "from": "packages/shared/src/index.ts", "to": "packages/shared/src/types.ts" },
    { "from": "packages/shared/src/index.ts", "to": "packages/shared/src/util.ts" },
    { "from": "packages/web/src/core.ts", "to": "packages/shared/src/util.ts" },
    { "from": "packages/web/src/reexports.ts", "to": "packages/shared/src/util.ts" },
    { "from": "packages/web/src/reexports.ts", "to": "packages/web/src/core.ts" },
    { "from": "packages/web/src/reexports.ts", "to": "packages/web/src/types.ts" },
    { "from": "extensions/alpha/src/types.ts", "to": "packages/web/src/types.ts" }
  ]
}"#,
    );

    let doctor = run([
        "specgate",
        "doctor",
        "compare",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--parser-mode",
        "structured",
        "--structured-snapshot-in",
        temp.path().join("trace.json").to_str().expect("utf8"),
    ]);

    assert_eq!(
        doctor.exit_code, EXIT_CODE_PASS,
        "doctor compare output: {}",
        doctor.stdout
    );

    let doctor_json = parse_json(&doctor.stdout);
    let edge_count = doctor_json["specgate_edge_count"]
        .as_u64()
        .expect("specgate_edge_count");

    assert!(
        edge_count < (file_count * 10) as u64,
        "edge explosion detected: {edge_count} edges over {file_count} files"
    );
}

#[test]
fn openclaw_scale_circular_dependency_does_not_hang() {
    let fixture = fixture_root("openclaw-scale/seed");
    let temp = TempDir::new().expect("tempdir");
    copy_dir_recursive_excluding(&fixture, temp.path(), &["packages/shared/src/violation.ts"]);

    let init = run([
        "specgate",
        "init",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);
    assert_eq!(init.exit_code, EXIT_CODE_PASS);

    let started = Instant::now();
    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--no-baseline",
    ]);
    let elapsed = started.elapsed();

    assert!(
        elapsed < Duration::from_secs(10),
        "check took {}s",
        elapsed.as_secs()
    );
    assert_ne!(result.exit_code, EXIT_CODE_RUNTIME_ERROR);
}

#[test]
fn openclaw_scale_verdict_includes_workspace_packages() {
    let fixture = fixture_root("openclaw-scale/seed");
    let temp = TempDir::new().expect("tempdir");
    copy_dir_recursive_excluding(
        &fixture,
        temp.path(),
        &[
            "packages/shared/src/violation.ts",
            "packages/shared/src/cycle.ts",
        ],
    );

    let init = run([
        "specgate",
        "init",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);
    assert_eq!(init.exit_code, EXIT_CODE_PASS);

    let result = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--no-baseline",
    ]);
    assert_eq!(result.exit_code, EXIT_CODE_PASS);

    let parsed = parse_json(&result.stdout);
    let workspace_packages = parsed["workspace_packages"]
        .as_array()
        .expect("workspace_packages array");
    let values: Vec<_> = workspace_packages
        .iter()
        .filter_map(|value| value.get("name").and_then(|name| name.as_str()))
        .collect();

    assert!(values.contains(&"web"), "missing web workspace package");
    assert!(values.contains(&"alpha"), "missing alpha workspace package");
}
