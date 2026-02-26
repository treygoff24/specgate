//! Tier A golden fixtures (CI gate).
//!
//! Tier A is strictly catchable-now and deterministic:
//! - intro fails now with exact expected violations (no extras)
//! - fix passes now with zero violations
//! - intro output is deterministic across 3 runs
//! - fixtures must not use `@specgate-ignore`

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use specgate::cli::{EXIT_CODE_PASS, EXIT_CODE_POLICY_VIOLATIONS, run};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ExpectedViolation {
    rule: String,
    from_module: Option<String>,
    to_module: Option<String>,
}

#[derive(Debug, Clone)]
struct TierAFixture {
    id: &'static str,
    expected_intro: Vec<ExpectedViolation>,
    near_miss_variant: Option<&'static str>,
}

fn ev(rule: &str, from_module: Option<&str>, to_module: Option<&str>) -> ExpectedViolation {
    ExpectedViolation {
        rule: rule.to_string(),
        from_module: from_module.map(ToString::to_string),
        to_module: to_module.map(ToString::to_string),
    }
}

fn fixtures() -> Vec<TierAFixture> {
    vec![
        TierAFixture {
            id: "a01-ingress-persistence-bypass",
            expected_intro: vec![ev(
                "boundary.allow_imports_from",
                Some("ingress"),
                Some("infra/db"),
            )],
            near_miss_variant: Some("near-miss"),
        },
        TierAFixture {
            id: "a02-internal-file-api-leak",
            expected_intro: vec![ev(
                "boundary.public_api",
                Some("consumer"),
                Some("provider"),
            )],
            near_miss_variant: None,
        },
        TierAFixture {
            id: "a03-layer-reversal-origin-guard",
            expected_intro: vec![ev(
                "enforce-layer",
                Some("domain/orders"),
                Some("ingress/http"),
            )],
            near_miss_variant: None,
        },
        TierAFixture {
            id: "a04-registry-canonical-entrypoint",
            expected_intro: vec![ev(
                "boundary.canonical_import",
                Some("consumer"),
                Some("registry"),
            )],
            near_miss_variant: None,
        },
        TierAFixture {
            id: "a06-external-cycle-registry",
            expected_intro: vec![ev("no-circular-deps", Some("registry"), None)],
            near_miss_variant: None,
        },
    ]
}

fn tier_a_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("golden")
        .join("tier-a")
}

fn fixture_dir(fixture: &TierAFixture) -> PathBuf {
    tier_a_dir().join(fixture.id)
}

fn variant_dir(fixture: &TierAFixture, variant: &str) -> PathBuf {
    fixture_dir(fixture).join(variant)
}

fn parse_json(stdout: &str) -> Value {
    serde_json::from_str(stdout).expect("cli output json")
}

fn run_check(project_root: &Path) -> (specgate::cli::CliRunResult, Value) {
    let result = run([
        "specgate",
        "check",
        "--project-root",
        project_root.to_str().expect("utf8 path"),
        "--no-baseline",
    ]);

    let json = parse_json(&result.stdout);
    (result, json)
}

