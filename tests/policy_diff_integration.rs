use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::Value;
use specgate::cli::{EXIT_CODE_PASS, EXIT_CODE_POLICY_VIOLATIONS, EXIT_CODE_RUNTIME_ERROR, run};
use tempfile::TempDir;

fn write_file(root: &Path, relative_path: &str, content: &str) {
    let path = root.join(relative_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("mkdir parent");
    }
    fs::write(path, content).expect("write file");
}

fn parse_json(stdout: &str) -> Value {
    serde_json::from_str(stdout).expect("valid json")
}

fn run_git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .expect("execute git");

    if !output.status.success() {
        panic!(
            "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    String::from_utf8(output.stdout)
        .expect("utf8 git stdout")
        .trim()
        .to_string()
}

fn init_git_repo(root: &Path) {
    run_git(root, &["init", "--initial-branch=main"]);
    run_git(root, &["config", "user.name", "Specgate Tests"]);
    run_git(
        root,
        &["config", "user.email", "specgate-tests@example.com"],
    );
}

fn commit_all(root: &Path, message: &str) {
    run_git(root, &["add", "-A"]);
    run_git(root, &["commit", "-m", message]);
}

fn write_common_project_files(root: &Path) {
    write_file(
        root,
        "specgate.config.yml",
        "spec_dirs:\n  - modules\nexclude: []\ntest_patterns: []\n",
    );
    write_file(root, "src/app/main.ts", "export const app = 1;\n");
    write_file(root, "src/app/deep/util.ts", "export const util = 1;\n");
    write_file(root, "src/core/index.ts", "export const core = 1;\n");
    write_file(root, "src/legacy/index.ts", "export const legacy = 1;\n");
    write_file(
        root,
        "modules/core.spec.yml",
        "version: \"2.3\"\nmodule: core\nboundaries:\n  path: src/core/**/*\nconstraints: []\n",
    );
}

fn run_policy_diff_json(root: &Path, base: &str) -> specgate::cli::CliRunResult {
    run([
        "specgate",
        "policy-diff",
        "--project-root",
        root.to_str().expect("utf8 root path"),
        "--base",
        base,
        "--format",
        "json",
    ])
}

fn run_policy_diff_ndjson(root: &Path, base: &str) -> specgate::cli::CliRunResult {
    run([
        "specgate",
        "policy-diff",
        "--project-root",
        root.to_str().expect("utf8 root path"),
        "--base",
        base,
        "--format",
        "ndjson",
    ])
}

fn parse_ndjson_lines(stdout: &str) -> Vec<Value> {
    stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("ndjson line"))
        .collect()
}

#[test]
fn widening_change_reports_exit_one() {
    let temp = TempDir::new().expect("tempdir");
    init_git_repo(temp.path());
    write_common_project_files(temp.path());

    write_file(
        temp.path(),
        "modules/app.spec.yml",
        "version: \"2.3\"\nmodule: app\nboundaries:\n  path: src/app/**/*\n  allow_imports_from:\n    - core\nconstraints: []\n",
    );
    commit_all(temp.path(), "base");

    write_file(
        temp.path(),
        "modules/app.spec.yml",
        "version: \"2.3\"\nmodule: app\nboundaries:\n  path: src/app/**/*\nconstraints: []\n",
    );
    commit_all(temp.path(), "head widening");

    let result = run_policy_diff_json(temp.path(), "HEAD~1");
    assert_eq!(result.exit_code, EXIT_CODE_POLICY_VIOLATIONS);

    let output = parse_json(&result.stdout);
    assert_eq!(output["summary"]["has_widening"], true);
    assert!(output["summary"]["widening_changes"].as_u64().unwrap() >= 1);
}

