use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::git_blast::BlastRadius;
use crate::graph::{DependencyGraph, EdgeKind, EdgeType};
use crate::resolver::{ModuleResolver, ModuleResolverOptions};
use crate::rules::HYGIENE_UNRESOLVED_IMPORT_RULE_ID;
use crate::rules::boundary::evaluate_boundary_rules;
use crate::rules::{
    DependencyRule, RuleContext, RuleWithResolver, evaluate_enforce_category,
    evaluate_enforce_layer, evaluate_hygiene_rules, evaluate_no_circular_deps,
    evaluate_unique_export,
};
use crate::spec::Severity;
use crate::verdict::{self, EdgeClassification, PolicyViolation, UnresolvedEdge, VerdictEdge};

use super::{
    AnalysisArtifacts, CliRunResult, LoadedProject, boundary_violation_severity,
    build_blast_radius, default_hygiene_rule_severity, dependency_violation_severity,
    derive_blast_edge_pairs, load_project_for_analysis, normalize_repo_relative,
    runtime_error_json, severity_for_constraint_rule,
};

#[derive(Debug, Clone)]
pub(crate) struct PreparedAnalysisContext {
    pub(crate) loaded: LoadedProject,
    pub(crate) artifacts: AnalysisArtifacts,
    pub(crate) blast_radius: Option<BlastRadius>,
}

