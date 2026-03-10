//! Integration tests for cross-file compensation in policy-diff.

use specgate::policy::types::{
    ChangeClassification, ChangeScope, CompensationCandidate, CompensationResult, DependencyEdge,
    FieldChange, PolicyDiffReport,
};

#[test]
fn compensation_candidate_has_typed_relationship() {
    let widening = FieldChange {
        module: "auth".into(),
        spec_path: "auth/.spec.yml".into(),
        scope: ChangeScope::Boundaries,
        field: "public_api".into(),
        classification: ChangeClassification::Widening,
        before: None,
        after: None,
        detail: "added public_api entry".into(),
    };
    let narrowing = FieldChange {
        module: "api".into(),
        spec_path: "api/.spec.yml".into(),
        scope: ChangeScope::Boundaries,
        field: "public_api".into(),
        classification: ChangeClassification::Narrowing,
        before: None,
        after: None,
        detail: "removed public_api entry".into(),
    };
    let edge = DependencyEdge {
        importer: "api".into(),
        provider: "auth".into(),
    };
    let candidate = CompensationCandidate {
        widening: widening.clone(),
        narrowing: narrowing.clone(),
        relationship: edge,
        result: CompensationResult::Offset,
    };
    assert_eq!(candidate.widening.module, "auth");
    assert_eq!(candidate.narrowing.module, "api");
    assert_eq!(candidate.relationship.importer, "api");
    assert_eq!(candidate.relationship.provider, "auth");
}

#[test]
fn report_has_compensations_and_net_classification() {
    let report = PolicyDiffReport {
        schema_version: "1".into(),
        base_ref: "base".into(),
        head_ref: "HEAD".into(),
        diffs: vec![],
        summary: Default::default(),
        errors: vec![],
        compensations: vec![],
        net_classification: ChangeClassification::Structural,
        config_changes: vec![],
    };
    assert!(report.compensations.is_empty());
    assert_eq!(report.net_classification, ChangeClassification::Structural);
}

use specgate::policy::compensate::find_compensation_candidates;

#[test]
fn same_field_connected_modules_produce_offset() {
    // api has allow_imports_from: [auth] — direct dependency
    // widening in auth/public_api + narrowing in api/public_api => Offset
    let widenings = vec![make_field_change(
        "auth",
        "public_api",
        ChangeClassification::Widening,
    )];
    let narrowings = vec![make_field_change(
        "api",
        "public_api",
        ChangeClassification::Narrowing,
    )];
    let edges = vec![DependencyEdge {
        importer: "api".into(),
        provider: "auth".into(),
    }];

    let candidates = find_compensation_candidates(&widenings, &narrowings, &edges);
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].result, CompensationResult::Offset);
    assert_eq!(candidates[0].relationship.importer, "api");
    assert_eq!(candidates[0].relationship.provider, "auth");
}

#[test]
fn different_field_family_does_not_compensate() {
    let widenings = vec![make_field_change(
        "auth",
        "public_api",
        ChangeClassification::Widening,
    )];
    let narrowings = vec![make_field_change(
        "api",
        "allow_imports_from",
        ChangeClassification::Narrowing,
    )];
    let edges = vec![DependencyEdge {
        importer: "api".into(),
        provider: "auth".into(),
    }];
    let candidates = find_compensation_candidates(&widenings, &narrowings, &edges);
    assert!(candidates.is_empty());
}

#[test]
fn unconnected_modules_do_not_compensate() {
    let widenings = vec![make_field_change(
        "auth",
        "public_api",
        ChangeClassification::Widening,
    )];
    let narrowings = vec![make_field_change(
        "unrelated",
        "public_api",
        ChangeClassification::Narrowing,
    )];
    let edges: Vec<DependencyEdge> = vec![];
    let candidates = find_compensation_candidates(&widenings, &narrowings, &edges);
    assert!(candidates.is_empty());
}

#[test]
fn ambiguous_compensation_fails_closed() {
    // One narrowing, two widenings in the same field family and connected
    let widenings = vec![
        make_field_change("auth", "public_api", ChangeClassification::Widening),
        make_field_change("core", "public_api", ChangeClassification::Widening),
    ];
    let narrowings = vec![make_field_change(
        "api",
        "public_api",
        ChangeClassification::Narrowing,
    )];
    let edges = vec![
        DependencyEdge {
            importer: "api".into(),
            provider: "auth".into(),
        },
        DependencyEdge {
            importer: "api".into(),
            provider: "core".into(),
        },
    ];
    let candidates = find_compensation_candidates(&widenings, &narrowings, &edges);
    // Ambiguous: one narrowing could offset either widening
    assert!(
        candidates
            .iter()
            .all(|c| c.result == CompensationResult::Ambiguous)
    );
}

// Helper
fn make_field_change(
    module: &str,
    field: &str,
    classification: ChangeClassification,
) -> FieldChange {
    FieldChange {
        module: module.into(),
        spec_path: format!("{module}/.spec.yml"),
        scope: ChangeScope::Boundaries,
        field: field.into(),
        classification,
        before: None,
        after: None,
        detail: format!("{classification:?} in {module}/{field}"),
    }
}
