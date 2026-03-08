use serde_json::json;

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