#[test]
fn narrowing_change_reports_exit_zero() {
    let temp = TempDir::new().expect("tempdir");
    init_git_repo(temp.path());
    write_common_project_files(temp.path());

    write_file(
        temp.path(),
        "modules/app.spec.yml",
        "version: \"2.3\"\nmodule: app\nboundaries:\n  path: src/app/**/*\n  never_imports: []\nconstraints: []\n",
    );
    commit_all(temp.path(), "base");

    write_file(
        temp.path(),
        "modules/app.spec.yml",
        "version: \"2.3\"\nmodule: app\nboundaries:\n  path: src/app/**/*\n  never_imports:\n    - core\nconstraints: []\n",
    );
    commit_all(temp.path(), "head narrowing");

    let result = run_policy_diff_json(temp.path(), "HEAD~1");
    assert_eq!(result.exit_code, EXIT_CODE_PASS);

    let output = parse_json(&result.stdout);
    assert!(!output["summary"]["has_widening"].as_bool().unwrap());
    assert!(output["summary"]["narrowing_changes"].as_u64().unwrap() >= 1);
    assert_eq!(output["summary"]["widening_changes"], 0);
}

#[test]
fn structural_only_change_reports_exit_zero() {
    let temp = TempDir::new().expect("tempdir");
    init_git_repo(temp.path());
    write_common_project_files(temp.path());

    write_file(
        temp.path(),
        "modules/app.spec.yml",
        "version: \"2.3\"\nmodule: app\ndescription: \"old\"\nboundaries:\n  path: src/app/**/*\nconstraints: []\n",
    );
    commit_all(temp.path(), "base");

    write_file(
        temp.path(),
        "modules/app.spec.yml",
        "version: \"2.3\"\nmodule: app\ndescription: \"new\"\nboundaries:\n  path: src/app/**/*\nconstraints: []\n",
    );
    commit_all(temp.path(), "head structural");

    let result = run_policy_diff_json(temp.path(), "HEAD~1");
    assert_eq!(result.exit_code, EXIT_CODE_PASS);

    let output = parse_json(&result.stdout);
    assert_eq!(output["summary"]["widening_changes"], 0);
    assert_eq!(output["summary"]["narrowing_changes"], 0);
    assert!(output["summary"]["structural_changes"].as_u64().unwrap() >= 1);
}

#[test]
fn mixed_change_set_contains_widening_narrowing_and_structural() {
    let temp = TempDir::new().expect("tempdir");
    init_git_repo(temp.path());
    write_common_project_files(temp.path());

    write_file(
        temp.path(),
        "modules/app.spec.yml",
        "version: \"2.3\"\nmodule: app\ndescription: \"old\"\nboundaries:\n  path: src/app/**/*\n  allow_imports_from:\n    - core\n  never_imports: []\nconstraints: []\n",
    );
    commit_all(temp.path(), "base");

    write_file(
        temp.path(),
        "modules/app.spec.yml",
        "version: \"2.3\"\nmodule: app\ndescription: \"new\"\nboundaries:\n  path: src/app/**/*\n  never_imports:\n    - legacy\nconstraints: []\n",
    );
    commit_all(temp.path(), "head mixed");

    let result = run_policy_diff_json(temp.path(), "HEAD~1");
    assert_eq!(result.exit_code, EXIT_CODE_POLICY_VIOLATIONS);

    let output = parse_json(&result.stdout);
    assert!(output["summary"]["widening_changes"].as_u64().unwrap() >= 1);
    assert!(output["summary"]["narrowing_changes"].as_u64().unwrap() >= 1);
    assert!(output["summary"]["structural_changes"].as_u64().unwrap() >= 1);
}

