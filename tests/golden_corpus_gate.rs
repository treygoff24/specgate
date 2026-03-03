//! Golden corpus gate-intended fixtures.
//!
//! These tests are catchable-now and intended to fail CI when regressed.
//! Informational fixtures live in `tests/golden_corpus.rs`.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use tempfile::TempDir;

use specgate::cli::{EXIT_CODE_PASS, EXIT_CODE_POLICY_VIOLATIONS, run};
use specgate::spec::{load_spec, validate_specs};

#[derive(Debug, Clone, Copy)]
struct ViolationIdentity<'a> {
    rule: &'a str,
    from_module: &'a str,
    to_module: Option<&'a str>,
    from_file: &'a str,
    to_file: Option<&'a str>,
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("golden")
}

fn copy_files(src_dir: &Path, dest_dir: &Path) {
    fs::create_dir_all(dest_dir).expect("create dest dir");
    for entry in fs::read_dir(src_dir).expect("read src dir") {
        let entry = entry.expect("dir entry");
        let src_path = entry.path();
        let dest_path = dest_dir.join(entry.file_name());

        if src_path.is_dir() {
            copy_files(&src_path, &dest_path);
        } else {
            fs::copy(&src_path, &dest_path).expect("copy file");
        }
    }
}

fn seed_stubbed_node_modules(project_root: &Path) {
    let package_json_path = project_root.join("package.json");
    if !package_json_path.exists() {
        return;
    }

    let raw = fs::read_to_string(&package_json_path).expect("read package.json");
    let parsed: Value = serde_json::from_str(&raw).expect("parse package.json");

    let node_modules = project_root.join("node_modules");
    fs::create_dir_all(&node_modules).expect("create node_modules");

    for section in ["dependencies", "devDependencies"] {
        let Some(table) = parsed.get(section).and_then(Value::as_object) else {
            continue;
        };

        for package_name in table.keys() {
            let package_dir = node_modules.join(package_name);
            fs::create_dir_all(&package_dir).expect("create package dir");

            let package_manifest = json!({
                "name": package_name,
                "version": "0.0.0-test",
                "main": "index.js"
            });
            fs::write(
                package_dir.join("package.json"),
                serde_json::to_string_pretty(&package_manifest)
                    .expect("serialize package manifest"),
            )
            .expect("write stub package manifest");

            fs::write(package_dir.join("index.js"), "module.exports = {};\n")
                .expect("write stub package index");
        }
    }
}

fn stage_dependency_fixture(
    fixture_id: &str,
    source_filename: &str,
    target_filename: &str,
) -> TempDir {
    let temp = TempDir::new().expect("tempdir");
    let fixture_root = fixtures_dir().join(fixture_id);

    copy_files(&fixture_root.join("modules"), &temp.path().join("modules"));

    fs::create_dir_all(temp.path().join("src")).expect("create src");
    fs::copy(
        fixture_root.join("src").join(source_filename),
        temp.path().join("src").join(target_filename),
    )
    .expect("copy source file");

    fs::copy(
        fixture_root.join("specgate.config.yml"),
        temp.path().join("specgate.config.yml"),
    )
    .expect("copy config");

    if fixture_root.join("package.json").exists() {
        fs::copy(
            fixture_root.join("package.json"),
            temp.path().join("package.json"),
        )
        .expect("copy package.json");
        seed_stubbed_node_modules(temp.path());
    }

    temp
}

fn run_check(project_root: &Path) -> (specgate::cli::CliRunResult, Value) {
    let result = run([
        "specgate",
        "check",
        "--project-root",
        project_root.to_str().expect("utf8 path"),
        "--no-baseline",
    ]);

    let verdict = serde_json::from_str::<Value>(&result.stdout).unwrap_or_else(|error| {
        panic!(
            "expected JSON verdict (error: {error}); stdout={}, stderr={}",
            result.stdout, result.stderr
        )
    });

    (result, verdict)
}

fn assert_status(verdict: &Value, expected: &str) {
    assert_eq!(
        verdict["status"],
        Value::String(expected.to_string()),
        "unexpected verdict status: {verdict:#}"
    );
}

