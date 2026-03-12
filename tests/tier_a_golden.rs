//! Tier A golden fixtures (CI gate).
//!
//! Tier A is strictly catchable-now and deterministic:
//! - intro fails now with exact expected violations (no extras)
//! - fix passes now with zero violations
//! - intro output is deterministic across 3 runs
//! - fixtures must not use `@specgate-ignore`

use std::collections::{BTreeSet, HashMap};
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
        TierAFixture {
            id: "c02-pattern-aware",
            near_miss_variant: None,
        },
        TierAFixture {
            id: "c06-category-gov",
            near_miss_variant: None,
        },
        TierAFixture {
            id: "c07-unique-export",
            near_miss_variant: None,
        },
    ]
}

fn intentional_tier_a_exclusions() -> Vec<(&'static str, &'static str)> {
    vec![(
        "a11-forbidden-dependency",
        "Requires npm dependency graph materialization (`package.json` + node_modules); validated by D01/D02 in golden_corpus",
    )]
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
    let path_display = path.display();
    let raw = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read {path_display}: {error}"));
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
    let fixture_id = fixture.id;
    match raw {
        Value::String(value) => Value::String(value.clone()),
        Value::Null => Value::Null,
        other => panic!(
            "{fixture_id} {variant} violation {index} has invalid {field} value {other}; expected string|null"
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
            let fixture_id = fixture.id;
            let object = violation.as_object().unwrap_or_else(|| {
                panic!(
                    "{fixture_id} {variant} violation {index} must be object"
                )
            });

            let rule = object
                .get("rule")
                .unwrap_or_else(|| {
                    panic!(
                        "{fixture_id} {variant} violation {index} missing required field rule"
                    )
                })
                .as_str()
                .unwrap_or_else(|| {
                    panic!(
                        "{fixture_id} {variant} violation {index} field rule must be string"
                    )
                });

            let from_module = normalize_optional_module_field(
                object.get("from_module").unwrap_or_else(|| {
                    panic!(
                        "{fixture_id} {variant} violation {index} missing required field from_module"
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
                        "{fixture_id} {variant} violation {index} missing required field to_module"
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
    let fixture_id = fixture.id;
    let status = verdict["status"]
        .as_str()
        .unwrap_or_else(|| panic!("{fixture_id} expected/{variant} status must be string"));
    assert!(
        status == "pass" || status == "fail",
        "{fixture_id} expected/{variant} status must be pass|fail"
    );

    let expected_count = verdict["expected_count"].as_u64().unwrap_or_else(|| {
        panic!("{fixture_id} expected/{variant} expected_count must be integer")
    });
    let violations_len = verdict["violations"]
        .as_array()
        .expect("violations array")
        .len();
    assert_eq!(
        expected_count as usize, violations_len,
        "{fixture_id} expected/{variant} expected_count must equal violations length"
    );
}

fn assert_a06_expected_to_module_explicit_null(
    fixture: &TierAFixture,
    verdict: &Value,
    variant: &str,
) {
    let fixture_id = fixture.id;
    if fixture.id != A06_FIXTURE_ID {
        return;
    }

    let violations = verdict["violations"].as_array().expect("violations array");
    let circular = violations
        .iter()
        .find(|violation| violation["rule"].as_str() == Some("no-circular-deps"))
        .unwrap_or_else(|| {
            panic!("{fixture_id} expected/{variant} missing no-circular-deps violation")
        });

    let object = circular
        .as_object()
        .expect("a06 expected violation must be object");
    assert!(
        object.contains_key("to_module"),
        "{fixture_id} expected/{variant} must explicitly include to_module key"
    );
    assert!(
        object["to_module"].is_null(),
        "{fixture_id} expected/{variant} no-circular-deps to_module must be null"
    );
}

fn assert_a06_actual_to_module_explicit_null(
    fixture: &TierAFixture,
    verdict: &Value,
    variant: &str,
    run_index: usize,
) {
    let fixture_id = fixture.id;
    if fixture.id != A06_FIXTURE_ID {
        return;
    }

    let violations = verdict["violations"].as_array().expect("violations array");
    let circular = violations
        .iter()
        .find(|violation| violation["rule"].as_str() == Some("no-circular-deps"))
        .unwrap_or_else(|| {
            panic!("{fixture_id} {variant} run {run_index} missing no-circular-deps violation")
        });

    let object = circular
        .as_object()
        .expect("a06 actual violation must be object");
    assert!(
        object.contains_key("to_module"),
        "{fixture_id} {variant} run {run_index} no-circular-deps must include to_module key"
    );
    assert!(
        object["to_module"].is_null(),
        "{fixture_id} {variant} run {run_index} no-circular-deps to_module must be null"
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
    let fixture_id = fixture.id;
    let src_root = variant_dir(fixture, variant).join("src");
    let mut files = Vec::new();
    list_files(&src_root, &mut files);

    for file in files {
        let file_display = file.display();
        let content = fs::read_to_string(&file).expect("read source file");
        assert!(
            !content.contains("@specgate-ignore"),
            "{fixture_id} {variant} contains disallowed @specgate-ignore in {file_display}"
        );
    }
}

fn assert_contract_files_exist(fixture: &TierAFixture) {
    let fixture_id = fixture.id;
    let root = fixture_dir(fixture);
    assert!(
        root.join("fixture.meta.yml").exists(),
        "{fixture_id} missing fixture.meta.yml"
    );
    assert!(
        expected_verdict_path(fixture, "intro").exists(),
        "{fixture_id} missing expected/intro.verdict.json"
    );
    assert!(
        expected_verdict_path(fixture, "fix").exists(),
        "{fixture_id} missing expected/fix.verdict.json"
    );
}

fn assert_tier_a_fixture_catalog_is_authorized() {
    let included: BTreeSet<String> = fixtures()
        .iter()
        .map(|fixture| fixture.id.to_string())
        .collect();
    let exclusions: HashMap<String, &str> = intentional_tier_a_exclusions()
        .into_iter()
        .map(|(id, reason)| (id.to_string(), reason))
        .collect();
    let excluded_ids = exclusions.keys().cloned().collect::<BTreeSet<_>>();
    let expected: BTreeSet<String> = included
        .iter()
        .cloned()
        .chain(excluded_ids.iter().cloned())
        .collect();

    let actual = fs::read_dir(tier_a_dir())
        .expect("read tier-a fixtures directory")
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            if !path.is_dir() {
                return None;
            }
            let name = path.file_name()?.to_str()?.to_string();
            Some(name)
        })
        .collect::<BTreeSet<_>>();

    if let Some((missing_exclusion,)) = excluded_ids
        .difference(&actual)
        .next()
        .map(|id| (id.clone(),))
    {
        panic!(
            "tier-a fixture exclusion list is stale: {missing_exclusion} is listed in intentional exclusions but directory is missing"
        );
    }

    let unexpected: Vec<_> = actual.difference(&expected).collect();
    if !unexpected.is_empty() {
        let details = unexpected
            .into_iter()
            .map(|id| {
                format!("{id} (add to fixtures() or intentional_tier_a_exclusions() with reason)")
            })
            .collect::<Vec<_>>()
            .join(", ");
        panic!("tier-a fixture directory drift detected for: {details}");
    }

    for (excluded_id, reason) in exclusions.iter() {
        assert!(
            actual.contains(excluded_id),
            "explicit tier-a exclusion is missing: {excluded_id} => {reason}"
        );
    }
}

fn assert_intro_contract_and_determinism(fixture: &TierAFixture) {
    let fixture_id = fixture.id;
    let expected_intro = load_expected_verdict(fixture, "intro");
    assert_expected_verdict_shape(fixture, "intro", &expected_intro);
    assert_a06_expected_to_module_explicit_null(fixture, &expected_intro, "intro");

    let expected_status = expected_intro["status"]
        .as_str()
        .expect("expected intro status");
    assert_eq!(
        expected_status, "fail",
        "{fixture_id} expected/intro status must be fail"
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
            result.exit_code,
            EXIT_CODE_POLICY_VIOLATIONS,
            "{fixture_id} intro run {run_index} should fail with policy violations; stdout={stdout}, stderr={stderr}",
            stdout = result.stdout,
            stderr = result.stderr
        );

        assert_eq!(
            verdict["status"],
            Value::String(expected_status.to_string()),
            "{fixture_id} intro run {run_index} should match expected intro status"
        );

        let actual_violations = canonical_violation_contract(&verdict, fixture, "intro");
        assert_eq!(
            actual_violations.len(),
            expected_count,
            "{fixture_id} intro run {run_index} should match expected intro violation count"
        );

        assert_eq!(
            actual_violations, expected_violations,
            "{fixture_id} intro run {run_index} should match expected intro violation contract"
        );

        assert_a06_actual_to_module_explicit_null(fixture, &verdict, "intro", run_index);

        signatures.push(normalized_violation_signature(&verdict));
    }

    assert_eq!(
        signatures[0], signatures[1],
        "{fixture_id} intro violations should be deterministic across run 1 and 2"
    );
    assert_eq!(
        signatures[1], signatures[2],
        "{fixture_id} intro violations should be deterministic across run 2 and 3"
    );
}

fn assert_fix_contract(fixture: &TierAFixture) {
    let fixture_id = fixture.id;
    let expected_fix = load_expected_verdict(fixture, "fix");
    assert_expected_verdict_shape(fixture, "fix", &expected_fix);

    let expected_status = expected_fix["status"]
        .as_str()
        .expect("expected fix status");
    assert_eq!(
        expected_status, "pass",
        "{fixture_id} expected/fix status must be pass"
    );

    let expected_count = expected_fix["expected_count"]
        .as_u64()
        .expect("expected count") as usize;
    let expected_violations = canonical_violation_contract(&expected_fix, fixture, "expected/fix");

    let (result, verdict) = run_check(&variant_dir(fixture, "fix"));

    assert_eq!(
        result.exit_code,
        EXIT_CODE_PASS,
        "{fixture_id} fix should pass; stdout={stdout}, stderr={stderr}",
        stdout = result.stdout,
        stderr = result.stderr
    );
    assert_eq!(
        verdict["status"],
        Value::String(expected_status.to_string()),
        "{fixture_id} fix should match expected/fix status"
    );

    let actual_violations = canonical_violation_contract(&verdict, fixture, "fix");
    assert_eq!(
        actual_violations.len(),
        expected_count,
        "{fixture_id} fix should match expected/fix violation count"
    );
    assert_eq!(
        actual_violations, expected_violations,
        "{fixture_id} fix should match expected/fix violation contract"
    );
}

fn assert_optional_near_miss_contract(fixture: &TierAFixture) {
    let fixture_id = fixture.id;
    if let Some(variant) = fixture.near_miss_variant {
        let variant_root = variant_dir(fixture, variant);
        assert!(
            variant_root.exists(),
            "{fixture_id} {variant} variant directory must exist"
        );
        assert!(
            variant_root.join("specgate.config.yml").exists(),
            "{fixture_id} {variant} missing specgate.config.yml"
        );

        let (result, verdict) = run_check(&variant_root);

        assert_eq!(
            result.exit_code,
            EXIT_CODE_PASS,
            "{fixture_id} {variant} should pass as near-miss precision check; stdout={stdout}, stderr={stderr}",
            stdout = result.stdout,
            stderr = result.stderr
        );
        assert_eq!(
            verdict["status"],
            Value::String("pass".to_string()),
            "{fixture_id} {variant} should produce pass status"
        );

        let actual_violations = canonical_violation_contract(&verdict, fixture, variant);
        assert_eq!(
            actual_violations.len(),
            0,
            "{fixture_id} {variant} should have zero violations"
        );
    }
}

#[test]
fn tier_a_fixtures_are_strict_deterministic_and_ignore_free() {
    assert_tier_a_fixture_catalog_is_authorized();

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