#[test]
fn rename_with_widening_attempt_is_fail_closed() {
    let temp = TempDir::new().expect("tempdir");
    init_git_repo(temp.path());
    write_common_project_files(temp.path());

    write_file(
        temp.path(),
        "modules/app.spec.yml",
        "version: \"2.3\"\nmodule: app\nboundaries:\n  path: src/app/**/*\n  never_imports:\n    - core\nconstraints: []\n",
    );
    commit_all(temp.path(), "base");

    run_git(
        temp.path(),
        &["mv", "modules/app.spec.yml", "modules/app-renamed.spec.yml"],
    );
    write_file(
        temp.path(),
        "modules/app-renamed.spec.yml",
        "version: \"2.3\"\nmodule: app\nboundaries:\n  path: src/app/**/*\nconstraints: []\n",
    );
    commit_all(temp.path(), "rename and widen");

    let result = run_policy_diff_json(temp.path(), "HEAD~1");
    assert_eq!(result.exit_code, EXIT_CODE_POLICY_VIOLATIONS);

    let output = parse_json(&result.stdout);
    let changes = output["diffs"]
        .as_array()
        .expect("diffs")
        .iter()
        .flat_map(|diff| diff["changes"].as_array().expect("changes"))
        .collect::<Vec<_>>();

    assert!(changes.iter().any(|change| {
        change["field"] == "spec_file"
            && change["classification"] == "widening"
            && change["detail"]
                .as_str()
                .unwrap_or("")
                .contains("widening-risk")
    }));
}

#[test]
fn pure_spec_deletion_is_widening() {
    let temp = TempDir::new().expect("tempdir");
    init_git_repo(temp.path());
    write_common_project_files(temp.path());

    write_file(
        temp.path(),
        "modules/app.spec.yml",
        "version: \"2.3\"\nmodule: app\nconstraints: []\n",
    );
    commit_all(temp.path(), "base");

    fs::remove_file(temp.path().join("modules/app.spec.yml")).expect("remove spec");
    commit_all(temp.path(), "delete spec");

    let result = run_policy_diff_json(temp.path(), "HEAD~1");
    assert_eq!(result.exit_code, EXIT_CODE_POLICY_VIOLATIONS);

    let output = parse_json(&result.stdout);
    assert_eq!(output["summary"]["has_widening"], true);
    assert!(
        output["diffs"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|diff| diff["changes"].as_array().unwrap())
            .any(|change| change["detail"].as_str().unwrap_or("").contains("deletion"))
    );
}

#[test]
fn malformed_yaml_in_head_exits_two_without_panic() {
    let temp = TempDir::new().expect("tempdir");
    init_git_repo(temp.path());
    write_common_project_files(temp.path());

    write_file(
        temp.path(),
        "modules/app.spec.yml",
        "version: \"2.3\"\nmodule: app\nconstraints: []\n",
    );
    commit_all(temp.path(), "base");

    write_file(
        temp.path(),
        "modules/app.spec.yml",
        "version: \"2.3\"\nmodule: app\nboundaries: [\n  path: src/app/**/*\n",
    );
    commit_all(temp.path(), "bad head");

    let result = run_policy_diff_json(temp.path(), "HEAD~1");
    assert_eq!(result.exit_code, EXIT_CODE_RUNTIME_ERROR);
    assert!(result.stderr.is_empty());

    let output = parse_json(&result.stdout);
    let diffs = output["diffs"].as_array().expect("diffs array");
    assert!(diffs.is_empty());

    let summary = &output["summary"];
    assert_eq!(summary["modules_changed"].as_u64().unwrap(), 0);
    assert_eq!(summary["widening_changes"].as_u64().unwrap(), 0);
    assert_eq!(summary["narrowing_changes"].as_u64().unwrap(), 0);
    assert_eq!(summary["structural_changes"].as_u64().unwrap(), 0);
    assert!(!summary["has_widening"].as_bool().unwrap());

    let errors = output["errors"].as_array().expect("errors array");
    assert!(
        errors
            .iter()
            .any(|error| error["code"] == "policy.spec_parse_error")
    );
}

