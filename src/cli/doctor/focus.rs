use std::collections::BTreeSet;
use std::path::Path;

use crate::cli::{AnalysisArtifacts, LoadedProject};
use crate::deterministic::normalize_repo_relative;
use crate::resolver::{ModuleResolver, ModuleResolverOptions, ResolvedImport};

use super::DoctorCompareArgs;
use super::trace_types::TraceResultKind;
use super::types::{DoctorCompareFocusOutput, DoctorCompareResolutionOutput};

#[derive(Debug, Clone)]
pub(super) struct DoctorCompareFocus {
    pub(super) edge: Option<(String, String)>,
    pub(super) output: DoctorCompareFocusOutput,
    pub(super) specgate_resolution: DoctorCompareResolutionOutput,
}

pub(super) fn build_doctor_compare_focus(
    loaded: &LoadedProject,
    artifacts: &AnalysisArtifacts,
    args: &DoctorCompareArgs,
) -> std::result::Result<Option<DoctorCompareFocus>, String> {
    match (&args.from, &args.import_specifier) {
        (None, None) => Ok(None),
        (Some(_), None) | (None, Some(_)) => {
            Err("`--from` and `--import` must be provided together".to_string())
        }
        (Some(from), Some(import_specifier)) => {
            let from_file = crate::cli::resolve_against_root(&loaded.project_root, from);
            let from_normalized = normalize_repo_relative(&loaded.project_root, &from_file);

            let mut resolver = ModuleResolver::new_with_options(
                &loaded.project_root,
                &loaded.specs,
                ModuleResolverOptions {
                    include_dirs: loaded.config.include_dirs.clone(),
                    tsconfig_filename: loaded.config.tsconfig_filename.clone(),
                },
            )
            .map_err(|error| format!("failed to initialize module resolver: {error}"))?;
            let explanation = resolver.explain_resolution(&from_file, import_specifier);
            let specgate_trace = explanation.steps.clone();

            let (edge, resolved_to, resolution_kind) = match &explanation.result {
                ResolvedImport::FirstParty { resolved_path, .. } => {
                    let to = normalize_repo_relative(&loaded.project_root, resolved_path);
                    (
                        Some((from_normalized.clone(), to.clone())),
                        Some(to),
                        "first_party".to_string(),
                    )
                }
                ResolvedImport::ThirdParty { package_name } => {
                    (None, Some(package_name.clone()), "third_party".to_string())
                }
                ResolvedImport::Unresolvable { .. } => (None, None, "unresolvable".to_string()),
            };

            let in_specgate_graph = edge
                .as_ref()
                .is_some_and(|edge| artifacts.edge_pairs.contains(edge));

            let specgate_resolution = doctor_resolution_from_specgate(
                &loaded.project_root,
                &explanation.result,
                specgate_trace.clone(),
            );

            Ok(Some(DoctorCompareFocus {
                edge,
                output: DoctorCompareFocusOutput {
                    from: from_normalized,
                    import_specifier: import_specifier.clone(),
                    resolved_to,
                    resolution_kind,
                    in_specgate_graph,
                    specgate_trace,
                },
                specgate_resolution,
            }))
        }
    }
}

pub(super) fn filter_edges_for_focus(
    edges: &BTreeSet<(String, String)>,
    focus: Option<&DoctorCompareFocus>,
) -> BTreeSet<(String, String)> {
    if let Some(focus) = focus {
        if let Some(edge) = &focus.edge {
            return edges
                .iter()
                .filter(|candidate| *candidate == edge)
                .cloned()
                .collect();
        }

        return BTreeSet::new();
    }

    edges.clone()
}

pub(super) fn doctor_resolution_from_specgate(
    project_root: &Path,
    result: &ResolvedImport,
    trace: Vec<String>,
) -> DoctorCompareResolutionOutput {
    match result {
        ResolvedImport::FirstParty {
            resolved_path,
            module_id: _,
        } => DoctorCompareResolutionOutput {
            source: "specgate".to_string(),
            result_kind: TraceResultKind::FirstParty,
            resolved_to: Some(normalize_repo_relative(project_root, resolved_path)),
            package_name: None,
            trace,
        },
        ResolvedImport::ThirdParty { package_name } => DoctorCompareResolutionOutput {
            source: "specgate".to_string(),
            result_kind: TraceResultKind::ThirdParty,
            resolved_to: None,
            package_name: Some(package_name.clone()),
            trace,
        },
        ResolvedImport::Unresolvable { reason, .. } => {
            let mut trace = trace;
            trace.push(format!("resolution error: {reason}"));
            DoctorCompareResolutionOutput {
                source: "specgate".to_string(),
                result_kind: TraceResultKind::Unresolvable,
                resolved_to: None,
                package_name: None,
                trace,
            }
        }
    }
}