/// Build edge classification and unresolved edge list from a dependency graph.
///
/// Uses the graph's tracked unresolved imports (external and unresolvable) plus
/// the first-party edges already in the graph.
pub(crate) fn build_edge_classification(
    project_root: &std::path::Path,
    graph: &DependencyGraph,
    canonical_import_ids: &BTreeSet<String>,
    policy: crate::spec::config::UnresolvedEdgePolicy,
) -> (EdgeClassification, Vec<UnresolvedEdge>) {
    use crate::spec::config::UnresolvedEdgePolicy;

    let mut resolved = 0usize;
    let mut external = 0usize;
    let mut type_only = 0usize;

    // Count first-party resolved edges.
    // Note: first-party imports that resolve to files outside the TS/JS discovery
    // set (e.g., .json files) are not added as graph edges, so they are not counted
    // here. This is intentional — those files are not part of the analysis boundary.
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
        // Imports suppressed by @specgate-ignore comments are excluded from
        // all counts and never reported as unresolved edges.
        if record.ignored_by_comment {
            continue;
        }

        let edge_type = unresolved_record_edge_type(record, canonical_import_ids);
        let kind_str = match edge_type {
            EdgeType::External => {
                external += 1;
                continue;
            }
            EdgeType::UnresolvedDynamic => {
                unresolved_dynamic += 1;
                edge_type.as_str()
            }
            EdgeType::UnresolvedLiteral => {
                unresolved_literal += 1;
                edge_type.as_str()
            }
            EdgeType::Resolved => {
                unreachable!("unresolved import records should not resolve to {edge_type:?}")
            }
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
    unresolved_edges.sort_by(|a, b| {
        a.from
            .cmp(&b.from)
            .then_with(|| a.specifier.cmp(&b.specifier))
    });

    let classification = EdgeClassification {
        resolved,
        unresolved_literal,
        unresolved_dynamic,
        external,
        type_only,
    };

    (classification, unresolved_edges)
}

pub(crate) fn build_verdict_edges(
    project_root: &std::path::Path,
    graph: &DependencyGraph,
    canonical_import_ids: &BTreeSet<String>,
) -> Vec<VerdictEdge> {
    let mut verdict_edges = graph
        .dependency_edges()
        .into_iter()
        .filter(|edge| edge.kind != EdgeKind::TypeOnlyImport)
        .map(|edge| VerdictEdge {
            from_module: graph.module_of_file(&edge.from).map(str::to_string),
            to_module: graph.module_of_file(&edge.to).map(str::to_string),
            edge_type: EdgeType::Resolved,
            import_path: edge.specifier,
            file: normalize_repo_relative(project_root, &edge.from),
            line: edge.line,
        })
        .collect::<Vec<_>>();

    verdict_edges.extend(
        graph
            .unresolved_imports()
            .iter()
            .filter(|record| !record.ignored_by_comment)
            .map(|record| VerdictEdge {
                from_module: graph.module_of_file(&record.from).map(str::to_string),
                to_module: None,
                edge_type: unresolved_record_edge_type(record, canonical_import_ids),
                import_path: record.specifier.clone(),
                file: normalize_repo_relative(project_root, &record.from),
                line: record.line,
            }),
    );

    verdict_edges.sort_by(|a, b| {
        a.file
            .cmp(&b.file)
            .then_with(|| a.line.cmp(&b.line))
            .then_with(|| a.import_path.cmp(&b.import_path))
            .then_with(|| a.edge_type.cmp(&b.edge_type))
            .then_with(|| a.from_module.cmp(&b.from_module))
            .then_with(|| a.to_module.cmp(&b.to_module))
    });

    verdict_edges
}

fn unresolved_edge_severity(
    policy: crate::spec::config::UnresolvedEdgePolicy,
) -> Option<crate::spec::Severity> {
    match policy {
        crate::spec::config::UnresolvedEdgePolicy::Warn => Some(crate::spec::Severity::Warning),
        crate::spec::config::UnresolvedEdgePolicy::Error => Some(crate::spec::Severity::Error),
        crate::spec::config::UnresolvedEdgePolicy::Ignore => None,
    }
}

fn unresolved_record_edge_type(
    record: &crate::graph::UnresolvedImportRecord,
    canonical_import_ids: &BTreeSet<String>,
) -> EdgeType {
    if record.is_external
        || canonical_import_ids.contains(&record.specifier)
        || is_bare_specifier(&record.specifier)
    {
        EdgeType::External
    } else if record.kind == EdgeKind::DynamicImport {
        EdgeType::UnresolvedDynamic
    } else {
        EdgeType::UnresolvedLiteral
    }
}

fn is_bare_specifier(specifier: &str) -> bool {
    !(specifier.starts_with("./") || specifier.starts_with("../") || specifier.starts_with('/'))
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
                edge_type: None,
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
                edge_type: None,
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
                edge_type: None,
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
            severity: spec_by_module
                .get(violation.from_module.as_str())
                .and_then(|spec| {
                    severity_for_constraint_rule(spec, crate::rules::ENFORCE_LAYER_RULE_ID)
                })
                .unwrap_or(Severity::Error),
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
            edge_type: None,
        })
        .collect::<Vec<_>>();
    policy_violations.extend(layer_violations);

    let category_report = evaluate_enforce_category(&loaded.specs, &graph);
    let category_violations = category_report
        .violations
        .into_iter()
        .map(|violation| PolicyViolation {
            rule: crate::rules::ENFORCE_CATEGORY_RULE_ID.to_string(),
            severity: spec_by_module
                .get(violation.from_module.as_str())
                .and_then(|spec| {
                    severity_for_constraint_rule(spec, crate::rules::ENFORCE_CATEGORY_RULE_ID)
                })
                .unwrap_or(Severity::Error),
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
            edge_type: None,
        })
        .collect::<Vec<_>>();
    policy_violations.extend(category_violations);

    let unique_export_report = evaluate_unique_export(&loaded.project_root, &loaded.specs, &graph);
    let unique_export_violations = unique_export_report
        .violations
        .into_iter()
        .map(|violation| {
            let first_file = violation
                .files
                .first()
                .cloned()
                .unwrap_or_else(|| loaded.project_root.clone());
            let second_file = violation.files.get(1).cloned();
            PolicyViolation {
                rule: crate::rules::UNIQUE_EXPORT_RULE_ID.to_string(),
                severity: spec_by_module
                    .get(violation.module.as_str())
                    .and_then(|spec| {
                        severity_for_constraint_rule(spec, crate::rules::UNIQUE_EXPORT_RULE_ID)
                    })
                    .unwrap_or(Severity::Error),
                message: violation.message,
                from_file: first_file,
                to_file: second_file,
                from_module: Some(violation.module.clone()),
                to_module: Some(violation.module),
                line: None,
                column: None,
                expected: None,
                actual: None,
                remediation_hint: Some(violation.fix_hint),
                contract_id: None,
                edge_type: None,
            }
        })
        .collect::<Vec<_>>();
    policy_violations.extend(unique_export_violations);

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
            edge_type: None,
        })
        .collect::<Vec<_>>();
    policy_violations.extend(contract_violations);

    let hygiene_violations = evaluate_hygiene_rules(&ctx)
        .into_iter()
        .map(|violation| PolicyViolation {
            severity: violation
                .severity
                .unwrap_or_else(|| default_hygiene_rule_severity(&violation.rule)),
            rule: violation.rule,
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
            edge_type: None,
        })
        .collect::<Vec<_>>();
    policy_violations.extend(hygiene_violations);

    let layer_config_issues = layer_report
        .config_issues
        .into_iter()
        .map(|issue| format!("{}: {}", issue.module, issue.message))
        .collect::<Vec<_>>();

    let category_config_issues = category_report
        .config_issues
        .into_iter()
        .map(|issue| format!("{}: {}", issue.module, issue.message))
        .collect::<Vec<_>>();

    let unique_export_config_issues = unique_export_report
        .config_issues
        .into_iter()
        .map(|issue| format!("{}: {}", issue.module, issue.message))
        .collect::<Vec<_>>();

    verdict::sort_policy_violations(&mut policy_violations);

    let canonical_import_ids = loaded
        .specs
        .iter()
        .flat_map(|spec| spec.canonical_import_ids())
        .collect::<BTreeSet<_>>();

    let (edge_classification, unresolved_edges) = build_edge_classification(
        &loaded.project_root,
        &graph,
        &canonical_import_ids,
        loaded.config.unresolved_edge_policy,
    );
    let verdict_edges = build_verdict_edges(&loaded.project_root, &graph, &canonical_import_ids);

    if let Some(severity) = unresolved_edge_severity(loaded.config.unresolved_edge_policy) {
        for edge in &unresolved_edges {
            policy_violations.push(PolicyViolation {
                rule: HYGIENE_UNRESOLVED_IMPORT_RULE_ID.to_string(),
                severity,
                message: format!("unresolved import: '{}'", edge.specifier),
                from_file: loaded.project_root.join(&edge.from),
                to_file: None,
                from_module: graph
                    .module_of_file(&loaded.project_root.join(&edge.from))
                    .map(str::to_string),
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
                edge_type: match edge.kind.as_str() {
                    "unresolved_dynamic" => Some(EdgeType::UnresolvedDynamic),
                    "unresolved_literal" => Some(EdgeType::UnresolvedLiteral),
                    _ => None,
                },
            });
        }
        verdict::sort_policy_violations(&mut policy_violations);
    }

    Ok(AnalysisArtifacts {
        policy_violations,
        layer_config_issues,
        category_config_issues,
        unique_export_config_issues,
        module_map_overlaps,
        parse_warning_count,
        graph_nodes: graph.node_count(),
        graph_edges: graph.edge_count(),
        suppressed_violations,
        edge_pairs,
        edge_classification,
        verdict_edges,
        unresolved_edges,
    })
}

