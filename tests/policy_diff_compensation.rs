//! Integration tests for cross-file compensation in policy-diff.

use specgate::policy::compensate::{dependency_edges_from_specs, find_compensation_candidates};
use specgate::policy::types::{
    ChangeClassification, ChangeScope, CompensationCandidate, CompensationResult, DependencyEdge,
    FieldChange, ModulePolicyDiff, PolicyDiffReport, PolicyDiffSummary,
};
use specgate::spec::{Boundaries, SpecFile};

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

#[test]
fn same_field_connected_modules_produce_offset() {
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
    assert!(candidates.iter().all(|c| c.result == CompensationResult::Ambiguous));
}

#[test]
fn reverse_edge_direction_still_compensates() {
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
    let widenings = vec![make_field_change(
        "auth",
        "public_api",
        ChangeClassification::Widening,
    )];
    let narrowings = vec![make_field_change(
        "auth",
        "public_api",
        ChangeClassification::Narrowing,
    )];
    let edges = vec![DependencyEdge {
        importer: "auth".into(),
        provider: "core".into(),
    }];
    let candidates = find_compensation_candidates(&widenings, &narrowings, &edges);
    assert!(candidates.is_empty(), "same-module changes should not compensate");
}

#[test]
fn multiple_narrowings_same_widening_marked_ambiguous() {
    let widenings = vec![make_field_change(
        "core",
        "public_api",
        ChangeClassification::Widening,
    )];
    let narrowings = vec![
        make_field_change("auth", "public_api", ChangeClassification::Narrowing),
        make_field_change("api", "public_api", ChangeClassification::Narrowing),
    ];
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
    assert_eq!(candidates.len(), 2);
    assert!(candidates.iter().all(|c| c.result == CompensationResult::Ambiguous));
}

#[test]
fn dependency_edges_extracted_from_specs() {
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
            boundaries: None,
            constraints: vec![],
            spec_path: None,
        },
    ];
    let edges = dependency_edges_from_specs(&specs);
    assert_eq!(edges.len(), 3);
}

#[test]
fn empty_allow_imports_from_produces_no_edges() {
    let specs = vec![SpecFile {
        version: "2.2".into(),
        module: "isolated".into(),
        package: None,
        import_id: None,
        import_ids: vec![],
        description: None,
        boundaries: Some(Boundaries {
            allow_imports_from: Some(vec![]),
            ..Default::default()
        }),
        constraints: vec![],
        spec_path: None,
    }];
    let edges = dependency_edges_from_specs(&specs);
    assert!(edges.is_empty());
}

// End-to-end tests

#[test]
fn compensation_enabled_produces_net_structural_for_offset_pair() {
    let widening = make_field_change("auth", "public_api", ChangeClassification::Widening);
    let narrowing = make_field_change("api", "public_api", ChangeClassification::Narrowing);
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

    let summary = PolicyDiffSummary {
        widening_changes: 1,
        narrowing_changes: 1,
        has_widening: true,
        ..Default::default()
    };
    let report = PolicyDiffReport {
        schema_version: "1".into(),
        base_ref: "base".into(),
        head_ref: "HEAD".into(),
        diffs: vec![
            ModulePolicyDiff {
                module: "auth".into(),
                spec_path: "auth/.spec.yml".into(),
                changes: vec![widening],
            },
            ModulePolicyDiff {
                module: "api".into(),
                spec_path: "api/.spec.yml".into(),
                changes: vec![narrowing],
            },
        ],
        summary,
        errors: vec![],
        compensations: vec![candidate],
        net_classification: ChangeClassification::Narrowing,
        config_changes: vec![],
    };
    assert_ne!(report.net_classification, ChangeClassification::Widening);
    assert_eq!(report.net_classification, ChangeClassification::Narrowing);
    assert!(!report.compensations.is_empty());
    assert_eq!(report.compensations[0].result, CompensationResult::Offset);
}

#[test]
fn compensation_disabled_widening_remains_widening() {
    let widening = make_field_change("auth", "public_api", ChangeClassification::Widening);
    let narrowing = make_field_change("api", "public_api", ChangeClassification::Narrowing);
    let summary = PolicyDiffSummary {
        widening_changes: 1,
        narrowing_changes: 1,
        has_widening: true,
        modules_changed: 2,
        ..Default::default()
    };
    let report = PolicyDiffReport {
        schema_version: "1".into(),
        base_ref: "base".into(),
        head_ref: "HEAD".into(),
        diffs: vec![
            ModulePolicyDiff {
                module: "auth".into(),
                spec_path: "auth/.spec.yml".into(),
                changes: vec![widening],
            },
            ModulePolicyDiff {
                module: "api".into(),
                spec_path: "api/.spec.yml".into(),
                changes: vec![narrowing],
            },
        ],
        summary,
        errors: vec![],
        compensations: vec![],
        net_classification: ChangeClassification::Widening,
        config_changes: vec![],
    };
    assert_eq!(report.net_classification, ChangeClassification::Widening);
    assert!(report.compensations.is_empty());
}

#[test]
fn policy_diff_options_defaults() {
    let options = specgate::policy::PolicyDiffOptions::default();
    assert!(!options.cross_file_compensation);
}

#[test]
fn policy_diff_options_with_compensation_enabled() {
    let options = specgate::policy::PolicyDiffOptions {
        cross_file_compensation: true,
    };
    assert!(options.cross_file_compensation);
}

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
