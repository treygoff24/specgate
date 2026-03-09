use crate::graph::{DependencyGraph, EdgeKind};

use super::*;

/// Build edge classification and unresolved edge list from a dependency graph.
///
/// Uses the graph's tracked unresolved imports (external and unresolvable) plus
/// the first-party edges already in the graph.
pub(crate) fn build_edge_classification(
    project_root: &std::path::Path,
    graph: &DependencyGraph,
    policy: crate::spec::config::UnresolvedEdgePolicy,
) -> (EdgeClassification, Vec<UnresolvedEdge>) {
    use crate::spec::config::UnresolvedEdgePolicy;

    let mut resolved = 0usize;
    let mut external = 0usize;
    let mut type_only = 0usize;

    // Count first-party resolved edges
    for edge in graph.dependency_edges() {
        if edge.kind == EdgeKind::TypeOnlyImport {
            type_only += 1;
        } else {
            resolved += 1;
        }
    }

    let mut unresolved_literal = 0usize;
    let mut unresolved_dynamic = 0usize;
    let mut unresolved_edges: Vec<UnresolvedEdge> = Vec::new();

    for record in graph.unresolved_imports() {
        if record.is_external {
            external += 1;
            // External imports are not reported as unresolved edges
            continue;
        }

        let kind_str = if record.kind == EdgeKind::DynamicImport {
            unresolved_dynamic += 1;
            "unresolved_dynamic"
        } else {
            unresolved_literal += 1;
            "unresolved_literal"
        };

        if matches!(policy, UnresolvedEdgePolicy::Ignore) {
            continue;
        }

        unresolved_edges.push(UnresolvedEdge {
            from: normalize_repo_relative(project_root, &record.from),
            specifier: record.specifier.clone(),
            kind: kind_str.to_string(),
            line: record.line,
        });
    }

    // Sort deterministically: by from, then specifier
    unresolved_edges.sort_by(|a, b| a.from.cmp(&b.from).then_with(|| a.specifier.cmp(&b.specifier)));

    let classification = EdgeClassification {
        resolved,
        unresolved_literal,
        unresolved_dynamic,
        external,
        type_only,
    };

    (classification, unresolved_edges)
}