fn normalized_violation_signature(verdict: &Value) -> String {
    let mut records = verdict["violations"]
        .as_array()
        .expect("violations array")
        .iter()
        .map(|violation| {
            (
                violation["rule"].as_str().unwrap_or_default().to_string(),
                violation["from_module"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                violation["to_module"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                violation["from_file"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                violation["to_file"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
            )
        })
        .collect::<Vec<_>>();

    // Determinism contract sort key:
    // rule_id, from_module, to_module, from_file, to_file
    records.sort();

    serde_json::to_string(&records).expect("serialize normalized records")
}

fn expected_set(fixture: &TierAFixture) -> BTreeSet<ExpectedViolation> {
    fixture.expected_intro.iter().cloned().collect()
}

fn actual_set(verdict: &Value) -> BTreeSet<ExpectedViolation> {
    verdict["violations"]
        .as_array()
        .expect("violations array")
        .iter()
        .map(|violation| {
            ev(
                violation["rule"].as_str().expect("violation.rule string"),
                violation["from_module"].as_str(),
                violation["to_module"].as_str(),
            )
        })
        .collect()
}

fn list_files(root: &Path, out: &mut Vec<PathBuf>) {
    if !root.exists() {
        return;
    }

    let entries = fs::read_dir(root).expect("read dir");
    for entry in entries {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if path.is_dir() {
            list_files(&path, out);
        } else {
            out.push(path);
        }
    }
}

fn assert_no_specgate_ignore(fixture: &TierAFixture, variant: &str) {
    let src_root = variant_dir(fixture, variant).join("src");
    let mut files = Vec::new();
    list_files(&src_root, &mut files);

    for file in files {
        let content = fs::read_to_string(&file).expect("read source file");
        assert!(
            !content.contains("@specgate-ignore"),
            "{} {} contains disallowed @specgate-ignore in {}",
            fixture.id,
            variant,
            file.display()
        );
    }
}

fn assert_contract_files_exist(fixture: &TierAFixture) {
    let root = fixture_dir(fixture);
    assert!(
        root.join("fixture.meta.yml").exists(),
        "{} missing fixture.meta.yml",
        fixture.id
    );
    assert!(
        root.join("expected/intro.verdict.json").exists(),
        "{} missing expected/intro.verdict.json",
        fixture.id
    );
    assert!(
        root.join("expected/fix.verdict.json").exists(),
        "{} missing expected/fix.verdict.json",
        fixture.id
    );
}

fn assert_intro_contract_and_determinism(fixture: &TierAFixture) {
    let intro_root = variant_dir(fixture, "intro");
    let mut signatures = Vec::new();

    for run_index in 0..3 {
        let (result, verdict) = run_check(&intro_root);
        assert_eq!(
            result.exit_code, EXIT_CODE_POLICY_VIOLATIONS,
            "{} intro run {} should fail with policy violations; stdout={}, stderr={}",
            fixture.id, run_index, result.stdout, result.stderr
        );

        assert_eq!(
            verdict["status"], "fail",
            "{} intro run {} should produce fail status",
            fixture.id, run_index
        );

        let violations = verdict["violations"].as_array().expect("violations array");
        assert_eq!(
            violations.len(),
            fixture.expected_intro.len(),
            "{} intro run {} should have exact violation count",
            fixture.id,
            run_index
        );

        let expected = expected_set(fixture);
        let actual = actual_set(&verdict);
        assert_eq!(
            actual, expected,
            "{} intro run {} should match exact expected violation set",
            fixture.id, run_index
        );

        let actual_rule_ids = violations
            .iter()
            .filter_map(|violation| violation["rule"].as_str())
            .map(ToString::to_string)
            .collect::<BTreeSet<_>>();
        let expected_rule_ids = fixture
            .expected_intro
            .iter()
            .map(|violation| violation.rule.clone())
            .collect::<BTreeSet<_>>();
        let unexpected_rule_ids = actual_rule_ids
            .difference(&expected_rule_ids)
            .cloned()
            .collect::<Vec<_>>();

        assert!(
            unexpected_rule_ids.is_empty(),
            "{} intro run {} has unexpected rule ids: {:?}",
            fixture.id,
            run_index,
            unexpected_rule_ids
        );

        signatures.push(normalized_violation_signature(&verdict));
    }

    assert_eq!(
        signatures[0], signatures[1],
        "{} intro violations should be deterministic across run 1 and 2",
        fixture.id
    );
    assert_eq!(
        signatures[1], signatures[2],
        "{} intro violations should be deterministic across run 2 and 3",
        fixture.id
    );
}

fn assert_fix_contract(fixture: &TierAFixture) {
    let (result, verdict) = run_check(&variant_dir(fixture, "fix"));

    assert_eq!(
        result.exit_code, EXIT_CODE_PASS,
        "{} fix should pass; stdout={}, stderr={}",
        fixture.id, result.stdout, result.stderr
    );
    assert_eq!(
        verdict["status"], "pass",
        "{} fix should produce pass status",
        fixture.id
    );

    let violations = verdict["violations"].as_array().expect("violations array");
    assert_eq!(
        violations.len(),
        0,
        "{} fix must have zero violations",
        fixture.id
    );
}

fn assert_optional_near_miss_contract(fixture: &TierAFixture) {
    if let Some(variant) = fixture.near_miss_variant {
        let (result, verdict) = run_check(&variant_dir(fixture, variant));

        assert_eq!(
            result.exit_code, EXIT_CODE_PASS,
            "{} {} should pass as near-miss precision check; stdout={}, stderr={}",
            fixture.id, variant, result.stdout, result.stderr
        );
        assert_eq!(
            verdict["status"], "pass",
            "{} {} should produce pass status",
            fixture.id, variant
        );
        assert_eq!(
            verdict["violations"]
                .as_array()
                .expect("violations array")
                .len(),
            0,
            "{} {} should have zero violations",
            fixture.id,
            variant
        );
    }
}

#[test]
fn tier_a_fixtures_are_strict_deterministic_and_ignore_free() {
    for fixture in fixtures() {
        assert_contract_files_exist(&fixture);

        assert_no_specgate_ignore(&fixture, "intro");
        assert_no_specgate_ignore(&fixture, "fix");
        if let Some(variant) = fixture.near_miss_variant {
            assert_no_specgate_ignore(&fixture, variant);
        }

        assert_intro_contract_and_determinism(&fixture);
        assert_fix_contract(&fixture);
        assert_optional_near_miss_contract(&fixture);
    }
}
