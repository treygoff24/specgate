use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::json;
use tempfile::TempDir;

use super::classify_fail_closed_operations;
use super::git::{
    FailClosedSpecOperation, RenameCopySemanticPairing, discover_spec_file_changes,
    parse_name_status_z,
};
use super::types::{
    ChangeClassification, ChangeScope, FieldChange, ModulePolicyDiff, POLICY_DIFF_SCHEMA_VERSION,
    PolicyDiffErrorEntry, PolicyDiffExit, PolicyDiffReport, PolicyDiffSummary,
};

fn change(
    module: &str,
    spec_path: &str,
    classification: ChangeClassification,
    field: &str,
    detail: &str,
) -> FieldChange {
    FieldChange {
        module: module.to_string(),
        spec_path: spec_path.to_string(),
        scope: ChangeScope::Boundaries,
        field: field.to_string(),
        classification,
        before: Some(json!("before")),
        after: Some(json!("after")),
        detail: detail.to_string(),
    }
}

#[test]
fn enums_serialize_in_snake_case() {
    let widening = serde_json::to_string(&ChangeClassification::Widening).expect("serialize");
    let contract_match = serde_json::to_string(&ChangeScope::ContractMatch).expect("serialize");

    assert_eq!(widening, "\"widening\"");
    assert_eq!(contract_match, "\"contract_match\"");
}

#[test]
fn report_uses_schema_version_constant() {
    let report = PolicyDiffReport::new(
        "origin/main".to_string(),
        "HEAD".to_string(),
        Vec::new(),
        PolicyDiffSummary::default(),
        Vec::new(),
    );

    assert_eq!(POLICY_DIFF_SCHEMA_VERSION, "1");
    assert_eq!(report.schema_version, POLICY_DIFF_SCHEMA_VERSION);
}

#[test]
fn deterministic_sort_orders_diffs_changes_and_errors() {
    let mut report = PolicyDiffReport::new(
        "base".to_string(),
        "head".to_string(),
        vec![
            ModulePolicyDiff {
                module: "module/z".to_string(),
                spec_path: "modules/z.spec.yml".to_string(),
                changes: vec![
                    change(
                        "module/z",
                        "modules/z.spec.yml",
                        ChangeClassification::Structural,
                        "boundaries.path",
                        "path changed",
                    ),
                    change(
                        "module/z",
                        "modules/z.spec.yml",
                        ChangeClassification::Widening,
                        "boundaries.allow_imports_from",
                        "added shared/db",
                    ),
                ],
            },
            ModulePolicyDiff {
                module: "module/a".to_string(),
                spec_path: "modules/a.spec.yml".to_string(),
                changes: vec![
                    change(
                        "module/a",
                        "modules/a.spec.yml",
                        ChangeClassification::Narrowing,
                        "boundaries.never_imports",
                        "added api/internal",
                    ),
                    change(
                        "module/a",
                        "modules/a.spec.yml",
                        ChangeClassification::Widening,
                        "boundaries.visibility",
                        "private -> public",
                    ),
                ],
            },
        ],
        PolicyDiffSummary::default(),
        vec![
            PolicyDiffErrorEntry {
                code: "b.code".to_string(),
                message: "z-msg".to_string(),
                spec_path: Some("z/spec.yml".to_string()),
            },
            PolicyDiffErrorEntry {
                code: "a.code".to_string(),
                message: "a-msg".to_string(),
                spec_path: Some("a/spec.yml".to_string()),
            },
        ],
    );

    report.sort_deterministic();

    assert_eq!(report.diffs[0].module, "module/a");
    assert_eq!(report.diffs[1].module, "module/z");

    assert_eq!(
        report.diffs[0].changes[0].classification,
        ChangeClassification::Widening
    );
    assert_eq!(
        report.diffs[0].changes[1].classification,
        ChangeClassification::Narrowing
    );

    assert_eq!(report.errors[0].code, "a.code");
    assert_eq!(report.errors[1].code, "b.code");
}

#[test]
fn policy_diff_exit_codes_are_stable() {
    assert_eq!(PolicyDiffExit::Clean.code(), 0);
    assert_eq!(PolicyDiffExit::Widening.code(), 1);
    assert_eq!(PolicyDiffExit::RuntimeError.code(), 2);
}

fn run_git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .expect("execute git command");

    if !output.status.success() {
        panic!(
            "git {:?} failed:\nstdout: {}\nstderr: {}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    String::from_utf8(output.stdout)
        .expect("git output should be valid UTF-8")
        .trim()
        .to_string()
}

fn init_git_repo(root: &Path) {
    run_git(root, &["init", "--initial-branch=main"]);
    run_git(
        root,
        &["config", "user.email", "specgate-tests@example.com"],
    );
    run_git(root, &["config", "user.name", "Specgate Tests"]);
}

fn write_file(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent directory");
    }
    fs::write(path, content).expect("write file");
}