pub(crate) fn prepare_analysis_context(
    project_root: &Path,
    since_ref: Option<&str>,
) -> std::result::Result<PreparedAnalysisContext, CliRunResult> {
    let loaded = load_project_for_analysis(project_root)?;
    prepare_analysis_for_loaded(loaded, since_ref)
}

pub(crate) fn prepare_analysis_for_loaded(
    loaded: LoadedProject,
    since_ref: Option<&str>,
) -> std::result::Result<PreparedAnalysisContext, CliRunResult> {
    let blast_edge_pairs = match derive_blast_edge_pairs(&loaded, since_ref) {
        Ok(edge_pairs) => edge_pairs,
        Err(error) => {
            return Err(runtime_error_json(
                "git",
                "failed to compute blast edge pairs",
                vec![error],
            ));
        }
    };

    let blast_radius = match build_blast_radius(&loaded, since_ref, &blast_edge_pairs) {
        Ok(radius) => radius,
        Err(error) => {
            return Err(runtime_error_json(
                "git",
                "failed to compute blast radius",
                vec![error],
            ));
        }
    };

    let affected_modules = blast_radius.as_ref().map(|radius| {
        radius
            .affected_modules
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>()
    });

    let artifacts = match analyze_project(&loaded, affected_modules.as_ref()) {
        Ok(artifacts) => artifacts,
        Err(error) => {
            return Err(runtime_error_json(
                "runtime",
                "failed to analyze project",
                vec![error],
            ));
        }
    };

    if !artifacts.layer_config_issues.is_empty() {
        return Err(runtime_error_json(
            "config",
            "invalid enforce-layer rule configuration",
            artifacts.layer_config_issues.clone(),
        ));
    }

    if !artifacts.category_config_issues.is_empty() {
        return Err(runtime_error_json(
            "config",
            "invalid enforce-category rule configuration",
            artifacts.category_config_issues.clone(),
        ));
    }

    if !artifacts.unique_export_config_issues.is_empty() {
        return Err(runtime_error_json(
            "config",
            "invalid boundary.unique_export rule configuration",
            artifacts.unique_export_config_issues.clone(),
        ));
    }

    Ok(PreparedAnalysisContext {
        loaded,
        artifacts,
        blast_radius,
    })
}