#[test]
fn malformed_yaml_in_head_emits_ndjson_error_event() {
    let temp = TempDir::new().expect("tempdir");
    init_git_repo(temp.path());
    write_common_project_files(temp.path());

    write_file(
        temp.path(),
        "modules/app.spec.yml",
        "version: \"2.3\"\nmodule: app\nconstraints: []\n",
    );
    commit_all(temp.path(), "base");

    write_file(
        temp.path(),
        "modules/app.spec.yml",
        "version: \"2.3\"\nmodule: app\nboundaries: [\n  path: src/app/**/*\n",
    );
    commit_all(temp.path(), "bad head");

    let result = run_policy_diff_ndjson(temp.path(), "HEAD~1");
    assert_eq!(result.exit_code, EXIT_CODE_RUNTIME_ERROR);
    assert!(result.stderr.is_empty());

    let events = parse_ndjson_lines(&result.stdout);
    assert!(events.len() >= 2);

    let summary = events.last().expect("summary event");
    assert_eq!(summary["type"], "summary");
    assert_eq!(summary["modules_changed"].as_u64().unwrap(), 0);
    assert_eq!(summary["widening_changes"].as_u64().unwrap(), 0);
    assert_eq!(summary["narrowing_changes"].as_u64().unwrap(), 0);
    assert_eq!(summary["structural_changes"].as_u64().unwrap(), 0);
    assert!(!summary["has_widening"].as_bool().unwrap());

    assert!(events.iter().any(|event| event["type"] == "error"));
    let error = events
        .iter()
        .find(|event| event["type"] == "error")
        .expect("error event");
    assert_eq!(error["code"], "policy.spec_parse_error");

    assert_eq!(events.first().expect("first event")["type"], "error");
}
#[test]
fn malformed_yaml_in_base_exits_two_without_panic() {
    let temp = TempDir::new().expect("tempdir");
    init_git_repo(temp.path());
    write_common_project_files(temp.path());

    write_file(
        temp.path(),
        "modules/app.spec.yml",
        "version: \"2.3\"\nmodule: app\nboundaries: [\n  path: src/app/**/*\n",
    );
    commit_all(temp.path(), "bad base");

    write_file(
        temp.path(),
        "modules/app.spec.yml",
        "version: \"2.3\"\nmodule: app\nconstraints: []\n",
    );
    commit_all(temp.path(), "fixed head");

    let result = run_policy_diff_json(temp.path(), "HEAD~1");
    assert_eq!(result.exit_code, EXIT_CODE_RUNTIME_ERROR);
    assert!(result.stderr.is_empty());

    let output = parse_json(&result.stdout);
    let diffs = output["diffs"].as_array().expect("diffs array");
    assert!(diffs.is_empty());

    let summary = &output["summary"];
    assert_eq!(summary["modules_changed"].as_u64().unwrap(), 0);
    assert_eq!(summary["widening_changes"].as_u64().unwrap(), 0);
    assert_eq!(summary["narrowing_changes"].as_u64().unwrap(), 0);
    assert_eq!(summary["structural_changes"].as_u64().unwrap(), 0);
    assert!(!summary["has_widening"].as_bool().unwrap());

    let errors = output["errors"].as_array().expect("errors array");
    assert!(
        errors
            .iter()
            .any(|error| error["code"] == "policy.spec_parse_error")
    );
}

