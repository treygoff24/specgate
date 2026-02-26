//! Tier A golden fixtures (CI gate).
//!
//! Tier A is strictly catchable-now and deterministic:
//! - intro fails now with exact expected violations (no extras)
//! - fix passes now with zero violations
//! - intro output is deterministic across 3 runs
//! - fixtures must not use `@specgate-ignore`

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use specgate::cli::{EXIT_CODE_PASS, EXIT_CODE_POLICY_VIOLATIONS, run};

const A06_FIXTURE_ID: &str = "a06-external-cycle-registry";

#[derive(Debug, Clone)]
struct TierAFixture {
    id: &'static str,
    near_miss_variant: Option<&'static str>,
}

fn fixtures() -> Vec<TierAFixture> {
    vec![
        TierAFixture {
            id: "a01-ingress-persistence-bypass",
            near_miss_variant: Some("near-miss"),
        },
        TierAFixture {
            id: "a02-internal-file-api-leak",
            near_miss_variant: None,
        },
        TierAFixture {
            id: "a03-layer-reversal-origin-guard",
            near_miss_variant: Some("near-miss"),
        },
        TierAFixture {
            id: "a04-registry-canonical-entrypoint",
            near_miss_variant: None,
        },
        TierAFixture {
            id: "a06-external-cycle-registry",
            near_miss_variant: Some("near-miss"),
        },
        TierAFixture {
            id: "a07-provider-visibility-private",
            near_miss_variant: None,
        },
        TierAFixture {
            id: "a08-provider-visibility-internal",
            near_miss_variant: None,
        },
        TierAFixture {
            id: "a09-importer-never-imports",
            near_miss_variant: None,
        },
        TierAFixture {
            id: "a10-provider-deny-imported-by",
            near_miss_variant: None,
        },
        // NOTE: a11-forbidden-dependency excluded from tier-a because it requires
        // npm dependencies (package.json + node_modules) which breaks the deterministic,
        // self-contained tier-a gate criteria. Dependency rules are covered by d01/d02
        // in the regular golden_corpus tests.
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

fn expected_verdict_path(fixture: &TierAFixture, variant: &str) -> PathBuf {
    fixture_dir(fixture)
        .join("expected")
        .join(format!("{variant}.verdict.json"))
}

fn parse_json(raw: &str) -> Value {
    serde_json::from_str(raw).expect("json payload")
}

fn load_expected_verdict(fixture: &TierAFixture, variant: &str) -> Value {
    let path = expected_verdict_path(fixture, variant);
    let raw = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
    parse_json(&raw)
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

fn normalize_optional_module_field(
    raw: &Value,
    fixture: &TierAFixture,
    variant: &str,
    index: usize,
    field: &str,
) -> Value {
    match raw {
        Value::String(value) => Value::String(value.clone()),
        Value::Null => Value::Null,
        other => panic!(
            "{} {} violation {} has invalid {} value {}; expected string|null",
            fixture.id, variant, index, field, other
        ),
    }
}

fn canonical_violation_contract(
    verdict: &Value,
    fixture: &TierAFixture,
    variant: &str,
) -> Vec<Value> {
    let mut violations = verdict["violations"]
        .as_array()
        .expect("violations array")
        .iter()
        .enumerate()
        .map(|(index, violation)| {
            let object = violation.as_object().unwrap_or_else(|| {
                panic!(
                    "{} {} violation {} must be object",
                    fixture.id, variant, index
                )
            });

            let rule = object
                .get("rule")
                .unwrap_or_else(|| {
                    panic!(
                        "{} {} violation {} missing required field rule",
                        fixture.id, variant, index
                    )
                })
                .as_str()
                .unwrap_or_else(|| {
                    panic!(
                        "{} {} violation {} field rule must be string",
                        fixture.id, variant, index
                    )
                });

            let from_module = normalize_optional_module_field(
                object.get("from_module").unwrap_or_else(|| {
                    panic!(
                        "{} {} violation {} missing required field from_module",
                        fixture.id, variant, index
                    )
                }),
                fixture,
                variant,
                index,
                "from_module",
            );
            let to_module = normalize_optional_module_field(
                object.get("to_module").unwrap_or_else(|| {
                    panic!(
                        "{} {} violation {} missing required field to_module",
                        fixture.id, variant, index
                    )
                }),
                fixture,
                variant,
                index,
                "to_module",
            );

            json!({
                "rule": rule,
                "from_module": from_module,
                "to_module": to_module,
            })
        })
        .collect::<Vec<_>>();

    violations.sort_by(|left, right| {
        left["rule"]
            .as_str()
            .unwrap_or_default()
            .cmp(right["rule"].as_str().unwrap_or_default())
            .then_with(|| {
                left["from_module"]
                    .to_string()
                    .cmp(&right["from_module"].to_string())
            })
            .then_with(|| {
                left["to_module"]
                    .to_string()
                    .cmp(&right["to_module"].to_string())
            })
    });

    violations
}

fn assert_expected_verdict_shape(fixture: &TierAFixture, variant: &str, verdict: &Value) {
    let status = verdict["status"]
        .as_str()
        .unwrap_or_else(|| panic!("{} expected/{} status must be string", fixture.id, variant));
    assert!(
        status == "pass" || status == "fail",
        "{} expected/{} status must be pass|fail",
        fixture.id,
        variant
    );

    let expected_count = verdict["expected_count"].as_u64().unwrap_or_else(|| {
        panic!(
            "{} expected/{} expected_count must be integer",
            fixture.id, variant
        )
    });
    let violations_len = verdict["violations"]
        .as_array()
        .expect("violations array")
        .len();
    assert_eq!(
        expected_count as usize, violations_len,
        "{} expected/{} expected_count must equal violations length",
        fixture.id, variant
    );
}

fn assert_a06_expected_to_module_explicit_null(
    fixture: &TierAFixture,
    verdict: &Value,
    variant: &str,
) {
    if fixture.id != A06_FIXTURE_ID {
        return;
    }

    let violations = verdict["violations"].as_array().expect("violations array");
    let circular = violations
        .iter()
        .find(|violation| violation["rule"].as_str() == Some("no-circular-deps"))
        .unwrap_or_else(|| {
            panic!(
                "{} expected/{} missing no-circular-deps violation",
                fixture.id, variant
            )
        });

    let object = circular
        .as_object()
        .expect("a06 expected violation must be object");
    assert!(
        object.contains_key("to_module"),
        "{} expected/{} must explicitly include to_module key",
        fixture.id,
        variant
    );
    assert!(
        object["to_module"].is_null(),
        "{} expected/{} no-circular-deps to_module must be null",
        fixture.id,
        variant
    );
}

fn assert_a06_actual_to_module_explicit_null(
    fixture: &TierAFixture,
    verdict: &Value,
    variant: &str,
    run_index: usize,
) {
    if fixture.id != A06_FIXTURE_ID {
        return;
    }

    let violations = verdict["violations"].as_array().expect("violations array");
    let circular = violations
        .iter()
        .find(|violation| violation["rule"].as_str() == Some("no-circular-deps"))
        .unwrap_or_else(|| {
            panic!(
                "{} {} run {} missing no-circular-deps violation",
                fixture.id, variant, run_index
            )
        });

    let object = circular
        .as_object()
        .expect("a06 actual violation must be object");
    assert!(
        object.contains_key("to_module"),
        "{} {} run {} no-circular-deps must include to_module key",
        fixture.id,
        variant,
        run_index
    );
    assert!(
        object["to_module"].is_null(),
        "{} {} run {} no-circular-deps to_module must be null",
        fixture.id,
        variant,
        run_index
    );
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
        expected_verdict_path(fixture, "intro").exists(),
        "{} missing expected/intro.verdict.json",
        fixture.id
    );
    assert!(
        expected_verdict_path(fixture, "fix").exists(),
        "{} missing expected/fix.verdict.json",
        fixture.id
    );
}

fn assert_intro_contract_and_determinism(fixture: &TierAFixture) {
    let expected_intro = load_expected_verdict(fixture, "intro");
    assert_expected_verdict_shape(fixture, "intro", &expected_intro);
    assert_a06_expected_to_module_explicit_null(fixture, &expected_intro, "intro");

    let expected_status = expected_intro["status"]
        .as_str()
        .expect("expected intro status");
    assert_eq!(
        expected_status, "fail",
        "{} expected/intro status must be fail",
        fixture.id
    );

    let expected_count = expected_intro["expected_count"]
        .as_u64()
        .expect("expected count") as usize;
    let expected_violations =
        canonical_violation_contract(&expected_intro, fixture, "expected/intro");

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
            verdict["status"],
            Value::String(expected_status.to_string()),
            "{} intro run {} should match expected intro status",
            fixture.id,
            run_index
        );

        let actual_violations = canonical_violation_contract(&verdict, fixture, "intro");
        assert_eq!(
            actual_violations.len(),
            expected_count,
            "{} intro run {} should match expected intro violation count",
            fixture.id,
            run_index
        );

        assert_eq!(
            actual_violations, expected_violations,
            "{} intro run {} should match expected intro violation contract",
            fixture.id, run_index
        );

        assert_a06_actual_to_module_explicit_null(fixture, &verdict, "intro", run_index);

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
    let expected_fix = load_expected_verdict(fixture, "fix");
    assert_expected_verdict_shape(fixture, "fix", &expected_fix);

    let expected_status = expected_fix["status"]
        .as_str()
        .expect("expected fix status");
    assert_eq!(
        expected_status, "pass",
        "{} expected/fix status must be pass",
        fixture.id
    );

    let expected_count = expected_fix["expected_count"]
        .as_u64()
        .expect("expected count") as usize;
    let expected_violations = canonical_violation_contract(&expected_fix, fixture, "expected/fix");

    let (result, verdict) = run_check(&variant_dir(fixture, "fix"));

    assert_eq!(
        result.exit_code, EXIT_CODE_PASS,
        "{} fix should pass; stdout={}, stderr={}",
        fixture.id, result.stdout, result.stderr
    );
    assert_eq!(
        verdict["status"],
        Value::String(expected_status.to_string()),
        "{} fix should match expected/fix status",
        fixture.id
    );

    let actual_violations = canonical_violation_contract(&verdict, fixture, "fix");
    assert_eq!(
        actual_violations.len(),
        expected_count,
        "{} fix should match expected/fix violation count",
        fixture.id
    );
    assert_eq!(
        actual_violations, expected_violations,
        "{} fix should match expected/fix violation contract",
        fixture.id
    );
}

fn assert_optional_near_miss_contract(fixture: &TierAFixture) {
    if let Some(variant) = fixture.near_miss_variant {
        let variant_root = variant_dir(fixture, variant);
        assert!(
            variant_root.exists(),
            "{} {} variant directory must exist",
            fixture.id,
            variant
        );
        assert!(
            variant_root.join("specgate.config.yml").exists(),
            "{} {} missing specgate.config.yml",
            fixture.id,
            variant
        );

        let (result, verdict) = run_check(&variant_root);

        assert_eq!(
            result.exit_code, EXIT_CODE_PASS,
            "{} {} should pass as near-miss precision check; stdout={}, stderr={}",
            fixture.id, variant, result.stdout, result.stderr
        );
        assert_eq!(
            verdict["status"],
            Value::String("pass".to_string()),
            "{} {} should produce pass status",
            fixture.id,
            variant
        );

        let actual_violations = canonical_violation_contract(&verdict, fixture, variant);
        assert_eq!(
            actual_violations.len(),
            0,
            "{} {} should have zero violations",
            fixture.id,
            variant
        );
    }
}

#[test]
fn tier_a_fixtures_are_strict_deterministic_and_ignore_free() {
    let fixtures = fixtures();
    let fixture_ids = fixtures
        .iter()
        .map(|fixture| fixture.id)
        .collect::<Vec<_>>();
    let mut sorted_fixture_ids = fixture_ids.clone();
    sorted_fixture_ids.sort();
    assert_eq!(
        fixture_ids, sorted_fixture_ids,
        "tier-a fixture list must remain lexicographically sorted for deterministic execution"
    );

    for fixture in fixtures {
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
