use std::fs;
use std::path::Path;

use serde_json::Value;
use tempfile::TempDir;

use specgate::baseline::audit::audit_baseline;
use specgate::baseline::{
    BASELINE_FILE_VERSION, BaselineEntry, BaselineFile, BaselineGeneratedFrom, is_valid_date_format,
};
use specgate::cli::{EXIT_CODE_PASS, EXIT_CODE_POLICY_VIOLATIONS, EXIT_CODE_RUNTIME_ERROR, run};
use specgate::spec::Severity;

fn write_file(root: &Path, relative_path: &str, content: &str) {
    let path = root.join(relative_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("mkdir parent");
    }
    fs::write(path, content).expect("write file");
}

fn parse_json(source: &str) -> Value {
    serde_json::from_str(source).expect("valid json")
}

fn write_project_with_violation(root: &Path, require_metadata: bool) {
    write_file(
        root,
        "modules/app.spec.yml",
        "version: \"2.2\"\nmodule: app\nboundaries:\n  path: src/app/**/*\n  never_imports:\n    - core\nconstraints: []\n",
    );
    write_file(
        root,
        "modules/core.spec.yml",
        "version: \"2.2\"\nmodule: core\nboundaries:\n  path: src/core/**/*\nconstraints: []\n",
    );
    write_file(
        root,
        "src/app/main.ts",
        "import { core } from '../core/index';\nexport const app = core;\n",
    );
    write_file(root, "src/core/index.ts", "export const core = 1;\n");

    let mut config = String::from("spec_dirs:\n  - modules\nexclude: []\ntest_patterns: []\n");
    if require_metadata {
        config.push_str("baseline:\n  require_metadata: true\n");
    }
    write_file(root, "specgate.config.yml", &config);
}

fn sample_entry(
    fingerprint: &str,
    owner: Option<&str>,
    reason: Option<&str>,
    expires_at: Option<&str>,
    added_at: Option<&str>,
) -> BaselineEntry {
    BaselineEntry {
        fingerprint: fingerprint.to_string(),
        positional_fingerprint: None,
        rule: "boundary.never_imports".to_string(),
        severity: Severity::Error,
        message: format!("violation-{fingerprint}"),
        from_file: format!("src/{fingerprint}.ts"),
        to_file: Some("src/core/index.ts".to_string()),
        from_module: Some(fingerprint.to_string()),
        to_module: Some("core".to_string()),
        line: Some(1),
        column: Some(0),
        owner: owner.map(str::to_string),
        reason: reason.map(str::to_string),
        expires_at: expires_at.map(str::to_string),
        added_at: added_at.map(str::to_string),
    }
}