pub(crate) fn analyze_project(
    loaded: &LoadedProject,
    affected_modules: Option<&BTreeSet<String>>,
) -> std::result::Result<AnalysisArtifacts, String> {
    let mut resolver = ModuleResolver::new_with_options(
        &loaded.project_root,
        &loaded.specs,
        ModuleResolverOptions {
            include_dirs: loaded.config.include_dirs.clone(),
            tsconfig_filename: loaded.config.tsconfig_filename.clone(),
        },
    )
    .map_err(|error| format!("failed to initialize module resolver: {error}"))?;
    let module_map_overlaps = resolver.module_map_overlaps().to_vec();

    let graph = DependencyGraph::build(&loaded.project_root, &mut resolver, &loaded.config)
        .map_err(|error| format!("failed to build dependency graph: {error}"))?;

    let parse_warning_count = graph
        .files()
        .into_iter()
        .map(|node| node.analysis.parse_warnings.len())
        .sum();

    let suppressed_violations = graph
        .dependency_edges()
        .iter()
        .filter(|edge| edge.ignored_by_comment)
        .count();

    let edge_pairs = graph
        .dependency_edges()
        .into_iter()
        .map(|edge| {
            (
                normalize_repo_relative(&loaded.project_root, &edge.from),
                normalize_repo_relative(&loaded.project_root, &edge.to),
            )
        })
        .collect::<BTreeSet<_>>();

    let ctx = RuleContext {
        project_root: &loaded.project_root,
        config: &loaded.config,
        specs: &loaded.specs,
        graph: &graph,
    };

    let mut policy_violations = Vec::new();
    let spec_by_module = loaded
        .specs
        .iter()
        .map(|spec| (spec.module.as_str(), spec))
        .collect::<BTreeMap<_, _>>();

    let boundary_violations = evaluate_boundary_rules(&ctx)
        .into_iter()
        .map(|violation| {
            let severity = boundary_violation_severity(&violation, &spec_by_module);

            PolicyViolation {
                rule: violation.rule,
                severity,
                message: violation.message,
                from_file: violation.from_file,
                to_file: violation.to_file,
                from_module: violation.from_module,
                to_module: violation.to_module,
                line: violation.line,
                column: violation.column,
                expected: None,
                actual: None,
                remediation_hint: None,
                contract_id: None,
            }
        })
        .collect::<Vec<_>>();
    policy_violations.extend(boundary_violations);

    let dependency_violations = DependencyRule
        .evaluate_with_resolver(&ctx, &mut resolver)
        .map_err(|error| format!("failed to evaluate dependency rules: {error}"))?
        .into_iter()
        .map(|violation| {
            let severity = dependency_violation_severity(violation.rule.as_str());

            PolicyViolation {
                rule: violation.rule,
                severity,
                message: violation.message,
                from_file: violation.from_file,
                to_file: violation.to_file,
                from_module: violation.from_module,
                to_module: violation.to_module,
                line: violation.line,
                column: violation.column,
                expected: None,
                actual: None,
                remediation_hint: None,
                contract_id: None,
            }
        })
        .collect::<Vec<_>>();
    policy_violations.extend(dependency_violations);

    let circular_violations = evaluate_no_circular_deps(&loaded.specs, &graph)
        .into_iter()
        .map(|violation| {
            let from_file = violation
                .component_files
                .first()
                .cloned()
                .unwrap_or_else(|| loaded.project_root.clone());

            PolicyViolation {
                rule: violation.rule,
                severity: violation.severity,
                message: violation.message,
                from_file,
                to_file: None,
                from_module: Some(violation.module),
                to_module: None,
                line: None,
                column: None,
                expected: None,
                actual: None,
                remediation_hint: None,
                contract_id: None,
            }
        })
        .collect::<Vec<_>>();
    policy_violations.extend(circular_violations);

    let layer_report = evaluate_enforce_layer(&loaded.specs, &graph);
    let layer_violations = layer_report
        .violations
        .into_iter()
        .map(|violation| PolicyViolation {
            rule: crate::rules::ENFORCE_LAYER_RULE_ID.to_string(),
            severity: Severity::Error,
            message: violation.message,
            from_file: violation.from_file,
            to_file: Some(violation.to_file),
            from_module: Some(violation.from_module),
            to_module: Some(violation.to_module),
            line: None,
            column: None,
            expected: None,
            actual: None,
            remediation_hint: Some(violation.fix_hint),
            contract_id: None,
        })
        .collect::<Vec<_>>();
    policy_violations.extend(layer_violations);

    // Evaluate contract rules with affected_modules scoping
    let contract_violations = crate::rules::evaluate_contract_rules(&ctx, affected_modules)
        .into_iter()
        .map(|contract_violation| PolicyViolation {
            rule: contract_violation.violation.rule,
            severity: contract_violation.severity,
            message: contract_violation.violation.message,
            from_file: contract_violation.violation.from_file,
            to_file: contract_violation.violation.to_file,
            from_module: contract_violation.violation.from_module,
            to_module: contract_violation.violation.to_module,
            line: contract_violation.violation.line,
            column: contract_violation.violation.column,
            expected: None,
            actual: None,
            remediation_hint: Some(contract_violation.remediation_hint),
            contract_id: Some(contract_violation.contract_id),
        })
        .collect::<Vec<_>>();
    policy_violations.extend(contract_violations);

    let layer_config_issues = layer_report
        .config_issues
        .into_iter()
        .map(|issue| format!("{}: {}", issue.module, issue.message))
        .collect::<Vec<_>>();

    verdict::sort_policy_violations(&mut policy_violations);

    let (edge_classification, unresolved_edges) = build_edge_classification(
        &loaded.project_root,
        &graph,
        loaded.config.unresolved_edge_policy,
    );

    // If policy is Error, generate PolicyViolation entries for unresolved literal imports
    if loaded.config.unresolved_edge_policy == crate::spec::config::UnresolvedEdgePolicy::Error {
        for edge in &unresolved_edges {
            if edge.kind == "unresolved_literal" {
                policy_violations.push(PolicyViolation {
                    rule: "edge.unresolved".to_string(),
                    severity: Severity::Error,
                    message: format!("unresolved import: '{}'", edge.specifier),
                    from_file: std::path::PathBuf::from(&edge.from),
                    to_file: None,
                    from_module: None,
                    to_module: None,
                    line: edge.line,
                    column: None,
                    expected: None,
                    actual: Some(edge.specifier.clone()),
                    remediation_hint: Some(
                        "Verify the import specifier resolves to a file within the project."
                            .to_string(),
                    ),
                    contract_id: None,
                });
            }
        }
        verdict::sort_policy_violations(&mut policy_violations);
    }

    Ok(AnalysisArtifacts {
        policy_violations,
        layer_config_issues,
        module_map_overlaps,
        parse_warning_count,
        graph_nodes: graph.node_count(),
        graph_edges: graph.edge_count(),
        suppressed_violations,
        edge_pairs,
        edge_classification,
        unresolved_edges,
    })
}
