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

use specgate::policy::compensate::{dependency_edges_from_specs, find_compensation_candidates};
use specgate::spec::{Boundaries, SpecFile};

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

#[test]
fn reverse_edge_direction_still_compensates() {
    // Edge direction is bidirectional in find_edge: if api imports from auth,
    // then auth→api widening + api→auth narrowing should still match.
    // Here we flip: edge says "auth imports from api" (reversed)
    // but widening is in api, narrowing in auth — should still match.
    let widenings = vec![make_field_change(
        "api",
        "public_api",
        ChangeClassification::Widening,
    )];
    let narrowings = vec![make_field_change(
        "auth",
        "public_api",
        ChangeClassification::Narrowing,
    )];
    // Edge is in "reverse" direction relative to the change
    let edges = vec![DependencyEdge {
        importer: "auth".into(),
        provider: "api".into(),
    }];

    let candidates = find_compensation_candidates(&widenings, &narrowings, &edges);
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].result, CompensationResult::Offset);
}

#[test]
fn same_module_change_does_not_compensate() {
    // A widening and narrowing in the same module should NOT compensate
    // even if they're in the same field family.
    let widenings = vec![make_field_change(
        "auth",
        "public_api",
        ChangeClassification::Widening,
    )];
    let narrowings = vec![make_field_change(
        "auth", // same module
        "public_api",
        ChangeClassification::Narrowing,
    )];
    let edges = vec![DependencyEdge {
        importer: "auth".into(),
        provider: "core".into(),
    }];

    let candidates = find_compensation_candidates(&widenings, &narrowings, &edges);
    assert!(
        candidates.is_empty(),
        "same-module changes should not compensate"
    );
}

#[test]
fn multiple_narrowings_same_widening_marked_ambiguous() {
    // Two narrowings in different modules both matching the same widening
    // should result in Ambiguous for all candidates (dedup logic).
    let widenings = vec![make_field_change(
        "core",
        "public_api",
        ChangeClassification::Widening,
    )];
    let narrowings = vec![
        make_field_change("auth", "public_api", ChangeClassification::Narrowing),
        make_field_change("api", "public_api", ChangeClassification::Narrowing),
    ];
    // Both auth and api import from core
    let edges = vec![
        DependencyEdge {
            importer: "auth".into(),
            provider: "core".into(),
        },
        DependencyEdge {
            importer: "api".into(),
            provider: "core".into(),
        },
    ];

    let candidates = find_compensation_candidates(&widenings, &narrowings, &edges);
    // Should have 2 candidates (one per narrowing), all marked Ambiguous
    assert_eq!(candidates.len(), 2);
    assert!(
        candidates
            .iter()
            .all(|c| c.result == CompensationResult::Ambiguous),
        "multiple narrowings matching same widening should be ambiguous"
    );
}

#[test]
fn dependency_edges_extracted_from_specs() {
    // Test that dependency_edges_from_specs correctly extracts edges
    // from specs' allow_imports_from fields.
    let specs = vec![
        SpecFile {
            version: "2.2".into(),
            module: "api".into(),
            package: None,
            import_id: None,
            import_ids: vec![],
            description: None,
            boundaries: Some(Boundaries {
                allow_imports_from: Some(vec!["auth".into(), "core".into()]),
                ..Default::default()
            }),
            constraints: vec![],
            spec_path: None,
        },
        SpecFile {
            version: "2.2".into(),
            module: "auth".into(),
            package: None,
            import_id: None,
            import_ids: vec![],
            description: None,
            boundaries: Some(Boundaries {
                allow_imports_from: Some(vec!["core".into()]),
                ..Default::default()
            }),
            constraints: vec![],
            spec_path: None,
        },
        SpecFile {
            version: "2.2".into(),
            module: "standalone".into(),
            package: None,
            import_id: None,
            import_ids: vec![],
            description: None,
            boundaries: None, // no boundaries
            constraints: vec![],
            spec_path: None,
        },
    ];

    let edges = dependency_edges_from_specs(&specs);

    // api → auth, api → core, auth → core
    assert_eq!(edges.len(), 3);

    // Verify api's edges
    let api_edges: Vec<_> = edges.iter().filter(|e| e.importer == "api").collect();
    assert_eq!(api_edges.len(), 2);
    assert!(api_edges.iter().any(|e| e.provider == "auth"));
    assert!(api_edges.iter().any(|e| e.provider == "core"));

    // Verify auth's edge
    let auth_edges: Vec<_> = edges.iter().filter(|e| e.importer == "auth").collect();
    assert_eq!(auth_edges.len(), 1);
    assert_eq!(auth_edges[0].provider, "core");

    // standalone has no edges (no boundaries)
    let standalone_edges: Vec<_> = edges
        .iter()
        .filter(|e| e.importer == "standalone")
        .collect();
    assert!(standalone_edges.is_empty());
}

#[test]
fn empty_allow_imports_from_produces_no_edges() {
    // Some(vec![]) should produce no edges for that module
    let specs = vec![SpecFile {
        version: "2.2".into(),
        module: "isolated".into(),
        package: None,
        import_id: None,
        import_ids: vec![],
        description: None,
        boundaries: Some(Boundaries {
            allow_imports_from: Some(vec![]), // explicitly empty
            ..Default::default()
        }),
        constraints: vec![],
        spec_path: None,
    }];

    let edges = dependency_edges_from_specs(&specs);
    assert!(
        edges.is_empty(),
        "empty allow_imports_from should produce no edges"
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