#[test]
fn baseline_fallthrough_populates_added_at() {
    let temp = TempDir::new().expect("tempdir");
    write_project_with_violation(temp.path(), false);

    let result = run([
        "specgate",
        "baseline",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_PASS);

    let output = parse_json(&result.stdout);
    assert_eq!(output["entry_count"], 1);

    let baseline = parse_json(
        &fs::read_to_string(temp.path().join(".specgate-baseline.json")).expect("read baseline"),
    );
    let added_at = baseline["entries"][0]["added_at"]
        .as_str()
        .expect("added_at string");
    assert!(is_valid_date_format(added_at));
}

#[test]
fn baseline_add_is_idempotent_and_matches_check_fingerprints() {
    let temp = TempDir::new().expect("tempdir");
    write_project_with_violation(temp.path(), false);

    let add_once = run([
        "specgate",
        "baseline",
        "add",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--rule",
        "boundary.never_imports",
        "--from-module",
        "app",
        "--owner",
        "team-app",
        "--reason",
        "legacy-migration",
    ]);
    assert_eq!(add_once.exit_code, EXIT_CODE_PASS);

    let add_output = parse_json(&add_once.stdout);
    assert_eq!(add_output["added_count"], 1);
    assert_eq!(add_output["entry_count"], 1);

    let check = run([
        "specgate",
        "check",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);
    assert_eq!(check.exit_code, EXIT_CODE_PASS);
    assert!(check.stdout.contains("\"baseline_violations\": 1"));

    let add_twice = run([
        "specgate",
        "baseline",
        "add",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--rule",
        "boundary.never_imports",
        "--from-module",
        "app",
        "--owner",
        "team-app",
        "--reason",
        "legacy-migration",
    ]);
    assert_eq!(add_twice.exit_code, EXIT_CODE_PASS);

    let second_output = parse_json(&add_twice.stdout);
    assert_eq!(second_output["added_count"], 0);
    assert_eq!(second_output["entry_count"], 1);

    let baseline = parse_json(
        &fs::read_to_string(temp.path().join(".specgate-baseline.json")).expect("read baseline"),
    );
    let entries = baseline["entries"].as_array().expect("entries array");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["owner"], "team-app");
    assert_eq!(entries[0]["reason"], "legacy-migration");
    assert!(is_valid_date_format(
        entries[0]["added_at"].as_str().expect("added_at")
    ));
}

#[test]
fn baseline_add_requires_owner_and_reason_when_configured() {
    let temp = TempDir::new().expect("tempdir");
    write_project_with_violation(temp.path(), true);

    let result = run([
        "specgate",
        "baseline",
        "add",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--rule",
        "boundary.never_imports",
        "--from-module",
        "app",
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_RUNTIME_ERROR);
    let output = parse_json(&result.stdout);
    assert_eq!(output["status"], "error");
    assert!(
        output["message"]
            .as_str()
            .expect("message")
            .contains("--owner and --reason")
    );
}

#[test]
fn baseline_add_rejects_blank_owner_and_reason_when_configured() {
    let temp = TempDir::new().expect("tempdir");
    write_project_with_violation(temp.path(), true);

    let result = run([
        "specgate",
        "baseline",
        "add",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--rule",
        "boundary.never_imports",
        "--from-module",
        "app",
        "--owner",
        "   ",
        "--reason",
        "",
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_RUNTIME_ERROR);
    let output = parse_json(&result.stdout);
    assert_eq!(output["status"], "error");
    assert!(
        output["message"]
            .as_str()
            .expect("message")
            .contains("--owner and --reason")
    );
}

#[test]
fn baseline_add_keeps_missing_duplicate_when_only_positional_identity_differs() {
    let temp = TempDir::new().expect("tempdir");
    write_project_with_violation(temp.path(), false);
    write_file(
        temp.path(),
        "src/app/main.ts",
        "import { core as coreA } from '../core/index';\nimport { core as coreB } from '../core/index';\nexport const app = coreA + coreB;\n",
    );

    let generated = run([
        "specgate",
        "baseline",
        "--project-root",
        temp.path().to_str().expect("utf8"),
    ]);
    assert_eq!(generated.exit_code, EXIT_CODE_PASS);

    let baseline_path = temp.path().join(".specgate-baseline.json");
    let mut baseline =
        parse_json(&fs::read_to_string(&baseline_path).expect("read generated baseline"));
    let entries = baseline["entries"].as_array_mut().expect("entries array");
    assert_eq!(entries.len(), 2);
    entries.remove(1);
    fs::write(
        &baseline_path,
        serde_json::to_string_pretty(&baseline).expect("serialize trimmed baseline"),
    )
    .expect("write trimmed baseline");

    let add_result = run([
        "specgate",
        "baseline",
        "add",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--rule",
        "boundary.never_imports",
        "--from-module",
        "app",
        "--owner",
        "team-app",
        "--reason",
        "backfill duplicate",
    ]);
    assert_eq!(add_result.exit_code, EXIT_CODE_PASS);

    let output = parse_json(&add_result.stdout);
    assert_eq!(output["matched_violation_count"], 2);
    assert_eq!(output["added_count"], 1);
    assert_eq!(output["entry_count"], 2);

    let rewritten = parse_json(&fs::read_to_string(&baseline_path).expect("read final baseline"));
    let entries = rewritten["entries"].as_array().expect("entries array");
    assert_eq!(entries.len(), 2);
}

#[test]
fn baseline_list_normalizes_legacy_rule_ids_for_display() {
    let temp = TempDir::new().expect("tempdir");
    write_project_with_violation(temp.path(), false);

    let baseline = BaselineFile {
        version: BASELINE_FILE_VERSION.to_string(),
        generated_from: BaselineGeneratedFrom::default(),
        entries: vec![sample_entry(
            "legacy-rule",
            Some("team-app"),
            Some("migration"),
            None,
            None,
        )],
    };

    let baseline_path = temp.path().join(".specgate-baseline.json");
    let mut baseline_json = serde_json::to_value(&baseline).expect("baseline json");
    baseline_json["entries"][0]["rule"] = Value::String("edge.unresolved".to_string());
    fs::write(
        &baseline_path,
        serde_json::to_string_pretty(&baseline_json).expect("serialize baseline"),
    )
    .expect("write baseline");

    let result = run([
        "specgate",
        "baseline",
        "list",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--format",
        "json",
    ]);

    assert_eq!(result.exit_code, EXIT_CODE_PASS);
    let output = parse_json(&result.stdout);
    assert_eq!(
        output["entries"][0]["rule"].as_str().expect("rule"),
        "hygiene.unresolved_import"
    );
}

#[test]
fn audit_report_counts_metadata_and_expiry_gaps() {
    let baseline = BaselineFile {
        version: BASELINE_FILE_VERSION.to_string(),
        generated_from: BaselineGeneratedFrom::default(),
        entries: vec![
            sample_entry(
                "team-a-expired",
                Some("team-a"),
                Some("cleanup"),
                Some("2026-01-01"),
                Some("2026-02-01"),
            ),
            sample_entry(
                "team-a-expiring",
                Some("team-a"),
                None,
                Some("2026-03-20"),
                Some("2026-03-01"),
            ),
            sample_entry("no-owner", None, Some("legacy"), None, None),
        ],
    };

    let report = audit_baseline(&baseline, "2026-03-10");

    assert_eq!(report.total_entries, 3);
    assert_eq!(report.by_owner["team-a"].total, 2);
    assert_eq!(report.by_owner["team-a"].expired, 1);
    assert_eq!(report.entries_without_owner, 1);
    assert_eq!(report.entries_without_reason, 1);
    assert_eq!(report.entries_without_added_at, 1);
    assert_eq!(report.expired, 1);
    assert_eq!(report.expiring_within_30d, 1);
    assert_eq!(report.no_expiry, 1);
    assert_eq!(report.active, 0);
    assert_eq!(report.has_owner_count, 2);
    assert_eq!(report.has_reason_count, 2);
    assert_eq!(report.has_added_at_count, 2);
    assert!(report.has_metadata_gaps());
}

#[test]
fn audit_treats_calendar_invalid_expiry_as_no_expiry() {
    let baseline = BaselineFile {
        version: BASELINE_FILE_VERSION.to_string(),
        generated_from: BaselineGeneratedFrom::default(),
        entries: vec![sample_entry(
            "calendar-invalid",
            Some("team-a"),
            Some("cleanup"),
            Some("2026-02-30"),
            Some("2026-03-01"),
        )],
    };

    let report = audit_baseline(&baseline, "2026-03-10");

    assert_eq!(report.expired, 0);
    assert_eq!(report.expiring_within_30d, 0);
    assert_eq!(report.no_expiry, 1);
    assert_eq!(report.active, 0);
}

#[test]
fn baseline_audit_uses_require_metadata_for_exit_code() {
    let temp = TempDir::new().expect("tempdir");
    write_project_with_violation(temp.path(), true);

    let baseline = BaselineFile {
        version: BASELINE_FILE_VERSION.to_string(),
        generated_from: BaselineGeneratedFrom::default(),
        entries: vec![sample_entry("app", None, None, None, None)],
    };
    fs::write(
        temp.path().join(".specgate-baseline.json"),
        serde_json::to_string_pretty(&baseline).expect("serialize baseline"),
    )
    .expect("write baseline");

    let strict = run([
        "specgate",
        "baseline",
        "audit",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--format",
        "json",
    ]);
    assert_eq!(strict.exit_code, EXIT_CODE_POLICY_VIOLATIONS);
    let strict_output = parse_json(&strict.stdout);
    assert_eq!(strict_output["status"], "fail");
    assert_eq!(strict_output["report"]["entries_without_owner"], 1);
    assert_eq!(strict_output["report"]["entries_without_reason"], 1);
    assert_eq!(strict_output["report"]["entries_without_added_at"], 1);

    write_file(
        temp.path(),
        "specgate.config.yml",
        "spec_dirs:\n  - modules\nexclude: []\ntest_patterns: []\nbaseline:\n  require_metadata: false\n",
    );

    let relaxed = run([
        "specgate",
        "baseline",
        "audit",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--format",
        "json",
    ]);
    assert_eq!(relaxed.exit_code, EXIT_CODE_PASS);
    let relaxed_output = parse_json(&relaxed.stdout);
    assert_eq!(relaxed_output["status"], "ok");
}

#[test]
fn baseline_list_filters_by_owner() {
    let temp = TempDir::new().expect("tempdir");
    write_project_with_violation(temp.path(), false);

    let baseline = BaselineFile {
        version: BASELINE_FILE_VERSION.to_string(),
        generated_from: BaselineGeneratedFrom::default(),
        entries: vec![
            sample_entry(
                "team-a",
                Some("team-a"),
                Some("legacy"),
                None,
                Some("2026-03-10"),
            ),
            sample_entry(
                "team-b",
                Some("team-b"),
                Some("legacy"),
                None,
                Some("2026-03-10"),
            ),
        ],
    };
    fs::write(
        temp.path().join(".specgate-baseline.json"),
        serde_json::to_string_pretty(&baseline).expect("serialize baseline"),
    )
    .expect("write baseline");

    let result = run([
        "specgate",
        "baseline",
        "list",
        "--project-root",
        temp.path().to_str().expect("utf8"),
        "--owner",
        "team-a",
        "--format",
        "json",
    ]);
    assert_eq!(result.exit_code, EXIT_CODE_PASS);

    let output = parse_json(&result.stdout);
    let entries = output["entries"].as_array().expect("entries array");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["owner"], "team-a");
}
