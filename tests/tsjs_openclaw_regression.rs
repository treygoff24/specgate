use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use tempfile::TempDir;

use specgate::cli::{EXIT_CODE_PASS, run};

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
    { "from": "packages/web/src/app.ts", "to": "src/plugin-sdk/index.ts" },
    { "from": "packages/web/src/app.ts", "to": "packages/web/src/barrel.ts" },
    { "from": "packages/web/src/app.ts", "to": "packages/web/src/lazy.ts" },
    { "from": "packages/web/src/app.ts", "to": "extensions/alpha/src/types.ts" },
    { "from": "packages/web/src/barrel.ts", "to": "packages/web/src/core.ts" },
    { "from": "packages/web/src/barrel.ts", "to": "packages/web/src/types.ts" },
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