fn assert_single_violation_identity(verdict: &Value, expected: ViolationIdentity<'_>) {
    let violations = verdict["violations"].as_array().expect("violations array");
    assert_eq!(
        violations.len(),
        1,
        "expected one violation; got {} in verdict: {verdict:#}",
        violations.len()
    );

    let violation = &violations[0];
    assert_eq!(violation["rule"], expected.rule);
    assert_eq!(violation["from_module"], expected.from_module);

    match expected.to_module {
        Some(to_module) => assert_eq!(violation["to_module"], to_module),
        None => assert_eq!(violation["to_module"], Value::Null),
    }

    assert_eq!(violation["from_file"], expected.from_file);

    match expected.to_file {
        Some(to_file) => assert_eq!(violation["to_file"], to_file),
        None => assert_eq!(violation["to_file"], Value::Null),
    }
}

// =============================================================================
// D01: Forbidden Third-Party Dependency
// Status: ✅ Direct Detection - dependency.forbidden rule catches this
// =============================================================================

#[test]
fn d01_forbidden_third_party_intro_fails_gate_contract() {
    let temp = stage_dependency_fixture("d01-forbidden-third-party", "utils-intro.ts", "utils.ts");

    let (result, verdict) = run_check(temp.path());

    assert_eq!(
        result.exit_code, EXIT_CODE_POLICY_VIOLATIONS,
        "D01 intro should fail: stdout={}, stderr={}",
        result.stdout, result.stderr
    );
    assert_status(&verdict, "fail");
    assert_single_violation_identity(
        &verdict,
        ViolationIdentity {
            rule: "dependency.forbidden",
            from_module: "utils",
            to_module: None,
            from_file: "src/utils.ts",
            to_file: None,
        },
    );
}

#[test]
fn d01_forbidden_third_party_fix_passes_gate_contract() {
    let temp = stage_dependency_fixture("d01-forbidden-third-party", "utils-fix.ts", "utils.ts");

    let (result, verdict) = run_check(temp.path());

    assert_eq!(
        result.exit_code, EXIT_CODE_PASS,
        "D01 fix should pass: stdout={}, stderr={}",
        result.stdout, result.stderr
    );
    assert_status(&verdict, "pass");
    assert_eq!(
        verdict["violations"]
            .as_array()
            .expect("violations array")
            .len(),
        0,
        "D01 fix should emit no violations: {verdict:#}"
    );
}

// =============================================================================
// D02: Dependency Not Allowed (whitelist violation)
// Status: ✅ Direct Detection - dependency.not_allowed rule catches this
// =============================================================================

#[test]
fn d02_dependency_not_allowed_intro_fails_gate_contract() {
    let temp =
        stage_dependency_fixture("d02-dependency-not-allowed", "client-intro.ts", "client.ts");

    let (result, verdict) = run_check(temp.path());

    assert_eq!(
        result.exit_code, EXIT_CODE_POLICY_VIOLATIONS,
        "D02 intro should fail: stdout={}, stderr={}",
        result.stdout, result.stderr
    );
    assert_status(&verdict, "fail");
    assert_single_violation_identity(
        &verdict,
        ViolationIdentity {
            rule: "dependency.not_allowed",
            from_module: "api-client",
            to_module: None,
            from_file: "src/client.ts",
            to_file: None,
        },
    );
}

#[test]
fn d02_dependency_not_allowed_fix_passes_gate_contract() {
    let temp = stage_dependency_fixture("d02-dependency-not-allowed", "client-fix.ts", "client.ts");

    let (result, verdict) = run_check(temp.path());

    assert_eq!(
        result.exit_code, EXIT_CODE_PASS,
        "D02 fix should pass: stdout={}, stderr={}",
        result.stdout, result.stderr
    );
    assert_status(&verdict, "pass");
    assert_eq!(
        verdict["violations"]
            .as_array()
            .expect("violations array")
            .len(),
        0,
        "D02 fix should emit no violations: {verdict:#}"
    );
}

#[test]
fn contract_23_golden_fixture_validates_cleanly() {
    let spec_path = fixtures_dir().join("contract-2.3.spec.yml");
    let spec = load_spec(&spec_path).expect("load contract 2.3 golden fixture");

    let report = validate_specs(&[spec]);

    assert!(
        !report.has_errors(),
        "contract 2.3 golden fixture should validate without errors: {report:#?}"
    );
}