fn commit_all(root: &Path, message: &str) {
    run_git(root, &["add", "-A"]);
    run_git(root, &["commit", "-m", message]);
}

#[test]
fn parse_name_status_z_buckets_amt_and_fail_closed_operations() {
    let raw = b"A\0modules/new policy.spec.yml\0M\0modules/modified.spec.yml\0T\0modules/type-changed.spec.yml\0D\0modules/gone.spec.yml\0R089\0modules/old policy.spec.yml\0modules/new policy name.spec.yml\0C100\0modules/original.spec.yml\0modules/copied.spec.yml\0";

    let parsed = parse_name_status_z(raw).expect("parse diff output");

    assert_eq!(
        parsed.changed_spec_paths,
        BTreeSet::from([
            "modules/new policy.spec.yml".to_string(),
            "modules/modified.spec.yml".to_string(),
            "modules/type-changed.spec.yml".to_string(),
        ])
    );

    assert_eq!(
        parsed.fail_closed_operations,
        vec![
            FailClosedSpecOperation::Deletion {
                path: "modules/gone.spec.yml".to_string(),
            },
            FailClosedSpecOperation::RenameOrCopy {
                status: "R089".to_string(),
                from_path: "modules/old policy.spec.yml".to_string(),
                to_path: "modules/new policy name.spec.yml".to_string(),
                semantic_pairing: RenameCopySemanticPairing::Unassessed,
            },
            FailClosedSpecOperation::RenameOrCopy {
                status: "C100".to_string(),
                from_path: "modules/original.spec.yml".to_string(),
                to_path: "modules/copied.spec.yml".to_string(),
                semantic_pairing: RenameCopySemanticPairing::Unassessed,
            },
        ]
    );
}

#[test]
fn parse_name_status_z_preserves_unicode_and_special_paths() {
    let raw = "A\0模块/支付.spec.yml\0M\0modules/@scope/payments v2.spec.yml\0"
        .as_bytes()
        .to_vec();

    let parsed = parse_name_status_z(&raw).expect("parse diff output");

    assert!(parsed.changed_spec_paths.contains("模块/支付.spec.yml"));
    assert!(
        parsed
            .changed_spec_paths
            .contains("modules/@scope/payments v2.spec.yml")
    );
}

#[test]
fn discover_spec_file_changes_rejects_non_git_directories() {
    let temp = TempDir::new().expect("tempdir");

    let error = discover_spec_file_changes(temp.path(), "HEAD~1", "HEAD")
        .expect_err("expected non-git directory error");

    assert_eq!(error.code(), "git.not_repository");
}

#[test]
fn discover_spec_file_changes_reports_invalid_refs_on_full_clone() {
    let temp = TempDir::new().expect("tempdir");
    init_git_repo(temp.path());
    write_file(
        temp.path(),
        "modules/app.spec.yml",
        "version: \"2.3\"\nmodule: app\nboundaries:\n  path: src/app/**/*\nconstraints: []\n",
    );
    commit_all(temp.path(), "initial");

    let error = discover_spec_file_changes(temp.path(), "missing-ref", "HEAD")
        .expect_err("expected invalid ref error");

    assert_eq!(error.code(), "git.invalid_ref");
    assert!(error.message().contains("missing-ref"));
}

#[test]
fn discover_spec_file_changes_reports_shallow_clone_missing_ref_with_guidance() {
    let origin = TempDir::new().expect("origin tempdir");
    init_git_repo(origin.path());

    write_file(
        origin.path(),
        "modules/app.spec.yml",
        "version: \"2.3\"\nmodule: app\nboundaries:\n  path: src/app/**/*\nconstraints: []\n",
    );
    commit_all(origin.path(), "commit one");
    let base_commit = run_git(origin.path(), &["rev-parse", "HEAD"]);

    write_file(
        origin.path(),
        "modules/app.spec.yml",
        "version: \"2.3\"\nmodule: app\ndescription: second\nboundaries:\n  path: src/app/**/*\nconstraints: []\n",
    );
    commit_all(origin.path(), "commit two");

    let clone_parent = TempDir::new().expect("clone parent tempdir");
    let clone_path = clone_parent.path().join("clone");
    let origin_url = format!("file://{}", origin.path().display());

    let clone_output = Command::new("git")
        .args([
            "clone",
            "--depth",
            "1",
            &origin_url,
            clone_path.to_str().expect("utf8 clone path"),
        ])
        .output()
        .expect("clone shallow repo");
    assert!(
        clone_output.status.success(),
        "git clone failed: {}",
        String::from_utf8_lossy(&clone_output.stderr)
    );

    let error = discover_spec_file_changes(&clone_path, &base_commit, "HEAD")
        .expect_err("expected shallow clone missing ref error");

    assert_eq!(error.code(), "git.shallow_clone_missing_ref");
    assert!(error.message().contains("fetch-depth: 0"));
    assert!(error.message().contains("git fetch --deepen=200 origin"));
}