#[cfg(test)]
mod tests {
    use crate::spec::config::UnresolvedEdgePolicy;

    use super::{analyze_project, build_edge_classification};

    #[test]
    fn ignored_unresolved_import_not_counted_in_classification() {
        // An ignored unresolved import (ignored_by_comment = true) should not
        // be counted in unresolved_literal or unresolved_dynamic totals and
        // should not appear in the unresolved_edges output.
        use std::fs;
        use tempfile::TempDir;

        use std::collections::BTreeSet;

        use crate::graph::DependencyGraph;
        use crate::resolver::ModuleResolver;
        use crate::spec::{Boundaries, SpecConfig, SpecFile};

        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path().join("src")).expect("mkdir");

        // File with one ignored unresolvable import and one non-ignored unresolvable import
        fs::write(
            temp.path().join("src/main.ts"),
            "// @specgate-ignore: temporary\nimport { x } from './missing-ignored';\nimport { y } from './missing-normal';\nconsole.log(x, y);\n",
        )
        .expect("write main");

        let spec = SpecFile {
            version: "2.2".to_string(),
            module: "app".to_string(),
            package: None,
            import_id: None,
            import_ids: Vec::new(),
            description: None,
            boundaries: Some(Boundaries {
                path: Some("src/**/*".to_string()),
                ..Boundaries::default()
            }),
            constraints: Vec::new(),
            spec_path: None,
        };

        let mut resolver = ModuleResolver::new(temp.path(), &[spec]).expect("resolver");
        let config = SpecConfig::default();
        let graph = DependencyGraph::build(temp.path(), &mut resolver, &config).expect("build");

        let (classification, edges) = build_edge_classification(
            temp.path(),
            &graph,
            &BTreeSet::new(),
            UnresolvedEdgePolicy::Warn,
        );