#[test]
fn weird_filename_spaces_and_unicode_is_nul_safe() {
    let temp = TempDir::new().expect("tempdir");
    init_git_repo(temp.path());
    write_common_project_files(temp.path());

    let spec_path = "modules/über module ✅.spec.yml";
    write_file(
        temp.path(),
        spec_path,
        "version: \"2.3\"\nmodule: app\nboundaries:\n  path: src/app/**/*\n  allow_imports_from:\n    - core\nconstraints: []\n",
    );
    commit_all(temp.path(), "base");

    write_file(
        temp.path(),
        spec_path,
        "version: \"2.3\"\nmodule: app\nboundaries:\n  path: src/app/**/*\nconstraints: []\n",
    );
    commit_all(temp.path(), "head");

    let result = run_policy_diff_json(temp.path(), "HEAD~1");
    assert_eq!(result.exit_code, EXIT_CODE_POLICY_VIOLATIONS);

    let output = parse_json(&result.stdout);
    assert!(
        output["diffs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|diff| diff["spec_path"].as_str() == Some(spec_path))
    );
}

#[test]
fn shallow_clone_missing_base_ref_has_guidance() {
    let source = TempDir::new().expect("source tempdir");
    init_git_repo(source.path());
    write_common_project_files(source.path());

    write_file(
        source.path(),
        "modules/app.spec.yml",
        "version: \"2.3\"\nmodule: app\nconstraints: []\n",
    );
    commit_all(source.path(), "first");

    write_file(
        source.path(),
        "modules/app.spec.yml",
        "version: \"2.3\"\nmodule: app\ndescription: \"second\"\nconstraints: []\n",
    );
    commit_all(source.path(), "second");

    let clone_dir = TempDir::new().expect("clone tempdir");
    let source_url = format!("file://{}", source.path().display());
    let clone_path = clone_dir.path().join("shallow-clone");

    let status = Command::new("git")
        .args([
            "clone",
            "--depth",
            "1",
            &source_url,
            clone_path.to_str().expect("utf8 clone path"),
        ])
        .status()
        .expect("git clone");
    assert!(status.success());

    assert_eq!(
        run_git(&clone_path, &["rev-parse", "--is-shallow-repository"]),
        "true"
    );

    let result = run_policy_diff_json(&clone_path, "HEAD~1");
    assert_eq!(result.exit_code, EXIT_CODE_RUNTIME_ERROR);

    let output = parse_json(&result.stdout);
    let errors = output["errors"].as_array().expect("errors array");
    assert!(
        errors
            .iter()
            .any(|error| error["code"] == "git.shallow_clone_missing_ref")
    );
    assert!(errors.iter().any(|error| {
        let message = error["message"].as_str().unwrap_or("");
        message.contains("fetch-depth: 0") && message.contains("git fetch --deepen")
    }));
}

#[test]
fn path_glob_broadened_is_widening() {
    let temp = TempDir::new().expect("tempdir");
    init_git_repo(temp.path());
    write_common_project_files(temp.path());

    write_file(
        temp.path(),
        "modules/app.spec.yml",
        "version: \"2.3\"\nmodule: app\nboundaries:\n  path: src/app/*.ts\nconstraints: []\n",
    );
    commit_all(temp.path(), "base");

    write_file(
        temp.path(),
        "modules/app.spec.yml",
        "version: \"2.3\"\nmodule: app\nboundaries:\n  path: src/app/**/*.ts\nconstraints: []\n",
    );
    commit_all(temp.path(), "head broaden");

    let result = run_policy_diff_json(temp.path(), "HEAD~1");
    assert_eq!(result.exit_code, EXIT_CODE_POLICY_VIOLATIONS);

    let output = parse_json(&result.stdout);
    assert!(
        output["diffs"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|diff| diff["changes"].as_array().unwrap())
            .any(|change| {
                change["field"] == "boundaries.path"
                    && change["classification"] == "widening"
                    && change["detail"]
                        .as_str()
                        .unwrap_or("")
                        .contains("broadened")
            })
    );
}

#[test]
fn path_glob_narrowed_is_narrowing() {
    let temp = TempDir::new().expect("tempdir");
    init_git_repo(temp.path());
    write_common_project_files(temp.path());

    write_file(
        temp.path(),
        "modules/app.spec.yml",
        "version: \"2.3\"\nmodule: app\nboundaries:\n  path: src/app/**/*.ts\nconstraints: []\n",
    );
    commit_all(temp.path(), "base");

    write_file(
        temp.path(),
        "modules/app.spec.yml",
        "version: \"2.3\"\nmodule: app\nboundaries:\n  path: src/app/*.ts\nconstraints: []\n",
    );
    commit_all(temp.path(), "head narrowed");

    let result = run_policy_diff_json(temp.path(), "HEAD~1");
    assert_eq!(result.exit_code, EXIT_CODE_PASS);

    let output = parse_json(&result.stdout);
    assert!(!output["summary"]["has_widening"].as_bool().unwrap());
    assert!(
        output["diffs"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|diff| diff["changes"].as_array().unwrap())
            .any(|change| {
                change["field"] == "boundaries.path"
                    && change["classification"] == "narrowing"
                    && change["detail"].as_str().unwrap_or("").contains("narrowed")
            })
    );
}