#[test]
fn discover_spec_file_changes_classifies_diff_operations_from_git() {
    let temp = TempDir::new().expect("tempdir");
    init_git_repo(temp.path());

    write_file(
        temp.path(),
        "modules/keep.spec.yml",
        "version: \"2.3\"\nmodule: keep\nboundaries:\n  path: src/keep/**/*\nconstraints: []\n",
    );
    write_file(
        temp.path(),
        "modules/delete.spec.yml",
        "module: deleted-completely-distinct\nlegacy_only: true\nnotes: this content intentionally diverges to avoid rename pairing\n",
    );
    write_file(
        temp.path(),
        "modules/rename-old.spec.yml",
        "version: \"2.3\"\nmodule: rename\nboundaries:\n  path: src/rename/**/*\nconstraints: []\n",
    );
    write_file(temp.path(), "src/ignore.ts", "export const one = 1;\n");
    commit_all(temp.path(), "base");

    write_file(
        temp.path(),
        "modules/keep.spec.yml",
        "version: \"2.3\"\nmodule: keep\ndescription: changed\nboundaries:\n  path: src/keep/**/*\nconstraints: []\n",
    );
    fs::remove_file(temp.path().join("modules/delete.spec.yml")).expect("remove spec file");
    run_git(
        temp.path(),
        &[
            "mv",
            "modules/rename-old.spec.yml",
            "modules/rename-new.spec.yml",
        ],
    );
    write_file(
        temp.path(),
        "modules/new.spec.yml",
        "version: \"2.3\"\nmodule: new\nboundaries:\n  path: src/new/**/*\nconstraints: []\n",
    );
    write_file(temp.path(), "src/ignore.ts", "export const two = 2;\n");
    commit_all(temp.path(), "head");

    let discovered =
        discover_spec_file_changes(temp.path(), "HEAD~1", "HEAD").expect("discover changes");

    assert_eq!(
        discovered.changed_spec_paths,
        BTreeSet::from([
            "modules/keep.spec.yml".to_string(),
            "modules/new.spec.yml".to_string(),
        ])
    );

    assert_eq!(discovered.fail_closed_operations.len(), 2);
    assert!(
        discovered
            .fail_closed_operations
            .iter()
            .any(|operation| matches!(
                operation,
                FailClosedSpecOperation::Deletion { path } if path == "modules/delete.spec.yml"
            ))
    );
    assert!(
        discovered
            .fail_closed_operations
            .iter()
            .any(|operation| matches!(
                operation,
                FailClosedSpecOperation::RenameOrCopy {
                    status,
                    from_path,
                    to_path,
                    ..
                } if status.starts_with('R')
                    && from_path == "modules/rename-old.spec.yml"
                    && to_path == "modules/rename-new.spec.yml"
            ))
    );
}

#[test]
fn classify_fail_closed_operations_downgrades_equivalent_rename_to_structural() {
    let diffs = classify_fail_closed_operations(&[FailClosedSpecOperation::RenameOrCopy {
        status: "R100".to_string(),
        from_path: "modules/app.spec.yml".to_string(),
        to_path: "modules/app-renamed.spec.yml".to_string(),
        semantic_pairing: RenameCopySemanticPairing::Equivalent,
    }]);

    assert_eq!(diffs.len(), 1);
    assert_eq!(diffs[0].changes.len(), 1);
    assert_eq!(
        diffs[0].changes[0].classification,
        ChangeClassification::Structural
    );
    assert!(
        diffs[0].changes[0]
            .detail
            .contains("semantically equivalent")
    );
}

#[test]
fn classify_fail_closed_operations_keeps_inconclusive_rename_fail_closed() {
    let diffs = classify_fail_closed_operations(&[FailClosedSpecOperation::RenameOrCopy {
        status: "R100".to_string(),
        from_path: "modules/app.spec.yml".to_string(),
        to_path: "modules/app-renamed.spec.yml".to_string(),
        semantic_pairing: RenameCopySemanticPairing::Inconclusive,
    }]);

    assert_eq!(diffs.len(), 1);
    assert_eq!(diffs[0].changes.len(), 1);
    assert_eq!(
        diffs[0].changes[0].classification,
        ChangeClassification::Widening
    );
    assert!(diffs[0].changes[0].detail.contains("widening-risk"));
}