        // Only the non-ignored import should be counted
        assert_eq!(
            classification.unresolved_literal, 1,
            "ignored import should not be counted: got {classification:?}"
        );
        // And it should not appear in the reported edges
        assert_eq!(
            edges.len(),
            1,
            "ignored edge should be excluded from unresolved_edges"
        );
        assert!(
            edges[0].specifier.contains("missing-normal"),
            "only the non-ignored specifier should appear"
        );
    }

    #[test]
    fn unresolved_dynamic_escalated_when_policy_error() {
        // When unresolved_edge_policy=Error, dynamic imports that fail to resolve
        // should also appear in unresolved_edges (not only literal imports).
        use std::fs;
        use tempfile::TempDir;

        use std::collections::BTreeSet;

        use crate::graph::DependencyGraph;
        use crate::resolver::ModuleResolver;
        use crate::spec::{Boundaries, SpecConfig, SpecFile};

        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path().join("src")).expect("mkdir");

        // One static unresolvable + one dynamic unresolvable
        fs::write(
            temp.path().join("src/main.ts"),
            "import { x } from './missing-static';\nasync function load() { await import('./missing-dynamic'); }\nconsole.log(x);\n",
        )
        .expect("write main");

        let spec = SpecFile {
            version: "2.2".to_string(),
            module: "app".to_string(),
            package: None,
            import_id: None,
            import_ids: Vec::new(),
            description: None,
            boundaries: Some(Boundaries {
                path: Some("src/**/*".to_string()),
                ..Boundaries::default()
            }),
            constraints: Vec::new(),
            spec_path: None,
        };

        let mut resolver = ModuleResolver::new(temp.path(), &[spec]).expect("resolver");
        let config = SpecConfig::default();
        let graph = DependencyGraph::build(temp.path(), &mut resolver, &config).expect("build");

        let (classification, edges) = build_edge_classification(
            temp.path(),
            &graph,
            &BTreeSet::new(),
            UnresolvedEdgePolicy::Error,
        );

        assert_eq!(
            classification.unresolved_literal, 1,
            "one literal unresolved"
        );
        assert_eq!(
            classification.unresolved_dynamic, 1,
            "one dynamic unresolved"
        );

        // Both kinds should appear in unresolved_edges when policy is Error
        assert_eq!(
            edges.len(),
            2,
            "both literal and dynamic unresolved imports should appear when policy=Error; got {edges:?}"
        );
        let kinds: Vec<&str> = edges.iter().map(|e| e.kind.as_str()).collect();
        assert!(
            kinds.contains(&"unresolved_literal"),
            "literal should be in edges"
        );
        assert!(
            kinds.contains(&"unresolved_dynamic"),
            "dynamic should be in edges"
        );
    }

    #[test]
    fn bare_and_canonical_unresolved_imports_count_as_external() {
        use std::collections::BTreeSet;
        use std::fs;
        use tempfile::TempDir;

        use crate::graph::DependencyGraph;
        use crate::resolver::ModuleResolver;
        use crate::spec::{Boundaries, SpecConfig, SpecFile};

        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path().join("src")).expect("mkdir");
        fs::write(
            temp.path().join("src/main.ts"),
            "import expressRouter from 'express/lib/router/index';\nimport { runAdapter } from '@app/registry';\nconsole.log(expressRouter, runAdapter);\n",
        )
        .expect("write main");

        let spec = SpecFile {
            version: "2.2".to_string(),
            module: "app".to_string(),
            package: None,
            import_id: None,
            import_ids: Vec::new(),
            description: None,
            boundaries: Some(Boundaries {
                path: Some("src/**/*".to_string()),
                ..Boundaries::default()
            }),
            constraints: Vec::new(),
            spec_path: None,
        };

        let mut resolver = ModuleResolver::new(temp.path(), &[spec]).expect("resolver");
        let config = SpecConfig::default();
        let graph = DependencyGraph::build(temp.path(), &mut resolver, &config).expect("build");

        let canonical_import_ids = BTreeSet::from(["@app/registry".to_string()]);
        let (classification, edges) = build_edge_classification(
            temp.path(),
            &graph,
            &canonical_import_ids,
            UnresolvedEdgePolicy::Warn,
        );

        assert_eq!(classification.external, 2, "{classification:?}");
        assert_eq!(classification.unresolved_literal, 0, "{classification:?}");
        assert_eq!(classification.unresolved_dynamic, 0, "{classification:?}");
        assert!(
            edges.is_empty(),
            "external-like unresolved imports should not emit hygiene edges"
        );
    }

    // ---------------------------------------------------------------------------
    // M3: severity should be read from the spec constraint, not hardcoded Error
    // ---------------------------------------------------------------------------

    fn minimal_loaded_project(
        temp: &tempfile::TempDir,
        specs: Vec<crate::spec::SpecFile>,
    ) -> crate::cli::LoadedProject {
        use crate::cli::LoadedProject;
        use crate::spec::{SpecConfig, ValidationReport};
        LoadedProject {
            project_root: temp.path().to_path_buf(),
            config: SpecConfig::default(),
            specs,
            validation: ValidationReport { issues: Vec::new() },
        }
    }

    #[test]
    fn enforce_layer_violation_respects_warning_severity() {
        use std::fs;
        use tempfile::TempDir;

        use crate::spec::{Boundaries, Constraint, Severity, SpecFile};

        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path().join("data")).expect("mkdir data");
        fs::create_dir_all(temp.path().join("ui")).expect("mkdir ui");

        fs::write(
            temp.path().join("data/store.ts"),
            "import { comp } from '../ui/comp';\nexport const store = comp;\n",
        )
        .expect("write store");
        fs::write(
            temp.path().join("ui/comp.ts"),
            "export const comp = 'comp';\n",
        )
        .expect("write comp");

        // layers = ["ui", "data"]: ui is index 0, data is index 1.
        // data importing ui means to_index(0) < from_index(1) → violation.
        let layer_params = serde_json::json!({ "layers": ["ui", "data"] });

        let data_spec = SpecFile {
            version: "2.2".to_string(),
            module: "data/store".to_string(),
            package: None,
            import_id: None,
            import_ids: Vec::new(),
            description: None,
            boundaries: Some(Boundaries {
                path: Some("data/**/*".to_string()),
                ..Boundaries::default()
            }),
            constraints: vec![Constraint {
                rule: "enforce-layer".to_string(),
                params: layer_params,
                severity: Severity::Warning,
                message: None,
            }],
            spec_path: None,
        };

        let ui_spec = SpecFile {
            version: "2.2".to_string(),
            module: "ui/comp".to_string(),
            package: None,
            import_id: None,
            import_ids: Vec::new(),
            description: None,
            boundaries: Some(Boundaries {
                path: Some("ui/**/*".to_string()),
                ..Boundaries::default()
            }),
            constraints: Vec::new(),
            spec_path: None,
        };

        let loaded = minimal_loaded_project(&temp, vec![data_spec, ui_spec]);
        let artifacts = analyze_project(&loaded, None).expect("analyze");

        let layer_violations: Vec<_> = artifacts
            .policy_violations
            .iter()
            .filter(|v| v.rule == "enforce-layer")
            .collect();

        assert!(
            !layer_violations.is_empty(),
            "expected at least one enforce-layer violation"
        );
        for v in &layer_violations {
            assert_eq!(
                v.severity,
                Severity::Warning,
                "enforce-layer severity should be Warning when spec declares it, got {:?}",
                v.severity
            );
        }
    }

    #[test]
    fn enforce_category_violation_respects_warning_severity() {
        use std::fs;
        use tempfile::TempDir;

        use crate::spec::{Boundaries, Constraint, Severity, SpecFile};

        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path().join("data")).expect("mkdir data");
        fs::create_dir_all(temp.path().join("ui")).expect("mkdir ui");

        fs::write(
            temp.path().join("data/store.ts"),
            "import { comp } from '../ui/comp';\nexport const store = comp;\n",
        )
        .expect("write store");
        fs::write(
            temp.path().join("ui/comp.ts"),
            "export const comp = 'comp';\n",
        )
        .expect("write comp");

        // Both "data" and "ui" categories are in the governed member set.
        // data/store (category "data") importing ui/comp (category "ui") crosses
        // categories within the governed set → violation.
        let category_params =
            serde_json::json!({ "category": "backend", "members": ["data", "ui"] });

        let data_spec = SpecFile {
            version: "2.2".to_string(),
            module: "data/store".to_string(),
            package: None,
            import_id: None,
            import_ids: Vec::new(),
            description: None,
            boundaries: Some(Boundaries {
                path: Some("data/**/*".to_string()),
                ..Boundaries::default()
            }),
            constraints: vec![Constraint {
                rule: "enforce-category".to_string(),
                params: category_params,
                severity: Severity::Warning,
                message: None,
            }],
            spec_path: None,
        };

        let ui_spec = SpecFile {
            version: "2.2".to_string(),
            module: "ui/comp".to_string(),
            package: None,
            import_id: None,
            import_ids: Vec::new(),
            description: None,
            boundaries: Some(Boundaries {
                path: Some("ui/**/*".to_string()),
                ..Boundaries::default()
            }),
            constraints: Vec::new(),
            spec_path: None,
        };

        let loaded = minimal_loaded_project(&temp, vec![data_spec, ui_spec]);
        let artifacts = analyze_project(&loaded, None).expect("analyze");

        let category_violations: Vec<_> = artifacts
            .policy_violations
            .iter()
            .filter(|v| v.rule == "enforce-category")
            .collect();

        assert!(
            !category_violations.is_empty(),
            "expected at least one enforce-category violation"
        );
        for v in &category_violations {
            assert_eq!(
                v.severity,
                Severity::Warning,
                "enforce-category severity should be Warning when spec declares it, got {:?}",
                v.severity
            );
        }
    }

    #[test]
    fn unique_export_violation_respects_warning_severity() {
        use std::fs;
        use tempfile::TempDir;

        use crate::spec::{Boundaries, Constraint, Severity, SpecFile};

        let temp = TempDir::new().expect("tempdir");
        fs::create_dir_all(temp.path().join("src")).expect("mkdir src");

        // Two files in the same module both export `Button` → unique_export violation.
        fs::write(
            temp.path().join("src/button-a.ts"),
            "export const Button = 'ButtonA';\n",
        )
        .expect("write button-a");
        fs::write(
            temp.path().join("src/button-b.ts"),
            "export const Button = 'ButtonB';\n",
        )
        .expect("write button-b");

        let spec = SpecFile {
            version: "2.2".to_string(),
            module: "ui".to_string(),
            package: None,
            import_id: None,
            import_ids: Vec::new(),
            description: None,
            boundaries: Some(Boundaries {
                path: Some("src/**/*".to_string()),
                ..Boundaries::default()
            }),
            constraints: vec![Constraint {
                rule: "boundary.unique_export".to_string(),
                params: serde_json::json!({}),
                severity: Severity::Warning,
                message: None,
            }],
            spec_path: None,
        };

        let loaded = minimal_loaded_project(&temp, vec![spec]);
        let artifacts = analyze_project(&loaded, None).expect("analyze");

        let unique_export_violations: Vec<_> = artifacts
            .policy_violations
            .iter()
            .filter(|v| v.rule == "boundary.unique_export")
            .collect();

        assert!(
            !unique_export_violations.is_empty(),
            "expected at least one boundary.unique_export violation"
        );
        for v in &unique_export_violations {
            assert_eq!(
                v.severity,
                Severity::Warning,
                "boundary.unique_export severity should be Warning when spec declares it, got {:?}",
                v.severity
            );
        }
    }
}
