use std::collections::BTreeMap;
use std::path::PathBuf;

use globset::GlobSet;

use super::types::DoctorFindingOutput;
use crate::cli::LoadedProject;
use crate::deterministic::normalize_repo_relative;
use crate::graph::DependencyGraph;
use crate::resolver::{ModuleResolver, ModuleResolverOptions, ResolvedImport};
use crate::rules::compile_optional_globset_strict;
use crate::spec::Severity;

const BOUNDARY_CANONICAL_IMPORT_DANGLING_RULE_ID: &str = "boundary.canonical_import_dangling";

#[derive(Debug, Clone)]
struct CanonicalProbeTarget {
    importer: PathBuf,
    description: String,
}

pub(super) fn canonical_import_findings(
    loaded: &LoadedProject,
) -> std::result::Result<Vec<DoctorFindingOutput>, String> {
    let mut resolver = ModuleResolver::new_with_options(
        &loaded.project_root,
        &loaded.specs,
        ModuleResolverOptions {
            include_dirs: loaded.config.include_dirs.clone(),
            tsconfig_filename: loaded.config.tsconfig_filename.clone(),
        },
    )
    .map_err(|error| format!("failed to initialize module resolver: {error}"))?;
    let graph = DependencyGraph::build(&loaded.project_root, &mut resolver, &loaded.config)
        .map_err(|error| format!("failed to build dependency graph: {error}"))?;
    let public_api_matchers = public_api_matchers(&loaded.specs);

    let mut findings = Vec::new();
    for spec in &loaded.specs {
        let Some(boundaries) = spec.boundaries.as_ref() else {
            continue;
        };
        if !boundaries.enforce_canonical_imports {
            continue;
        }

        let canonical_ids = spec.canonical_import_ids();
        if canonical_ids.is_empty() {
            continue;
        }

        let probe = representative_importer(&graph, &spec.module, &loaded.project_root);
        let matcher = public_api_matchers
            .get(&spec.module)
            .and_then(|matcher| matcher.as_ref());

        for canonical_id in canonical_ids {
            let resolution = resolver.resolve(&probe.importer, &canonical_id);
            match resolution {
                ResolvedImport::FirstParty {
                    resolved_path,
                    module_id,
                } if module_id.as_deref() == Some(spec.module.as_str()) => {
                    let relative = normalize_repo_relative(&loaded.project_root, &resolved_path);
                    let is_public = matcher.is_some_and(|matcher| matcher.is_match(&relative));
                    if !is_public {
                        findings.push(DoctorFindingOutput {
                            rule: BOUNDARY_CANONICAL_IMPORT_DANGLING_RULE_ID.to_string(),
                            severity: Severity::Warning,
                            module: spec.module.clone(),
                            message: format!(
                                "canonical import id '{canonical_id}' resolves to non-public file '{relative}' via representative importer probe ({})"
                                ,
                                probe.description
                            ),
                            spec_path: spec
                                .spec_path
                                .as_ref()
                                .map(|path| normalize_repo_relative(&loaded.project_root, path)),
                        });
                    }
                }
                other => findings.push(DoctorFindingOutput {
                    rule: BOUNDARY_CANONICAL_IMPORT_DANGLING_RULE_ID.to_string(),
                    severity: Severity::Warning,
                    module: spec.module.clone(),
                    message: format!(
                        "canonical import id '{canonical_id}' did not resolve to this module's public API via representative importer probe ({}) ({other:?})",
                        probe.description
                    ),
                    spec_path: spec
                        .spec_path
                        .as_ref()
                        .map(|path| normalize_repo_relative(&loaded.project_root, path)),
                }),
            }
        }
    }

    findings.sort_by(|a, b| {
        a.module
            .cmp(&b.module)
            .then_with(|| a.rule.cmp(&b.rule))
            .then_with(|| a.message.cmp(&b.message))
    });
    Ok(findings)
}

fn public_api_matchers(specs: &[crate::spec::SpecFile]) -> BTreeMap<String, Option<GlobSet>> {
    let mut matchers = BTreeMap::new();
    for spec in specs {
        let matcher = spec
            .boundaries
            .as_ref()
            .filter(|boundaries| !boundaries.public_api.is_empty())
            .and_then(|boundaries| compile_optional_globset_strict(&boundaries.public_api).ok())
            .flatten();
        matchers.insert(spec.module.clone(), matcher);
    }
    matchers
}

fn representative_importer(
    graph: &DependencyGraph,
    module_id: &str,
    project_root: &std::path::Path,
) -> CanonicalProbeTarget {
    if let Some(node) = graph
        .files()
        .into_iter()
        .find(|node| node.module_id.as_deref() != Some(module_id))
    {
        let relative = normalize_repo_relative(project_root, &node.path);
        return CanonicalProbeTarget {
            importer: node.path.clone(),
            description: format!("importer '{relative}' outside module '{module_id}'"),
        };
    }

    if let Some(node) = graph.files_in_module(module_id).into_iter().next() {
        let relative = normalize_repo_relative(project_root, &node.path);
        return CanonicalProbeTarget {
            importer: node.path.clone(),
            description: format!("fallback importer '{relative}' from module '{module_id}'"),
        };
    }

    let synthetic = project_root.join("specgate-doctor-canonical.ts");
    let relative = normalize_repo_relative(project_root, &synthetic);
    CanonicalProbeTarget {
        importer: synthetic,
        description: format!(
            "synthetic importer '{relative}' because no graph files were available"
        ),
    }
}
