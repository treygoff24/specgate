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

fn run_policy_diff_human(root: &Path, base: &str) -> specgate::cli::CliRunResult {
    run([
        "specgate",
        "policy-diff",
        "--project-root",
        root.to_str().expect("utf8 root path"),
        "--base",
        base,
        "--format",
        "human",
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
    assert_eq!(output["net_classification"], "narrowing");
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
fn rename_only_semantically_equivalent_is_structural() {
    let temp = TempDir::new().expect("tempdir");
    init_git_repo(temp.path());
    write_common_project_files(temp.path());

    write_file(
        temp.path(),
        "modules/app.spec.yml",
        "version: \"2.3\"\nmodule: app\ndescription: \"App policy\"\nboundaries:\n  path: src/app/**/*\n  public_api:\n    - src/app/main.ts\n    - src/app/deep/util.ts\n  never_imports:\n    - core\nconstraints:\n  - rule: module-boundary\n    params: {}\n",
    );
    commit_all(temp.path(), "base");

    run_git(
        temp.path(),
        &["mv", "modules/app.spec.yml", "modules/app-renamed.spec.yml"],
    );
    write_file(
        temp.path(),
        "modules/app-renamed.spec.yml",
        "version: \"2.3\"\nmodule: app\ndescription: \"  App policy  \"\nboundaries:\n  path: src/app/**/*\n  public_api:\n    - src/app/deep/util.ts\n    - src/app/main.ts\n  never_imports:\n    - core\nconstraints:\n  - rule: module-boundary\n    params: {}\n",
    );
    commit_all(temp.path(), "rename only");

    let result = run_policy_diff_json(temp.path(), "HEAD~1");
    assert_eq!(result.exit_code, EXIT_CODE_PASS);

    let output = parse_json(&result.stdout);
    assert_eq!(output["summary"]["has_widening"], false);
    assert_eq!(output["summary"]["widening_changes"], 0);
    assert!(output["summary"]["structural_changes"].as_u64().unwrap() >= 1);
    assert!(
        output["diffs"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|diff| diff["changes"].as_array().unwrap())
            .any(|change| {
                change["field"] == "spec_file"
                    && change["classification"] == "structural"
                    && change["detail"]
                        .as_str()
                        .unwrap_or("")
                        .contains("semantically equivalent")
            })
    );
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
fn rename_with_unparseable_head_stays_fail_closed_widening() {
    let temp = TempDir::new().expect("tempdir");
    init_git_repo(temp.path());
    write_common_project_files(temp.path());

    write_file(
        temp.path(),
        "modules/app.spec.yml",
        "version: \"2.3\"\nmodule: app\nboundaries:\n  path: src/app/**/*\nconstraints: []\n",
    );
    commit_all(temp.path(), "base");

    run_git(
        temp.path(),
        &["mv", "modules/app.spec.yml", "modules/app-renamed.spec.yml"],
    );
    write_file(
        temp.path(),
        "modules/app-renamed.spec.yml",
        "version: \"2.3\"\nmodule: app\nboundaries: [\n  path: src/app/**/*\n",
    );
    commit_all(temp.path(), "rename malformed head");

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
            .any(|change| {
                change["field"] == "spec_file"
                    && change["classification"] == "widening"
                    && change["detail"]
                        .as_str()
                        .unwrap_or("")
                        .contains("widening-risk")
            })
    );
}

#[test]
fn copy_only_semantically_equivalent_is_structural() {
    let temp = TempDir::new().expect("tempdir");
    init_git_repo(temp.path());
    write_common_project_files(temp.path());

    write_file(
        temp.path(),
        "modules/app.spec.yml",
        "version: \"2.3\"\nmodule: app\ndescription: \"App policy\"\nboundaries:\n  path: src/app/**/*\n  public_api:\n    - src/app/main.ts\n    - src/app/deep/util.ts\n  never_imports:\n    - core\nconstraints:\n  - rule: module-boundary\n    params: {}\n",
    );
    commit_all(temp.path(), "base");

    write_file(
        temp.path(),
        "modules/app-copy.spec.yml",
        "version: \"2.3\"\nmodule: app\ndescription: \"App policy\"\nboundaries:\n  path: src/app/**/*\n  public_api:\n    - src/app/main.ts\n    - src/app/deep/util.ts\n  never_imports:\n    - core\nconstraints:\n  - rule: module-boundary\n    params: {}\n",
    );
    commit_all(temp.path(), "copy only");

    let result = run_policy_diff_json(temp.path(), "HEAD~1");
    assert_eq!(result.exit_code, EXIT_CODE_PASS);

    let output = parse_json(&result.stdout);
    assert_eq!(output["summary"]["has_widening"], false);
    assert_eq!(output["summary"]["widening_changes"], 0);
    assert!(output["summary"]["structural_changes"].as_u64().unwrap() >= 1);
    assert!(
        output["diffs"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|diff| diff["changes"].as_array().unwrap())
            .any(|change| {
                change["field"] == "spec_file"
                    && change["classification"] == "structural"
                    && change["detail"]
                        .as_str()
                        .unwrap_or("")
                        .contains("semantically equivalent")
                    && change["detail"].as_str().unwrap_or("").contains("(C")
            })
    );
}

#[test]
fn copy_with_widening_attempt_is_fail_closed() {
    let temp = TempDir::new().expect("tempdir");
    init_git_repo(temp.path());
    write_common_project_files(temp.path());

    write_file(
        temp.path(),
        "modules/app.spec.yml",
        "version: \"2.3\"\nmodule: app\nboundaries:\n  path: src/app/**/*\n  never_imports:\n    - core\nconstraints: []\n",
    );
    commit_all(temp.path(), "base");

    write_file(
        temp.path(),
        "modules/app-copy.spec.yml",
        "version: \"2.3\"\nmodule: app\nboundaries:\n  path: src/app/**/*\nconstraints: []\n",
    );
    commit_all(temp.path(), "copy and widen");

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
            .any(|change| {
                change["field"] == "spec_file"
                    && change["classification"] == "widening"
                    && change["detail"]
                        .as_str()
                        .unwrap_or("")
                        .contains("widening-risk")
                    && change["detail"].as_str().unwrap_or("").contains("(C")
            })
    );
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

#[test]
fn path_glob_change_with_unbounded_prefix_reports_limitation_summary() {
    let temp = TempDir::new().expect("tempdir");
    init_git_repo(temp.path());
    write_common_project_files(temp.path());

    write_file(
        temp.path(),
        "modules/app.spec.yml",
        "version: \"2.3\"\nmodule: app\nboundaries:\n  path: src/app/**/*\nconstraints: []\n",
    );
    commit_all(temp.path(), "base");

    write_file(
        temp.path(),
        "modules/app.spec.yml",
        "version: \"2.3\"\nmodule: app\nboundaries:\n  path: \":(bad)\"\nconstraints: []\n",
    );
    commit_all(temp.path(), "head unbounded path change");

    let result = run_policy_diff_json(temp.path(), "HEAD~1");
    assert_eq!(result.exit_code, EXIT_CODE_PASS);

    let output = parse_json(&result.stdout);
    assert!(!output["summary"]["has_widening"].as_bool().unwrap());
    assert_eq!(
        output["summary"]["limitations"]
            .as_array()
            .expect("limitations array")
            .iter()
            .map(|value| value.as_str().expect("string limitation"))
            .collect::<Vec<_>>(),
        vec!["path_coverage_unbounded_mvp"]
    );
    assert!(
        output["diffs"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|diff| diff["changes"].as_array().unwrap())
            .any(|change| {
                change["field"] == "boundaries.path"
                    && change["classification"] == "structural"
                    && change["detail"]
                        .as_str()
                        .unwrap_or("")
                        .contains("path_coverage_unbounded_mvp")
            })
    );
}

#[test]
fn config_addition_is_structural_only_and_surfaces_all_formats() {
    let temp = TempDir::new().expect("tempdir");
    init_git_repo(temp.path());

    write_file(temp.path(), "src/app.ts", "export const app = 1;\n");
    commit_all(temp.path(), "base");

    write_file(
        temp.path(),
        "specgate.config.yml",
        "jest_mock_mode: enforce\n",
    );
    commit_all(temp.path(), "head config widening");

    let json_result = run_policy_diff_json(temp.path(), "HEAD~1");
    assert_eq!(json_result.exit_code, EXIT_CODE_PASS);

    let json_output = parse_json(&json_result.stdout);
    assert_eq!(json_output["summary"]["has_widening"], false);
    assert_eq!(json_output["summary"]["widening_changes"], 0);
    assert_eq!(json_output["summary"]["structural_changes"], 1);
    assert_eq!(json_output["net_classification"], "structural");
    assert_eq!(json_output["config_changes"].as_array().unwrap().len(), 1);
    assert_eq!(
        json_output["config_changes"][0]["field_path"],
        "jest_mock_mode"
    );
    assert_eq!(
        json_output["config_changes"][0]["classification"],
        "structural"
    );

    let ndjson_result = run_policy_diff_ndjson(temp.path(), "HEAD~1");
    assert_eq!(ndjson_result.exit_code, EXIT_CODE_PASS);

    let events = parse_ndjson_lines(&ndjson_result.stdout);
    assert!(events.iter().any(|event| {
        event["type"] == "config_change"
            && event["field_path"] == "jest_mock_mode"
            && event["classification"] == "structural"
    }));
    assert_eq!(
        events.last().expect("summary")["net_classification"],
        "structural"
    );

    let human_result = run_policy_diff_human(temp.path(), "HEAD~1");
    assert_eq!(human_result.exit_code, EXIT_CODE_PASS);
    assert!(
        human_result
            .stdout
            .contains("Config changes (specgate.config.yml):")
    );
    assert!(
        human_result
            .stdout
            .contains("STRUCTURAL: jest_mock_mode: warn -> enforce")
    );
}

#[test]
fn config_addition_widening_keeps_widening_classification() {
    let temp = TempDir::new().expect("tempdir");
    init_git_repo(temp.path());

    write_file(temp.path(), "src/app.ts", "export const app = 1;\n");
    commit_all(temp.path(), "base");

    write_file(
        temp.path(),
        "specgate.config.yml",
        "unresolved_edge_policy: ignore\n",
    );
    commit_all(temp.path(), "head config first-introduction widening");

    let result = run_policy_diff_json(temp.path(), "HEAD~1");
    assert_eq!(result.exit_code, EXIT_CODE_POLICY_VIOLATIONS);

    let output = parse_json(&result.stdout);
    assert_eq!(output["summary"]["has_widening"], true);
    assert_eq!(output["summary"]["widening_changes"], 1);
    assert_eq!(output["summary"]["structural_changes"], 0);
    assert_eq!(output["net_classification"], "widening");
    assert_eq!(output["diffs"].as_array().unwrap().len(), 0);
    assert_eq!(output["config_changes"].as_array().unwrap().len(), 1);
    assert_eq!(
        output["config_changes"][0]["field_path"],
        "unresolved_edge_policy"
    );
    assert_eq!(
        output["config_changes"][0]["classification"],
        "widening"
    );
}

#[test]
fn config_deletion_relaxes_to_defaults_and_reports_widening() {
    let temp = TempDir::new().expect("tempdir");
    init_git_repo(temp.path());

    write_file(
        temp.path(),
        "specgate.config.yml",
        "strict_ownership: true\nstrict_ownership_level: warnings\n",
    );
    commit_all(temp.path(), "base strict config");

    fs::remove_file(temp.path().join("specgate.config.yml")).expect("remove config");
    commit_all(temp.path(), "delete config");

    let result = run_policy_diff_json(temp.path(), "HEAD~1");
    assert_eq!(result.exit_code, EXIT_CODE_POLICY_VIOLATIONS);

    let output = parse_json(&result.stdout);
    assert_eq!(output["summary"]["has_widening"], true);
    assert_eq!(output["summary"]["widening_changes"], 2);
    assert_eq!(output["net_classification"], "widening");
    assert!(
        output["config_changes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|change| {
                change["field_path"] == "strict_ownership" && change["classification"] == "widening"
            })
    );
    assert!(
        output["config_changes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|change| {
                change["field_path"] == "strict_ownership_level"
                    && change["classification"] == "widening"
            })
    );
}

#[test]
fn config_update_widening_contributes_to_exit_code_and_summary_counts() {
    let temp = TempDir::new().expect("tempdir");
    init_git_repo(temp.path());

    write_file(
        temp.path(),
        "specgate.config.yml",
        "jest_mock_mode: enforce\n",
    );
    commit_all(temp.path(), "base config");

    write_file(temp.path(), "specgate.config.yml", "jest_mock_mode: warn\n");
    commit_all(temp.path(), "relax config");

    let result = run_policy_diff_json(temp.path(), "HEAD~1");
    assert_eq!(result.exit_code, EXIT_CODE_POLICY_VIOLATIONS);

    let output = parse_json(&result.stdout);
    assert_eq!(output["summary"]["has_widening"], true);
    assert_eq!(output["summary"]["widening_changes"], 1);
    assert_eq!(output["summary"]["structural_changes"], 0);
    assert_eq!(output["net_classification"], "widening");
    assert_eq!(output["diffs"].as_array().unwrap().len(), 0);
    assert_eq!(output["config_changes"][0]["field_path"], "jest_mock_mode");
    assert_eq!(output["config_changes"][0]["classification"], "widening");
}
